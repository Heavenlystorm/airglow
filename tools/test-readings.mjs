// Real telemetry module, with controlled time and delayed history replies.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const plain = value => JSON.parse(JSON.stringify(value));
async function harness() {
  const requests = [], S = { state: { readings: {} } };
  let now = 1000, fahrenheit = false;
  class Clock extends Date { static now() { return now * 1000; } }
  const context = vm.createContext({ Date: Clock });
  const core = new vm.SyntheticModule(["S", "get", "emit", "unitF"], function () {
    this.setExport("S", S);
    this.setExport("get", path => new Promise((resolve, reject) => requests.push({ path, resolve, reject })));
    this.setExport("emit", () => {});
    this.setExport("unitF", () => fahrenheit);
  }, { context });
  const module = new vm.SourceTextModule(readFileSync(new URL("../web/js/readings.js", import.meta.url), "utf8"), { context });
  await module.link(() => core); await module.evaluate();
  return { ...module.namespace, S, requests, clock: t => { now = t; }, fahrenheit: v => { fahrenheit = v; } };
}

test("history retains missing samples and live points received during the request", async () => {
  const h = await harness(), pending = h.ensureHistory(["cpu"]);
  h.pushReadings({ cpu: 55 });
  h.requests[0].resolve({ t: [980, 982, 984], series: { cpu: [40, null, 45] } });
  await pending;
  assert.deepEqual(plain(h.R.series.cpu), [[980, 40], [982, null], [984, 45], [1000, 55]]);
});

test("history loads even after a live sample and concurrent requests coalesce", async () => {
  const h = await harness(); h.pushReadings({ cpu: 55 });
  const one = h.ensureHistory(["cpu"]), two = h.ensureHistory(["cpu"]);
  assert.equal(h.requests.length, 1);
  h.requests[0].resolve({ t: [998], series: { cpu: [50] } });
  await Promise.all([one, two]);
  assert.deepEqual(plain(h.R.series.cpu), [[998, 50], [1000, 55]]);
});

test("samples expire and absent tracked sensors create gaps", async () => {
  const h = await harness(); h.pushReadings({ cpu: 45 });
  h.clock(1002); h.pushReadings({});
  assert.deepEqual(plain(h.R.series.cpu), [[1000, 45], [1002, null]]);
  h.clock(1603); h.pushReadings({});
  assert.ok((h.R.series.cpu || []).every(([t]) => t >= 1003));
});

test("large layouts load every sensor despite the API's forty-id request limit", async () => {
  const h = await harness(), ids = Array.from({ length: 64 }, (_, i) => `sensor-${i}`);
  const pending = h.ensureHistory(ids);
  assert.equal(h.requests.length, 2);
  for (const request of h.requests) {
    const batch = decodeURIComponent(request.path.split("ids=")[1]).split(",");
    assert.ok(batch.length <= 40);
    request.resolve({ t: [998], series: Object.fromEntries(batch.map(id => [id, [42]])) });
  }
  await pending;
  assert.ok(ids.every(id => h.R.series[id]?.[0]?.[1] === 42));
});

test("resampling does not invent cold-start history or bridge outages", async () => {
  const h = await harness();
  const gap = h.resample([[400, 40], [410, null], [420, 50]], 61, 1000);
  assert.deepEqual(plain(gap.slice(0, 3)), [40, null, 50]);
  const cold = h.resample([[999, 42]], 61, 1000);
  assert.equal(cold[59], null); assert.equal(cold[60], 42);
  const stalled = h.resample([[400, 40], [1000, 80]], 61, 1000);
  assert.equal(stalled[30], null);
  const brief = h.resample([[400, 40], [402, null], [404, 45], [410, 50]], 61, 1000);
  assert.ok(brief[0] == null || brief[1] == null, "A brief outage between graph points must still break the line");
});

test("missing, nonfinite and stale values never become plausible readings", async () => {
  const h = await harness(), meta = { unit: "°C", value: 45 };
  h.R.meta.cpu = meta;
  for (const value of [null, undefined, NaN, Infinity, "42"]) {
    assert.equal(h.fmt(meta, value).num, "–");
  }
  h.S.state.readings.cpu = null; assert.equal(h.value("cpu"), null);
  h.fahrenheit(true);
  assert.equal(h.toDisplay(meta, null), null);
  assert.equal(h.fmt(meta, 0).num, "32");
  h.S.state.readings.cpu = 45; h.markReadingsStale();
  assert.equal(h.value("cpu"), null);
  h.pushReadings({ cpu: 46 }); h.S.state.readings.cpu = 46;
  assert.equal(h.value("cpu"), 46);
});

test("binary byte units and temperature scales match the values plotted", async () => {
  const h = await harness();
  assert.deepEqual(plain(h.fmt({ unit: "B/s" }, 1024)), { num: "1.0", unit: "KiB/s" });
  assert.deepEqual(plain(h.fmt({ unit: "MB" }, 2048)), { num: "2.0", unit: "GiB" });
  const scale = h.range("cold", { unit: "°C", value: -5 }, [[1000, -5], [1002, null]]);
  assert.ok(scale[0] <= -5);
  assert.ok(h.range("x", { unit: "W", value: NaN }, [[1000, Infinity]]).every(Number.isFinite));
});
