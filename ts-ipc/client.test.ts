import assert from "node:assert/strict";
import { mkdtemp, rm } from "node:fs/promises";
import * as net from "node:net";
import * as os from "node:os";
import * as path from "node:path";
import { describe, test } from "node:test";
import { IpcClient } from "./client.js";

async function withServer(
  handler: (socket: net.Socket, request: Record<string, unknown>) => void,
  run: (socketPath: string, server: net.Server) => Promise<void>,
): Promise<void> {
  const directory = await mkdtemp(path.join(os.tmpdir(), "baoclaw-ipc-"));
  const socketPath = path.join(directory, "ipc.sock");
  const server = net.createServer((socket) => {
    let buffer = "";
    socket.on("data", (data) => {
      buffer += data.toString();
      let newline: number;
      while ((newline = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, newline);
        buffer = buffer.slice(newline + 1);
        if (line) handler(socket, JSON.parse(line));
      }
    });
  });
  await new Promise<void>((resolve) => server.listen(socketPath, resolve));
  try {
    await run(socketPath, server);
  } finally {
    await new Promise<void>((resolve) => server.close(() => resolve()));
    await rm(directory, { recursive: true, force: true });
  }
}

describe("IpcClient", () => {
  test("correlates NDJSON responses and dispatches notifications", async () => {
    await withServer(
      (socket, request) => {
        socket.write(
          JSON.stringify({
            jsonrpc: "2.0",
            method: "event",
            params: { ok: true },
          }) + "\n",
        );
        const response =
          JSON.stringify({
            jsonrpc: "2.0",
            id: request.id,
            result: request.params,
          }) + "\n";
        socket.write(response.slice(0, 10));
        setImmediate(() => socket.write(response.slice(10)));
      },
      async (socketPath) => {
        const client = new IpcClient({ requestTimeoutMs: 500 });
        const notification = new Promise<unknown>((resolve) =>
          client.onNotification("event", resolve),
        );
        await client.connect(socketPath);
        assert.deepStrictEqual(await client.request("echo", { value: 42 }), {
          value: 42,
        });
        assert.deepStrictEqual(await notification, { ok: true });
        await client.disconnect();
      },
    );
  });

  test("times out requests and supports disabled timeout", async () => {
    await withServer(
      () => {},
      async (socketPath) => {
        const client = new IpcClient({ requestTimeoutMs: 20 });
        await client.connect(socketPath);
        await assert.rejects(client.request("slow"), /timed out after 20ms/);
        const pending = client.request("still-slow", undefined, 0);
        await new Promise((resolve) => setTimeout(resolve, 35));
        assert.equal(client.connected, true);
        const disconnected = assert.rejects(pending, /Client disconnected/);
        await client.disconnect();
        await disconnected;
      },
    );
  });

  test("reports remote close and clears connected state", async () => {
    await withServer(
      (socket) => {
        socket.destroy();
      },
      async (socketPath) => {
        const client = new IpcClient();
        const disconnected = new Promise<void>((resolve) =>
          client.onDisconnect(() => resolve()),
        );
        await client.connect(socketPath);
        client.notify("probe");
        await disconnected;
        assert.equal(client.connected, false);
      },
    );
  });

  test("only invokes disconnect handlers once", async () => {
    await withServer(
      () => {},
      async (socketPath) => {
        const client = new IpcClient();
        let disconnects = 0;
        client.onDisconnect(() => {
          disconnects++;
        });
        await client.connect(socketPath);
        await client.disconnect();
        await new Promise((resolve) => setImmediate(resolve));
        assert.equal(disconnects, 1);
      },
    );
  });
});
