// Exercise the actual sandbox frame and runner with worker/DOM boundaries and a deterministic clock.
// Run: node --experimental-vm-modules --test tools/test-effect-runtime.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const frameSource = readFileSync(new URL("../web/runtime/frame.js", import.meta.url), "utf8");
const runnerSource = readFileSync(new URL("../web/js/fxrun.js", import.meta.url), "utf8");
const settle = async () => { for (let i = 0; i < 15; i++) await Promise.resolve(); };

function clock() {
  let now = 1, next = 0;
  const timers = new Map();
  return {
    timers,
    performance: { now: () => now },
    setTimeout(fn, delay) { timers.set(++next, { fn, at: now + delay }); return next; },
    clearTimeout(id) { timers.delete(id); },
    advance(ms) {
      const end = now + ms;
      while (true) {
        const due = [...timers].filter(([, t]) => t.at <= end).sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        const [id, timer] = due;
        now = timer.at;
        timers.delete(id);
        timer.fn();
      }
      now = end;
    },
  };
}

function frameHarness() {
  const time = clock(), messages = [], workers = [];
  let listener;
  const parent = { postMessage: (message) => messages.push(message) };
  class Worker {
    constructor() { this.sent = []; this.terminated = false; workers.push(this); }
    postMessage(message) { this.sent.push(message); }
    terminate() { this.terminated = true; }
    emit(data) { this.onmessage({ data }); }
  }
  vm.runInNewContext(frameSource, {
    ...time, parent, Worker, Blob,
    URL: { createObjectURL: () => "blob:test", revokeObjectURL() {} },
    window: { addEventListener(type, fn) { listener = fn; } },
  });
  const send = (data) => listener({ source: parent, data });
  send({ type: "load", kind: "code", runtime: "", layersLib: "", code: "", points: [] });
  return { ...time, messages, worker: workers[0], send };
}

test("a worker that never becomes ready is terminated without any ticks", () => {
  const h = frameHarness();
  assert.equal(h.worker.sent[0].type, "start");
  h.advance(8000);
  assert.equal(h.worker.terminated, true);
  assert.deepEqual(h.messages.map((m) => m.type), ["hello", "stopped"]);
  assert.equal(h.messages[1].reason, "hung");
  assert.equal(h.timers.size, 0);
  h.worker.emit({ type: "ready" });
  assert.equal(h.messages.length, 2, "late worker messages cannot revive a stopped effect");
});

test("healthy initialization cancels the watchdog and rendering remains available", () => {
  const h = frameHarness();
  h.advance(2000);
  h.worker.emit({ type: "ready" });
  assert.equal(h.timers.size, 0);
  h.advance(10000);
  assert.equal(h.worker.terminated, false);
  h.send({ type: "tick", time: 0, dt: 1 / 30 });
  const colors = new Uint8Array([1, 2, 3]);
  h.worker.emit({ type: "frame", colors, ms: 1 });
  assert.equal(h.messages.at(-1).colors, colors);
  h.send({ type: "stop" });
  assert.equal(h.worker.terminated, true);
  assert.equal(h.messages.at(-1).reason, "stopped");
});

test("stopping or failing during initialization cancels the watchdog", () => {
  for (const end of [
    (h) => h.send({ type: "stop" }),
    (h) => h.worker.emit({ type: "error", message: "bad init" }),
    (h) => h.worker.onerror({ preventDefault() {}, message: "bad script" }),
  ]) {
    const h = frameHarness();
    end(h);
    assert.equal(h.worker.terminated, true);
    assert.equal(h.timers.size, 0);
    const count = h.messages.length;
    h.advance(10000);
    assert.equal(h.messages.length, count);
  }
});

async function runnerHarness({ assetsGate = Promise.resolve() } = {}) {
  const time = clock(), listeners = new Set(), messages = [], events = [], appended = [];
  const frame = {
    style: {}, setAttribute() {}, removed: false,
    contentWindow: { postMessage: (message) => messages.push(message) },
    remove() { this.removed = true; },
  };
  const context = vm.createContext({
    ...time, Uint8Array, ImageBitmap: class {},
    window: {
      addEventListener(type, fn) { listeners.add(fn); },
      removeEventListener(type, fn) { listeners.delete(fn); },
    },
    document: { createElement: () => frame, body: { appendChild: (f) => appended.push(f) } },
    fetch: async () => {
      await assetsGate;
      return { text: async () => "", json: async () => ({ types: {} }) };
    },
  });
  const module = new vm.SourceTextModule(runnerSource, { context });
  await module.link(() => { throw new Error("unexpected import"); });
  await module.evaluate();
  const runner = new module.namespace.EffectRunner({
    onReady: () => events.push("ready"), onStopped: (reason) => events.push(reason),
    onError: (error) => events.push(error.message),
  });
  const send = (data) => { for (const fn of listeners) fn({ source: frame.contentWindow, data }); };
  const loading = runner.load({ kind: "code", code: "" }, []);
  await settle();
  return { ...time, runner, frame, loading, send, messages, events, listeners, appended };
}

test("runner clears the hello timeout after startup and removes stopped frames", async () => {
  const h = await runnerHarness();
  h.send({ type: "hello" });
  await h.loading;
  assert.equal(h.timers.size, 0);
  assert.equal(h.messages.at(-1).type, "load");
  h.send({ type: "ready" });
  assert.equal(h.runner.tick(0, 1 / 30), true);
  h.send({ type: "stopped", reason: "hung" });
  assert.equal(h.runner.tick(1, 1 / 30), false);
  assert.equal(h.frame.removed, true);
  assert.equal(h.listeners.size, 0);
  assert.deepEqual(h.events, ["ready", "hung"]);
});

test("destroy while waiting for hello settles load without sending effect code", async () => {
  const h = await runnerHarness();
  h.runner.destroy();
  assert.equal(h.timers.size, 0);
  await h.loading;
  h.advance(10000);
  assert.deepEqual(h.messages.map((m) => m.type), ["stop"]);
  assert.deepEqual(h.events, []);
  assert.equal(h.listeners.size, 0);
});

test("destroy while loading assets never appends an orphaned iframe", async () => {
  let release;
  const h = await runnerHarness({ assetsGate: new Promise((resolve) => { release = resolve; }) });
  h.runner.destroy();
  release();
  await settle();
  assert.equal(h.appended.length, 0);
  assert.equal(h.timers.size, 0);
  await h.loading;
});

test("a page that never says hello reports its startup error and cleans up", async () => {
  const h = await runnerHarness();
  h.advance(8000);
  await h.loading;
  assert.equal(h.frame.removed, true);
  assert.equal(h.listeners.size, 0);
  assert.equal(h.timers.size, 0);
  assert.equal(h.events.length, 2);
  assert.match(h.events[0], /didn't start/);
  assert.equal(h.events[1], "error");
});
