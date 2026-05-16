import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";

import {
  clearSocketEventLog,
  disconnectMockSockets,
  emitMockSocketEvent,
  listSocketSessions,
  resetMockBehavior,
  startMockServer,
  stopMockServer,
} from "./index.mjs";
import { handleWebSocketUpgrade } from "./socket.mjs";
import { getSocketSession, registerSocketSession } from "./state.mjs";
import { createSocket, onceSocket } from "./test-helpers/socket-client.mjs";

class FakeWebSocket extends EventEmitter {
  constructor() {
    super();
    this.destroyed = false;
    this.writes = [];
  }

  write(chunk) {
    this.writes.push(String(chunk));
  }

  end() {
    this.destroyed = true;
  }

  destroy() {
    this.destroyed = true;
  }
}

test.beforeEach(async () => {
  await stopMockServer();
  resetMockBehavior();
  clearSocketEventLog();
});

test.afterEach(async () => {
  disconnectMockSockets();
  await stopMockServer();
});

test("accepts a real socket.io client and delivers server-pushed events", async () => {
  const started = await startMockServer(18573, { retryIfInUse: true });
  const baseUrl = `http://127.0.0.1:${started.port}`;

  const socket = createSocket(baseUrl, {
    auth: { token: "mock-jwt-token" },
    transports: ["polling", "websocket"],
  });

  try {
    const readyPayload = await onceSocket(socket, "ready");
    assert.equal(typeof readyPayload.sid, "string");
    assert.equal(readyPayload.userId, "mock-user");
    assert.equal(listSocketSessions().length, 1);

    const donePromise = onceSocket(socket, "chat_done");
    const delivered = emitMockSocketEvent({
      event: "chat_done",
      data: {
        thread_id: "thread-1",
        request_id: "request-1",
        full_response: "mock transport works",
        rounds_used: 1,
        total_input_tokens: 12,
        total_output_tokens: 4,
      },
    });
    assert.equal(delivered, 1);

    const donePayload = await donePromise;
    assert.equal(donePayload.full_response, "mock transport works");
    assert.equal(donePayload.thread_id, "thread-1");
  } finally {
    socket.disconnect();
  }
});

test("supports polling-only clients", async () => {
  const started = await startMockServer(18574, { retryIfInUse: true });
  const baseUrl = `http://127.0.0.1:${started.port}`;

  const pollingSocket = createSocket(baseUrl, {
    auth: { token: "polling-only" },
    transports: ["polling"],
    upgrade: false,
  });

  try {
    const readyPayload = await onceSocket(pollingSocket, "ready");
    assert.equal(readyPayload.userId, "mock-user");
  } finally {
    pollingSocket.disconnect();
  }
});

test("keeps polling session alive when websocket probe closes before upgrade", () => {
  const session = registerSocketSession({
    sid: "probe-fallback-sid",
    socketId: "probe-fallback-sid",
    transport: "polling",
    createdAt: new Date().toISOString(),
  });
  const socket = new FakeWebSocket();

  handleWebSocketUpgrade(
    {
      url: `/socket.io/?transport=websocket&sid=${session.sid}`,
      headers: { "sec-websocket-key": "dGhlIHNhbXBsZSBub25jZQ==" },
    },
    socket,
  );

  socket.emit("close");

  const live = getSocketSession(session.sid);
  assert.ok(live);
  assert.equal(live.transport, "polling");
  assert.equal(live.upgradedToWebSocket, false);
  assert.equal(live.webSocket, null);
});
