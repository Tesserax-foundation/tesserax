/** JSON-RPC client of a Tesserax node over a websocket, reconnecting when the connection drops. */

import { log } from "./log.js";

type Pending = { resolve: (value: unknown) => void; reject: (error: Error) => void; timer: NodeJS.Timeout };

/** Requests not answered within this time fail. */
const REQUEST_TIMEOUT_MS = 60_000;

export class RpcError extends Error {
  override name = "RpcError";
}

export class Rpc {
  private socket?: WebSocket;
  private connecting?: Promise<void>;
  private nextId = 1;
  private readonly pending = new Map<number, Pending>();
  private readonly subscriptions = new Map<string, (params: unknown) => void>();
  private closed = false;
  /** Called after every new connection, to subscribe again. */
  onConnect?: () => void;

  constructor(readonly url: string) {}

  /** Connect, retrying with a growing delay until the node answers. */
  async connect(): Promise<void> {
    if (this.socket?.readyState === WebSocket.OPEN) return;
    this.connecting ??= this.open().finally(() => (this.connecting = undefined));
    return this.connecting;
  }

  private async open(): Promise<void> {
    for (let attempt = 0; !this.closed; attempt++) {
      try {
        await new Promise<void>((resolve, reject) => {
          const socket = new WebSocket(this.url);
          socket.onopen = () => {
            this.socket = socket;
            socket.onmessage = (event) => this.receive(String(event.data));
            socket.onclose = () => this.dropped();
            resolve();
          };
          socket.onerror = () => reject(new RpcError(`cannot connect to ${this.url}`));
        });
        log.info(`connected to ${this.url}`);
        this.onConnect?.();
        return;
      } catch (error) {
        const delay = Math.min(30_000, 1_000 * 2 ** attempt);
        log.warn(`${(error as Error).message}, retrying in ${delay / 1000} s`);
        await new Promise((r) => setTimeout(r, delay));
      }
    }
  }

  private dropped(): void {
    this.socket = undefined;
    for (const [, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(new RpcError("connection to the node lost"));
    }
    this.pending.clear();
    this.subscriptions.clear();
    if (!this.closed) {
      log.warn("connection to the node lost, reconnecting");
      void this.connect();
    }
  }

  private receive(text: string): void {
    const message = JSON.parse(text);
    if (message.id !== undefined) {
      const pending = this.pending.get(message.id);
      if (!pending) return;
      this.pending.delete(message.id);
      clearTimeout(pending.timer);
      if (message.error) pending.reject(new RpcError(`${message.error.message} (${message.error.code})`));
      else pending.resolve(message.result);
    } else if (message.params?.subscription !== undefined) {
      this.subscriptions.get(String(message.params.subscription))?.(message.params.result);
    }
  }

  async request<T>(method: string, params: unknown[] = []): Promise<T> {
    await this.connect();
    const socket = this.socket;
    if (!socket) throw new RpcError("not connected");
    const id = this.nextId++;
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new RpcError(`${method} timed out`));
      }, REQUEST_TIMEOUT_MS);
      this.pending.set(id, { resolve: resolve as (value: unknown) => void, reject, timer });
      socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
    });
  }

  /** Subscribe with `method`; `onEvent` receives each notification until the connection drops. */
  async subscribe(method: string, params: unknown[], onEvent: (params: unknown) => void): Promise<void> {
    const id = await this.request<string | number>(method, params);
    this.subscriptions.set(String(id), onEvent);
  }

  close(): void {
    this.closed = true;
    this.socket?.close();
  }
}
