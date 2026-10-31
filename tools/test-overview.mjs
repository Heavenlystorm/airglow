// Exercise the actual Overview module with deferred HTTP replies and a deterministic debounce clock.
// Run: node --experimental-vm-modules --test tools/test-overview.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

const source = readFileSync(new URL("../web/js/overview.js", import.meta.url), "utf8");
const plain = (value) => JSON.parse(JSON.stringify(value));

async function harness(gridStyle = { gridTemplateColumns: "297px 297px 297px 297px", paddingLeft: "28px", paddingRight: "28px" }, gridWidth = 1280) {
  const timers = new Map(), requests = [], errors = [];
  let nextTimer = 0;
  const context = vm.createContext({
    window: { addEventListener() {} }, document: {},
    getComputedStyle: () => gridStyle,
    setTimeout(fn) { timers.set(++nextTimer, fn); return nextTimer; },
    clearTimeout(id) { timers.delete(id); },
  });
  const core = {
    $: () => ({ clientWidth: gridWidth }),
    clamp: (v, lo, hi) => Math.max(lo, Math.min(hi, v)),
    S: {}, emit() {}, fail(error) { errors.push(error); },
    post(path, body) {
      return new Promise((resolve, reject) => requests.push({ path, body: plain(body), resolve, reject }));
    },
  };
  const imports = new Map([...source.matchAll(/import\s+\{([^}]+)\}\s+from\s+["']([^"']+)["']/g)]
    .map((m) => [m[2], m[1].split(",").map((name) => name.trim())]));
  const module = new vm.SourceTextModule(source + "\nexport { save, O, cell, onKey, onPointerMove };", { context });
  await module.link((specifier) => {
    const names = imports.get(specifier);
    return new vm.SyntheticModule(names, function () {
      for (const name of names) this.setExport(name, specifier === "./core.js" && name in core ? core[name] : () => {});
    }, { context });
  });
  await module.evaluate();
  const { save, O, cell, onKey, onPointerMove } = module.namespace;
  O.layout = { tiles: [{ id: "cpu", label: "A" }], pins: ["cpu"] };
  function fire() {
    assert.equal(timers.size, 1, "only one debounce should be pending");
    const [id, callback] = timers.entries().next().value;
    timers.delete(id);
    return callback();
  }
  return { save, O, requests, errors, timers, fire, cell, onKey, onPointerMove };
}

test("resize geometry excludes dashboard gutters and follows visible columns", async () => {
  const desktop = await harness();
  assert.deepEqual(plain(desktop.cell()), { cw: 297, rh: 152, cols: 4 });
  const narrow = await harness({ gridTemplateColumns: "318px 318px", paddingLeft: "16px", paddingRight: "16px" }, 680);
  assert.deepEqual(plain(narrow.cell()), { cw: 318, rh: 152, cols: 2 });
  const phone = await harness({ gridTemplateColumns: "358px", paddingLeft: "16px", paddingRight: "16px" }, 390);
  assert.deepEqual(plain(phone.cell()), { cw: 358, rh: 152, cols: 1 });
});

test("a grow key at the narrow viewport limit preserves the saved desktop span", async () => {
  const h = await harness({ gridTemplateColumns: "318px 318px", paddingLeft: "16px", paddingRight: "16px" }, 680);
  const tile = h.O.layout.tiles[0];
  tile.w = 4; tile.h = 1; h.O.editing = true;
  const el = { dataset: { id: tile.id }, closest() { return this; }, focus() {} };
  h.onKey({ target: el, shiftKey: true, key: "ArrowRight", preventDefault() {} });
  assert.equal(tile.w, 4);
  assert.equal(h.timers.size, 0, "a visually unchanged tile must not overwrite its saved span");
  h.onKey({ target: el, shiftKey: true, key: "ArrowLeft", preventDefault() {} });
  assert.equal(tile.w, 1, "explicit shrinking starts from the visible two-column width");
});

test("grabbing a clamped tile resize handle without moving does not shrink it", async () => {
  const h = await harness({ gridTemplateColumns: "318px 318px", paddingLeft: "16px", paddingRight: "16px" }, 680);
  const tile = h.O.layout.tiles[0];
  tile.w = 4; tile.h = 1;
  h.O.drag = { mode: "resize", t: tile, w0: 2, savedW0: 4, h0: 1, x0: 0, y0: 0 };
  h.onPointerMove({ clientX: 0, clientY: 0 });
  assert.equal(tile.w, 4);
  assert.ok(!h.O.drag.changed);
});

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
