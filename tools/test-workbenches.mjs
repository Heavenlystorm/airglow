import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

async function load(name) {
  const path = new URL(`../web/js/${name}.js`, import.meta.url);
  assert.ok(existsSync(path), `${name} module exists`);
  const context = vm.createContext({ console, Date, Number, URL, Blob, setTimeout, clearTimeout });
  const core = new vm.SyntheticModule(["$", "S", "esc", "get", "post", "setHTML", "toast", "fmtTemp"], function () {
    this.setExport("$", () => null);
    this.setExport("S", {});
    this.setExport("esc", (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({"&":"&amp;","<":"&lt;",">":"&gt;",'"':"&quot;","'":"&#39;"}[c])));
    this.setExport("get", async () => ({}));
    this.setExport("post", async () => ({}));
    this.setExport("setHTML", (el, html) => { el.innerHTML = html; });
    this.setExport("toast", () => {});
    this.setExport("fmtTemp", (v) => `${v}°C`);
  }, { context });
  const sheet = new vm.SyntheticModule(["openSheet", "closeSheet"], function () {
    this.setExport("openSheet", () => null); this.setExport("closeSheet", () => {});
  }, { context });
  const module = new vm.SourceTextModule(readFileSync(path, "utf8"), { context });
  await module.link((specifier) => specifier === "./core.js" ? core : sheet);
  await module.evaluate();
  return module.namespace;
}

test("completed-work ages distinguish never completed, recent and old work", async () => {
  const ui = await load("control-center");
  assert.equal(ui.formatAge(null), "No completed work yet");
  assert.equal(ui.formatAge(0.2), "Just now");
  assert.equal(ui.formatAge(70), "1m 10s ago");
  const html = ui.statusRow({ label: "<device>", state: "pending", owner: "Airglow", age_seconds: null, error: null });
  assert.match(html, /Pending/);
  assert.match(html, /No completed work yet/);
  assert.ok(!html.includes("<device>"));
});

test("cooling chart breaks lines at missing readings and places samples by elapsed time", async () => {
  const ui = await load("cooling-workbench");
  const lines = ui.seriesSegments([{elapsed: 0, cpu: 40}, {elapsed: 2, cpu: null}, {elapsed: 10, cpu: 60}], "cpu", 10, 100);
  assert.equal(lines.length, 2, "a missing reading must be a gap, never zero or interpolation");
  assert.match(lines[0], /^0,/);
  assert.match(lines[1], /^600,/);
  assert.match(ui.chartMarkup({samples: []}), /No samples/);
});

test("draft parser accepts paired points and rejects invalid data before replay", async () => {
  const ui = await load("cooling-workbench");
  assert.deepEqual(JSON.parse(JSON.stringify(ui.parseDraftPoints("30: 30\n60: 55\n80: 100"))), [[30,30],[60,55],[80,100]]);
  assert.throws(() => ui.parseDraftPoints("30: nope\n80: 100"), /temperature: duty/);
  assert.throws(() => ui.parseDraftPoints("30: 30"), /at least two/);
  assert.throws(() => ui.parseDraftPoints("20: 30\n30: 35\n40: 40\n50: 45\n60: 50\n70: 60\n80: 75\n90: 90\n100: 100"), /eight/);
});

async function coolingHarness() {
  const nodes = new Map(), requests = [], timers = new Map();
  let timer = 0;
  function node(selector) {
    if (!nodes.has(selector)) {
      const events = new Map();
      nodes.set(selector, { selector, value: "", textContent: "", disabled: false, hidden: false,
        classList: { add() {}, remove() {} }, dataset: {}, events,
        addEventListener(type, fn) { events.set(type, fn); },
        matches(query) { return selector === query; },
        closest(query) { return query === "button" ? this : null; },
        querySelectorAll() { return [node('[data-cooling-export="json"]'), node('[data-cooling-export="csv"]'), node("[data-cooling-replay]")]; },
        async fire(type, event) { await events.get(type)?.(event); },
      });
    }
    return nodes.get(selector);
  }
  const sheet = { open: true, addEventListener() {} }, root = node(".cooling-workbench");
  function request(method, path, body) {
    return new Promise((resolve, reject) => requests.push({ method, path, body, resolve, reject }));
  }
  const context = vm.createContext({ console, Date, Number, URL, Blob,
    setTimeout(fn) { timers.set(++timer, fn); return timer; }, clearTimeout(id) { timers.delete(id); },
  });
  const core = new vm.SyntheticModule(["$", "S", "esc", "get", "post", "setHTML"], function () {
    this.setExport("$", (selector) => node(selector));
    this.setExport("S", {}); this.setExport("esc", (s) => String(s ?? ""));
    this.setExport("get", (path) => request("GET", path));
    this.setExport("post", (path, body) => request("POST", path, body));
    this.setExport("setHTML", (el, html) => { el.innerHTML = html; });
  }, { context });
  const sheets = new vm.SyntheticModule(["openSheet"], function () { this.setExport("openSheet", () => sheet); }, { context });
  const module = new vm.SourceTextModule(readFileSync(new URL("../web/js/cooling-workbench.js", import.meta.url), "utf8"), { context });
  await module.link((specifier) => specifier === "./core.js" ? core : sheets);
  await module.evaluate();
  const idle = { active: null, sessions: [], save_error: null, load_error: null };
  const opened = module.namespace.openCoolingWorkbench();
  requests.shift().resolve(idle);
  await opened;
  return { node, requests, root, idle, poll() { const [id, fn] = [...timers][0]; timers.delete(id); return fn(); } };
}

const flush = () => new Promise(setImmediate);
test("recording start waits for an older status read and blocks duplicate starts", async () => {
  const h = await coolingHarness();
  h.poll();
  assert.equal(h.requests.length, 1);
  h.node("#cooling-annotation").value = "Benchmark";
  const starting = h.root.fire("click", { target: h.node("[data-cooling-start]") });
  await flush();
  assert.equal(h.requests.filter((r) => r.method === "POST").length, 0, "an older read must settle before changing recorder state");
  h.requests.shift().resolve(h.idle);
  await flush();
  assert.equal(h.requests[0].path, "/api/cooling/start");
  await h.root.fire("click", { target: h.node("[data-cooling-start]") });
  assert.equal(h.requests.length, 1, "a second click must not create a second recording");
  h.requests.shift().resolve({});
  await flush();
  const active = { id: "one", annotation: "Benchmark", started_at: 1000, sample_count: 0 };
  h.requests.shift().resolve({ ...h.idle, active });
  await flush();
  h.requests.shift().resolve({ ...active, samples: [] });
  await starting;
  assert.equal(h.node("[data-cooling-start]").disabled, true);
  assert.equal(h.node("[data-cooling-stop]").disabled, false);
});
