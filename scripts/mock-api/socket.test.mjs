import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";

import {
  clearSocketEventLog,
  disconnectMockSockets,
  emitMockSocketEvent,
  listSocketSessions,
  resetMockBehavior,
  setMockBehaviors,
  startMockServer,
  stopMockServer,
} from "./index.mjs";

const requireFromApp = createRequire(
  new URL("../../app/package.json", import.meta.url),
);
const { io: SocketClient } = requireFromApp("socket.io-client");

function onceSocket(socket, event) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      cleanup();
      reject(new Error(`Timed out waiting for socket event: ${event}`));
    }, 5_000);

    const onEvent = (...args) => {
      cleanup();
      resolve(args[0]);
    };

    const onError = (err) => {
      cleanup();
      reject(err instanceof Error ? err : new Error(String(err)));
    };

    const cleanup = () => {
      clearTimeout(timeout);
      socket.off(event, onEvent);
      if (event !== "connect_error") {
        socket.off("connect_error", onError);
      }
    };

    socket.on(event, onEvent);
    if (event !== "connect_error") {
      socket.on("connect_error", onError);
    }
  });
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

  const socket = SocketClient(baseUrl, {
    auth: { token: "mock-jwt-token" },
    path: "/socket.io/",
    transports: ["polling", "websocket"],
    reconnection: false,
    forceNew: true,
    timeout: 3_000,
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

test("supports polling-only clients and connect_error for missing tokens", async () => {
  const started = await startMockServer(18574, { retryIfInUse: true });
  const baseUrl = `http://127.0.0.1:${started.port}`;

  const pollingSocket = SocketClient(baseUrl, {
    auth: { token: "polling-only" },
    path: "/socket.io/",
    transports: ["polling"],
    upgrade: false,
    reconnection: false,
    forceNew: true,
    timeout: 3_000,
  });

  try {
    const readyPayload = await onceSocket(pollingSocket, "ready");
    assert.equal(readyPayload.userId, "mock-user");
  } finally {
    pollingSocket.disconnect();
  }

  setMockBehaviors({ socketAuthMode: "required" }, "replace");
  const rejectedSocket = SocketClient(baseUrl, {
    auth: {},
    path: "/socket.io/",
    transports: ["polling"],
    upgrade: false,
    reconnection: false,
    forceNew: true,
    timeout: 3_000,
  });

  try {
    const error = await onceSocket(rejectedSocket, "connect_error");
    assert.match(String(error?.message || error), /No token provided/);
  } finally {
    rejectedSocket.disconnect();
  }
});
