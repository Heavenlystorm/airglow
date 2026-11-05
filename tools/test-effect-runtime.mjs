// Exercise the actual sandbox frame and runner with worker/DOM boundaries and a deterministic clock.
// Run: node --experimental-vm-modules --test tools/test-effect-runtime.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const frameSource = readFileSync(new URL("../web/runtime/frame.js", import.meta.url), "utf8");
const runnerSource = readFileSync(new URL("../web/js/fxrun.js", import.meta.url), "utf8");
const liveSource = readFileSync(new URL("../web/js/live.js", import.meta.url), "utf8");
const hostSource = readFileSync(new URL("../web/js/fxhost.js", import.meta.url), "utf8");
const previewSource = readFileSync(new URL("../web/js/fxpreview.js", import.meta.url), "utf8");
const settle = async () => { for (let i = 0; i < 15; i++) await Promise.resolve(); };

test("a permanently CLOSED stream retries without a state event or window switch", async () => {
  const time = clock(), streams = [], visibility = new Map();
  const document = { visibilityState: "visible", addEventListener: (t, fn) => visibility.set(t, fn) };
  class EventSource {
    static CLOSED = 2;
    constructor(url) { this.url = url; this.readyState = 1; streams.push(this); }
    addEventListener() {}
    close() { this.readyState = 2; }
  }
  const context = vm.createContext({ ...time, document, EventSource, window: { addEventListener() {} } });
  const core = new vm.SyntheticModule(["emit"], function () { this.setExport("emit", () => {}); }, { context });
  const module = new vm.SourceTextModule(liveSource, { context });
  await module.link(() => core);
  await module.evaluate();
  module.namespace.startLive({ state() {}, lost() {}, frames: () => true });
  streams[0].readyState = 2;
  streams[0].onerror();
  time.advance(1000);
  assert.equal(streams.length, 2);
  assert.equal(streams[1].url, "/api/stream?frames=1");
  streams[1].readyState = 2;
  streams[1].onerror();
  document.visibilityState = "hidden";
  visibility.get("visibilitychange")();
  time.advance(10000);
  assert.equal(streams.length, 2, "a hidden window cancels the pending retry");
});

async function hostHarness() {
  const time = clock(), requests = [], runners = [], events = new Map();
  class Runner {
    constructor(h) { this.h = h; runners.push(this); }
    load() { return Promise.resolve(); }
    tick() { return false; }
    destroy() {}
  }
  const context = vm.createContext({ ...time, Uint8Array, URLSearchParams,
    location: { hash: "#token=fixture" }, console,
    EventSource: class { addEventListener(t, fn) { events.set(t, fn); } },
    fetch: () => new Promise((resolve, reject) => requests.push({ resolve, reject })),
  });
  const fx = new vm.SyntheticModule(["EffectRunner"], function () { this.setExport("EffectRunner", Runner); }, { context });
  const module = new vm.SourceTextModule(hostSource, { context });
  await module.link(() => fx);
  await module.evaluate();
  events.get("load")({ data: JSON.stringify({ id: "one", kind: "code", code: "", params: {}, points: [] }) });
  const frame = (colors = [20, 30, 40]) => runners[0].h.onFrame(new Uint8Array(colors));
  return { ...time, requests, frame, events };
}

test("static host frames retry after network and HTTP failures and deduplicate only acknowledged colours", async () => {
  for (const network of [false, true]) {
    const h = await hostHarness();
    h.frame();
    if (network) h.requests[0].reject(new Error("offline"));
    else h.requests[0].resolve({ ok: false, status: 503, arrayBuffer: async () => new ArrayBuffer(0) });
    await settle();
    h.frame();
    assert.equal(h.requests.length, 2, "the unchanged static frame must retry");
    h.requests[1].resolve({ ok: true, arrayBuffer: async () => new ArrayBuffer(0) });
    await settle();
    h.frame();
    assert.equal(h.requests.length, 2, "a successful acknowledgement permits deduplication");
  }
});

test("binary host posts serialize changing colours and ignore acknowledgements predating point updates", async () => {
  const h = await hostHarness();
  h.frame([1, 2, 3]);
  h.frame([4, 5, 6]);
  assert.equal(h.requests.length, 1, "only one binary frame may be in flight");
  h.requests[0].resolve({ ok: true, arrayBuffer: async () => new ArrayBuffer(0) });
  await settle();
  h.frame([4, 5, 6]);
  assert.equal(h.requests.length, 2);
  h.events.get("points")({ data: JSON.stringify({ id: "one", points: [[1, 2]] }) });
  h.requests[1].resolve({ ok: true, arrayBuffer: async () => new ArrayBuffer(0) });
  await settle();
  h.frame([4, 5, 6]);
  assert.equal(h.requests.length, 3, "the point change invalidates the older in-flight acknowledgement");
  h.requests[2].resolve({ ok: true, arrayBuffer: async () => new ArrayBuffer(0) });
  await settle();
});

async function previewHarness() {
  const time = clock(), requests = [], events = [];
  const context = vm.createContext({ ...time, Uint8Array, atob, Blob, structuredClone, createImageBitmap: async () => ({ close() {} }) });
  const core = new vm.SyntheticModule(["post"], function () {
    this.setExport("post", (path, body) => new Promise((resolve, reject) => requests.push({ path, body, resolve, reject })));
  }, { context });
  const module = new vm.SourceTextModule(previewSource, { context });
  await module.link(() => core);
  await module.evaluate();
  const runner = new module.namespace.PreviewRunner({ onReady: () => events.push("ready"),
    onFrame: colors => events.push(Array.from(colors)), onStopped: reason => events.push(reason),
    onError: error => events.push(error.message) });
  const loading = runner.load({ kind: "code", code: "", params: {} }, [[1, 2]]);
  return { ...time, runner, loading, requests, events };
}

test("destroying a preview during load releases the late session without reviving it", async () => {
  const h = await previewHarness();
  h.runner.destroy();
  h.requests[0].resolve({ id: "p1" });
  await h.loading;
  assert.deepEqual(h.events, []);
  assert.equal(h.requests[1].path, "/api/studio/preview/stop");
  assert.equal(h.requests[1].body.id, "p1");
  h.requests[1].resolve({});
});

test("preview ticks coalesce updates and never paint a response after close", async () => {
  const h = await previewHarness();
  h.requests[0].resolve({ id: "p1" });
  await h.loading;
  h.runner.setParams({ speed: 2 });
  h.runner.setParams({ tone: "#112233" });
  assert.equal(h.runner.tick(2, 1 / 30), true);
  assert.equal(h.runner.tick(3, 1 / 30), false);
  assert.deepEqual(JSON.parse(JSON.stringify(h.requests[1].body.updates.params)), { speed: 2, tone: "#112233" });
  h.runner.destroy();
  h.requests[1].resolve({ colors: "/wAA", ms: 1 });
  await settle();
  assert.deepEqual(h.events, ["ready"]);
  h.requests[2].resolve({});
});

test("preview memory failures are surfaced and stop the remote session", async () => {
  const h = await previewHarness();
  h.requests[0].resolve({ id: "p1" });
  await h.loading;
  h.runner.tick(2, 1 / 30);
  h.requests[1].resolve({ problem: { reason: "memory", message: "Memory limit reached" } });
  await settle();
  assert.deepEqual(h.events, ["ready", "Memory limit reached", "memory"]);
  assert.equal(h.runner.ready, false);
  h.requests[2].resolve({});
});

test("a preview resumes after idle expiry with its latest parameters and positions", async () => {
  const h = await previewHarness();
  h.requests[0].resolve({ id: "p1" });
  await h.loading;
  h.runner.setParams({ speed: 2 });
  h.runner.setPoints([[20, 30]]);
  h.runner.tick(20, 1 / 30);
  h.requests[1].resolve({ problem: { reason: "stopped" } });
  await settle();
  assert.equal(h.requests[2].path, "/api/studio/preview/load");
  assert.deepEqual(JSON.parse(JSON.stringify(h.requests[2].body.effect.params)), { speed: 2 });
  assert.deepEqual(JSON.parse(JSON.stringify(h.requests[2].body.points)), [[20, 30]]);
  h.requests[2].resolve({ id: "p2" });
  await settle();
  assert.equal(h.requests[3].body.id, "p2");
  h.requests[3].resolve({ colors: "/wAA" });
  await settle();
  assert.deepEqual(h.events.at(-1), [255, 0, 0]);
  assert.equal(h.runner.ready, true);
  h.runner.destroy();
  h.requests[4].resolve({});
});

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
      return { ok: true, text: async () => "", json: async () => ({ types: {} }) };
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

test("runtime assets retry after a failed HTTP response and coalesce concurrent retries", async () => {
  let fail = true, calls = 0;
  const context = vm.createContext({ fetch: async () => {
    calls++;
    return { ok: !fail, status: 503, text: async () => "", json: async () => ({ types: {} }) };
  } });
  const module = new vm.SourceTextModule(runnerSource, { context });
  await module.link(() => { throw new Error("unexpected import"); });
  await module.evaluate();
  await assert.rejects(module.namespace.runtimeAssets(), /503/);
  fail = false;
  await Promise.all([module.namespace.runtimeAssets(), module.namespace.runtimeAssets()]);
  assert.equal(calls, 6);
});

test("runtime fetch failure reports a stopped runner without an unhandled rejection", async () => {
  const h = await runnerHarness({ assetsGate: Promise.reject(new Error("network down")) });
  await h.loading;
  assert.equal(h.frame.removed, true);
  assert.deepEqual(h.events, ["network down", "error"]);
  assert.equal(h.listeners.size, 0);
});

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
