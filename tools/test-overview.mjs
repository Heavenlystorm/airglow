// Exercise the actual Overview module with deferred HTTP replies and a deterministic debounce clock.
// Run: node --experimental-vm-modules --test tools/test-overview.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("../web/js/overview.js", import.meta.url), "utf8");
const plain = (value) => JSON.parse(JSON.stringify(value));

async function harness() {
  const timers = new Map(), requests = [], errors = [];
  let nextTimer = 0;
  const context = vm.createContext({
    window: { addEventListener() {} }, document: {},
    setTimeout(fn) { timers.set(++nextTimer, fn); return nextTimer; },
    clearTimeout(id) { timers.delete(id); },
  });
  const core = {
    S: {}, emit() {}, fail(error) { errors.push(error); },
    post(path, body) {
      return new Promise((resolve, reject) => requests.push({ path, body: plain(body), resolve, reject }));
    },
  };
  const imports = new Map([...source.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g)]
    .map((m) => [m[2], m[1].split(",").map((name) => name.trim())]));
  const module = new vm.SourceTextModule(source + "\nexport { save, O };", { context });
  await module.link((specifier) => {
    const names = imports.get(specifier);
    return new vm.SyntheticModule(names, function () {
      for (const name of names) this.setExport(name, specifier === "./core.js" && name in core ? core[name] : () => {});
    }, { context });
  });
  await module.evaluate();
  const { save, O } = module.namespace;
  O.layout = { tiles: [{ id: "cpu", label: "A" }], pins: ["cpu"] };
  function fire() {
    assert.equal(timers.size, 1, "only one debounce should be pending");
    const [id, callback] = timers.entries().next().value;
    timers.delete(id);
    return callback();
  }
  return { save, O, requests, errors, timers, fire };
}

test("edits during a save survive its response and persist in the next serialized request", async () => {
  const h = await harness(), tile = h.O.layout.tiles[0];
  h.save();
  const first = h.fire();
  tile.label = "AB";
  h.O.layout.pins = ["gpu"];
  h.save();
  h.fire();                         // debounce expires while the first request is pending
  assert.equal(h.requests.length, 1, "writes must not overlap");
  h.requests[0].resolve(h.requests[0].body);
  await first;
  assert.equal(tile.label, "AB");
  assert.deepEqual(h.O.layout.pins, ["gpu"]);
  const second = h.fire();
  assert.equal(h.requests[1].body.tiles[0].label, "AB");
  h.requests[1].resolve(h.requests[1].body);
  await second;
  assert.equal(h.O.layout.tiles[0], tile, "an open tile sheet must keep its live reference");
  assert.equal(h.timers.size, 0);
});

test("a stale response cannot resurrect a removed tile or discard a newly added tile", async () => {
  const h = await harness();
  h.save();
  const first = h.fire();
  h.O.layout.tiles = [{ id: "gpu", label: "Graphics" }];
  h.save();
  h.requests[0].resolve(h.requests[0].body);
  await first;
  const second = h.fire();
  assert.deepEqual(h.requests[1].body.tiles, [{ id: "gpu", label: "Graphics" }]);
  h.requests[1].resolve(h.requests[1].body);
  await second;
  assert.deepEqual(plain(h.O.layout.tiles), [{ id: "gpu", label: "Graphics" }]);
});

test("the latest edit is still sent when an earlier request fails", async () => {
  const h = await harness();
  h.save();
  const first = h.fire();
  h.O.layout.tiles[0].label = "Latest";
  h.save();
  h.fire();
  h.requests[0].reject(new Error("temporary failure"));
  await first;
  assert.equal(h.errors.length, 1);
  const second = h.fire();
  assert.equal(h.requests[1].body.tiles[0].label, "Latest");
  h.requests[1].resolve(h.requests[1].body);
  await second;
});

test("rapid edits coalesce and a current response can normalize the layout", async () => {
  const h = await harness(), tile = h.O.layout.tiles[0];
  for (const label of ["B", "C", "D"]) { tile.label = label; h.save(); }
  const saved = h.fire();
  assert.equal(h.requests.length, 1);
  assert.equal(h.requests[0].body.tiles[0].label, "D");
  h.requests[0].resolve({ tiles: [{ id: "cpu", label: "D", w: 1 }], pins: ["cpu"] });
  await saved;
  assert.equal(tile.w, 1);
  assert.equal(h.O.layout.tiles[0], tile);
});
