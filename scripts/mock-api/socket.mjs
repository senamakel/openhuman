import crypto from "node:crypto";

import { json, setCors } from "./http.mjs";
import {
  appendSocketEvent,
  attachWebSocketToSession,
  behavior,
  buildSocketReadyPayload,
  createMockId,
  drainSocketPackets,
  getSocketSession,
  listSocketSessions,
  parseBehaviorJson,
  queueSocketPacket,
  registerSocketSession,
  touchSocketSession,
  dropSocketSession,
} from "./state.mjs";

const EIO_PING_INTERVAL = 25_000;
const EIO_PING_TIMEOUT = 20_000;
const EIO_MAX_PAYLOAD = 1_000_000;
const POLLING_SEPARATOR = "\x1e";

function parseRequestUrl(rawUrl) {
  return new URL(rawUrl || "/socket.io/", "http://127.0.0.1");
}

function socketIoSid() {
  return `mock-sio-${createMockId("sid")}`;
}

function engineOpenPacket(sid, upgrades = ["websocket"]) {
  return `0${JSON.stringify({
    sid,
    upgrades,
    pingInterval: EIO_PING_INTERVAL,
    pingTimeout: EIO_PING_TIMEOUT,
    maxPayload: EIO_MAX_PAYLOAD,
  })}`;
}

function socketConnectPacket(session) {
  return `40${JSON.stringify({ sid: session.socketId })}`;
}

function socketConnectErrorPacket(message) {
  return `44${JSON.stringify({ message })}`;
}

function socketEventPacket(event, data) {
  const payload = data === undefined ? [event] : [event, data];
  return `42${JSON.stringify(payload)}`;
}

function encodePollingPayload(packets) {
  return packets.join(POLLING_SEPARATOR);
}

function decodePollingPayload(rawBody) {
  const body = String(rawBody || "");
  if (!body) return [];
  return body.split(POLLING_SEPARATOR).filter(Boolean);
}

function authenticateSession(authPayload) {
  const mockBehavior = behavior();
  const socketAuthMode = mockBehavior.socketAuthMode || "required";
  const token =
    authPayload && typeof authPayload === "object"
      ? authPayload.token
      : undefined;

  if (socketAuthMode !== "disabled" && !token) {
    return { ok: false, message: "No token provided" };
  }
  if (
    mockBehavior.socketAuthMode === "reject" ||
    mockBehavior.socketReject === "true" ||
    String(token || "").includes("invalid")
  ) {
    return {
      ok: false,
      message: mockBehavior.socketRejectMessage || "Authentication failed",
    };
  }

  return {
    ok: true,
    token: token || null,
    userId: mockBehavior.socketUserId || mockBehavior.userId || "mock-user",
  };
}

function normalizeAuthPayload(packet) {
  if (!packet.startsWith("40")) return null;
  const payload = packet.slice(2).trim();
  if (!payload) return {};
  try {
    return JSON.parse(payload);
  } catch {
    return {};
  }
}

function logSocketCheckpoint(kind, detail = {}) {
  appendSocketEvent({ direction: "system", kind, ...detail });
}

function writePollingResponse(res, packets) {
  setCors(res);
  res.writeHead(200, { "Content-Type": "text/plain; charset=UTF-8" });
  res.end(encodePollingPayload(packets));
}

function writePollingOk(res) {
  setCors(res);
  res.writeHead(200, { "Content-Type": "text/plain; charset=UTF-8" });
  res.end("ok");
}

function sendWsFrame(socket, opcode, payload) {
  if (!socket || socket.destroyed) return;

  const len = payload.length;
  let header;
  if (len < 126) {
    header = Buffer.alloc(2);
    header[0] = 0x80 | opcode;
    header[1] = len;
  } else if (len < 65536) {
    header = Buffer.alloc(4);
    header[0] = 0x80 | opcode;
    header[1] = 126;
    header.writeUInt16BE(len, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x80 | opcode;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(len), 2);
  }

  try {
    socket.write(header);
    socket.write(payload);
  } catch {
    // noop
  }
}

function sendWsText(socket, text) {
  sendWsFrame(socket, 0x01, Buffer.from(text, "utf-8"));
}

function sendSocketPacket(session, packet) {
  const target = getSocketSession(session.sid);
  if (!target) return false;
  target.lastSeenAt = new Date().toISOString();
  if (
    target.webSocket &&
    !target.webSocket.destroyed &&
    target.upgradedToWebSocket === true
  ) {
    sendWsText(target.webSocket, packet);
    return true;
  }
  return queueSocketPacket(target.sid, packet);
}

function scheduleMockSocketActions(session, actions = []) {
  for (const action of actions) {
    const delayMs = Math.max(0, Number(action?.delayMs || 0));
    setTimeout(() => {
      if (action?.disconnect === true) {
        disconnectMockSockets({ targetSid: session.sid });
        return;
      }
      if (typeof action?.event === "string" && action.event) {
        emitMockSocketEvent({
          event: action.event,
          data:
            action.data === "__ready__"
              ? buildSocketReadyPayload(session)
              : (action.data ?? null),
          targetSid: session.sid,
        });
      }
    }, delayMs);
  }
}

function onSocketConnected(session) {
  touchSocketSession(session.sid, { connected: true });
  sendSocketPacket(session, socketConnectPacket(session));
  sendSocketPacket(
    session,
    socketEventPacket("ready", buildSocketReadyPayload(session)),
  );

  logSocketCheckpoint("connected", {
    sid: session.sid,
    socketId: session.socketId,
    userId: session.userId,
    transport: session.transport,
  });

  const connectScript = parseBehaviorJson("socketConnectScript", []);
  if (Array.isArray(connectScript) && connectScript.length > 0) {
    scheduleMockSocketActions(session, connectScript);
  }
}

function handleClientSocketEvent(session, event, data) {
  appendSocketEvent({
    direction: "inbound",
    kind: "event",
    sid: session.sid,
    socketId: session.socketId,
    userId: session.userId,
    event,
    data,
  });

  if (event === "webhook:response" && data?.correlationId) {
    sendSocketPacket(
      session,
      socketEventPacket(`webhook:response:${data.correlationId}`, data),
    );
  }

  const scripts = parseBehaviorJson("socketClientEventScripts", {});
  const actions = Array.isArray(scripts?.[event]) ? scripts[event] : [];
  if (actions.length > 0) {
    scheduleMockSocketActions(session, actions);
  }
}

function handleSocketPacket(session, packet) {
  touchSocketSession(session.sid);

  if (packet === "2") {
    sendSocketPacket(session, "3");
    return;
  }

  if (packet === "2probe") {
    sendSocketPacket(session, "3probe");
    return;
  }

  if (packet === "5") {
    touchSocketSession(session.sid, {
      upgradedToWebSocket: true,
      transport: "websocket",
    });
    logSocketCheckpoint("upgrade_complete", { sid: session.sid });
    const pending = drainSocketPackets(session.sid);
    const live = getSocketSession(session.sid);
    if (live?.webSocket && pending.length > 0) {
      for (const queued of pending) {
        sendWsText(live.webSocket, queued);
      }
    }
    return;
  }

  if (packet.startsWith("40")) {
    const auth = normalizeAuthPayload(packet);
    const result = authenticateSession(auth);
    if (!result.ok) {
      sendSocketPacket(session, socketConnectErrorPacket(result.message));
      appendSocketEvent({
        direction: "outbound",
        kind: "connect_error",
        sid: session.sid,
        message: result.message,
      });
      return;
    }
    touchSocketSession(session.sid, {
      token: result.token,
      userId: result.userId,
    });
    onSocketConnected(getSocketSession(session.sid));
    return;
  }

  if (packet.startsWith("42")) {
    try {
      const payload = JSON.parse(packet.slice(2));
      const [event, data] = Array.isArray(payload) ? payload : [];
      if (typeof event === "string" && event) {
        handleClientSocketEvent(session, event, data);
      }
    } catch {
      appendSocketEvent({
        direction: "system",
        kind: "parse_error",
        sid: session.sid,
        packet,
      });
    }
  }
}

function createSession(transport) {
  const sid = socketIoSid();
  const session = registerSocketSession({
    sid,
    socketId: sid,
    transport,
    createdAt: new Date().toISOString(),
  });
  logSocketCheckpoint("session_created", { sid, transport });
  return session;
}

function lookupSessionFromUrl(urlObj) {
  const sid = urlObj.searchParams.get("sid");
  if (!sid) return null;
  return getSocketSession(sid);
}

export function handleSocketRequest(ctx) {
  const { method, url, body, res } = ctx;
  if (!url?.startsWith("/socket.io/")) return false;

  const urlObj = parseRequestUrl(url);
  const transport = urlObj.searchParams.get("transport");
  if (transport !== "polling") {
    json(res, 400, {
      success: false,
      error: "Mock socket only handles polling HTTP",
    });
    return true;
  }

  if (method === "GET") {
    const existing = lookupSessionFromUrl(urlObj);
    if (!existing) {
      const session = createSession("polling");
      writePollingResponse(res, [engineOpenPacket(session.sid)]);
      return true;
    }

    const packets = drainSocketPackets(existing.sid);
    writePollingResponse(res, packets.length > 0 ? packets : ["6"]);
    return true;
  }

  if (method === "POST") {
    const session = lookupSessionFromUrl(urlObj);
    if (!session) {
      json(res, 400, { success: false, error: "Unknown socket session" });
      return true;
    }
    const packets = decodePollingPayload(body);
    for (const packet of packets) {
      handleSocketPacket(session, packet);
    }
    writePollingOk(res);
    return true;
  }

  json(res, 405, { success: false, error: "Method not allowed" });
  return true;
}

function acceptWebSocket(req, socket) {
  const key = req.headers["sec-websocket-key"];
  if (!key) {
    socket.destroy();
    return false;
  }
  const acceptKey = crypto
    .createHash("sha1")
    .update(key + "258EAFA5-E914-47DA-95CA-5AB5DC085B11")
    .digest("base64");
  socket.write(
    "HTTP/1.1 101 Switching Protocols\r\n" +
      "Upgrade: websocket\r\n" +
      "Connection: Upgrade\r\n" +
      `Sec-WebSocket-Accept: ${acceptKey}\r\n` +
      "\r\n",
  );
  return true;
}

function decodeWebSocketFrames(socket, onText) {
  let buffer = Buffer.alloc(0);

  socket.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (buffer.length >= 2) {
      const firstByte = buffer[0];
      const opcode = firstByte & 0x0f;
      const secondByte = buffer[1];
      const masked = (secondByte & 0x80) !== 0;
      let payloadLen = secondByte & 0x7f;
      let offset = 2;

      if (payloadLen === 126) {
        if (buffer.length < 4) return;
        payloadLen = buffer.readUInt16BE(2);
        offset = 4;
      } else if (payloadLen === 127) {
        if (buffer.length < 10) return;
        payloadLen = Number(buffer.readBigUInt64BE(2));
        offset = 10;
      }

      const maskSize = masked ? 4 : 0;
      const totalLen = offset + maskSize + payloadLen;
      if (buffer.length < totalLen) return;

      let payload = buffer.subarray(offset + maskSize, totalLen);
      if (masked) {
        const mask = buffer.subarray(offset, offset + 4);
        payload = Buffer.from(payload);
        for (let i = 0; i < payload.length; i += 1) {
          payload[i] ^= mask[i % 4];
        }
      }

      buffer = buffer.subarray(totalLen);

      if (opcode === 0x08) {
        socket.end();
        return;
      }

      if (opcode === 0x09) {
        sendWsFrame(socket, 0x0a, payload);
        continue;
      }

      if (opcode === 0x01) {
        onText(payload.toString("utf-8"));
      }
    }
  });
}

export function handleWebSocketUpgrade(req, socket) {
  if (!req.url?.startsWith("/socket.io/")) {
    socket.destroy();
    return;
  }

  if (!acceptWebSocket(req, socket)) return;

  const urlObj = parseRequestUrl(req.url);
  const requestedSid = urlObj.searchParams.get("sid");
  let session = requestedSid ? getSocketSession(requestedSid) : null;

  if (!session) {
    session = createSession("websocket");
    attachWebSocketToSession(session.sid, socket, {
      transport: "websocket",
      upgraded: true,
    });
    sendWsText(socket, engineOpenPacket(session.sid, []));
  } else {
    attachWebSocketToSession(session.sid, socket, { upgraded: false });
  }

  logSocketCheckpoint("websocket_attached", { sid: session.sid, requestedSid });

  decodeWebSocketFrames(socket, (packet) =>
    handleSocketPacket(session, packet),
  );

  socket.on("close", () => {
    dropSocketSession(session.sid);
    logSocketCheckpoint("websocket_closed", { sid: session.sid });
  });
  socket.on("error", () => {});
}

function matchSession(session, filters = {}) {
  if (filters.targetSid && session.sid !== filters.targetSid) return false;
  if (filters.excludeSid && session.sid === filters.excludeSid) return false;
  if (filters.targetUserId && session.userId !== filters.targetUserId)
    return false;
  return true;
}

export function emitMockSocketEvent({
  event,
  data,
  targetSid,
  targetUserId,
  excludeSid,
  delayMs = 0,
}) {
  if (typeof event !== "string" || !event) return 0;

  const matchingSessions = listSocketSessions().filter((session) =>
    matchSession(session, { targetSid, targetUserId, excludeSid }),
  );

  const deliver = () => {
    for (const info of matchingSessions) {
      const session = getSocketSession(info.sid);
      if (!session) continue;
      sendSocketPacket(session, socketEventPacket(event, data));
      appendSocketEvent({
        direction: "outbound",
        kind: "event",
        sid: session.sid,
        socketId: session.socketId,
        userId: session.userId,
        event,
        data,
      });
    }
  };

  const normalizedDelay = Math.max(0, Number(delayMs || 0));
  if (normalizedDelay > 0) {
    setTimeout(deliver, normalizedDelay);
  } else {
    deliver();
  }

  return matchingSessions.length;
}

export function disconnectMockSockets({ targetSid, targetUserId } = {}) {
  let disconnected = 0;
  for (const sessionInfo of listSocketSessions()) {
    if (!matchSession(sessionInfo, { targetSid, targetUserId })) continue;
    const session = getSocketSession(sessionInfo.sid);
    if (!session) continue;
    try {
      session.webSocket?.end?.();
      session.webSocket?.destroy?.();
    } catch {
      // noop
    }
    dropSocketSession(session.sid);
    disconnected += 1;
  }
  return disconnected;
}
