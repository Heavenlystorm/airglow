import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { test } from "node:test";

const path = new URL("../web/js/overview-workspaces.js", import.meta.url);
const W = existsSync(path) ? await import(`data:text/javascript;base64,${Buffer.from(readFileSync(path, "utf8")).toString("base64")}`) : {};
const original = () => ({ tiles: [
  { id: "rig", type: "lighting", w: 3, h: 2 },
  { id: "cpu", type: "sensor", sensors: ["auto/cpu_temp"], style: "spark", w: 1, h: 1, label: "Processor" },
], pins: ["auto/cpu_temp"] });

test("legacy layout becomes the first workspace without changing widths or sharing tile references", () => {
  assert.equal(typeof W.initializeWorkspaces, "function", "workspace initialization is implemented");
  const doc = original(), before = structuredClone(doc), tile = doc.tiles[0];
  W.initializeWorkspaces(doc);
  assert.equal(doc.active_workspace, "my-workspace");
  assert.deepEqual(doc.tiles, before.tiles);
  assert.equal(doc.tiles[0], tile);
  assert.deepEqual(doc.workspaces[0], { id: "my-workspace", name: "My workspace", ...before });
  doc.tiles[0].w = 2;
  assert.equal(doc.workspaces[0].tiles[0].w, 3);
});

test("switching stores explicit edits and restores both tile widths and pins", () => {
  const doc = original(); W.initializeWorkspaces(doc);
  W.createWorkspace(doc, "Evening");
  doc.tiles[0].w = 4; doc.pins = [];
  W.switchWorkspace(doc, "my-workspace");
  assert.equal(doc.tiles[0].w, 3);
  assert.deepEqual(doc.pins, ["auto/cpu_temp"]);
  W.switchWorkspace(doc, "evening");
  assert.equal(doc.tiles[0].w, 4);
  assert.deepEqual(doc.pins, []);
});

test("workspaces validate names and bounds before changing any document state", () => {
  const doc = original(); W.initializeWorkspaces(doc);
  assert.throws(() => W.createWorkspace(doc, " "), /name/i);
  for (let i = 1; i < 8; i++) W.createWorkspace(doc, "Evening");
  assert.equal(new Set(doc.workspaces.map(w => w.id)).size, 8);
  const before = structuredClone(doc);
  assert.throws(() => W.createWorkspace(doc, "Too many"), /eight|8/i);
  assert.throws(() => W.renameWorkspace(doc, doc.active_workspace, "x".repeat(41)), /40/);
  assert.deepEqual(doc, before);
});

test("deleting the active workspace switches to a retained layout and never removes the last", () => {
  const doc = original(); W.initializeWorkspaces(doc);
  W.createWorkspace(doc, "Evening"); doc.tiles[0].w = 1;
  W.renameWorkspace(doc, "evening", "After dark");
  assert.equal(doc.workspaces[1].name, "After dark");
  W.deleteWorkspace(doc, "evening");
  assert.equal(doc.active_workspace, "my-workspace");
  assert.equal(doc.tiles[0].w, 3);
  assert.throws(() => W.deleteWorkspace(doc, "my-workspace"), /last/i);
});

test("undo and redo restore workspace operations and coalesce a continuous name edit", () => {
  const doc = original(); W.initializeWorkspaces(doc);
  const history = W.createLayoutHistory(doc);
  doc.tiles[1].label = "C"; history.record(doc, "label-cpu");
  doc.tiles[1].label = "CPU"; history.record(doc, "label-cpu");
  W.createWorkspace(doc, "Evening"); history.record(doc);
  let next = history.undo();
  assert.equal(next.workspaces.length, 1); assert.equal(next.tiles[1].label, "CPU");
  next = history.undo(); assert.equal(next.tiles[1].label, "Processor");
  next = history.redo(); assert.equal(next.tiles[1].label, "CPU");
  next.tiles[0].w = 4; history.record(next);
  assert.equal(history.canRedo, false);
});

test("history has a bounded independent baseline and records no viewport-only steps", () => {
  const doc = original(); W.initializeWorkspaces(doc);
  const history = W.createLayoutHistory(doc, 3);
  assert.equal(history.record(doc), false);
  for (let i = 1; i <= 5; i++) { doc.tiles[1].label = `${i}`; history.record(doc); }
  assert.equal(history.undo().tiles[1].label, "4");
  assert.equal(history.undo().tiles[1].label, "3");
  assert.equal(history.undo(), null);
});

test("export/import carries only a bounded layout and cannot import hardware settings", () => {
  const doc = original(); W.initializeWorkspaces(doc);
  const recipe = W.exportWorkspace(doc);
  recipe.scene = { on: false }; recipe.settings = { fan_failsafe_duty: 0 };
  const imported = W.importWorkspace(JSON.stringify(recipe));
  assert.deepEqual(imported, { name: "My workspace", ...original() });
  assert.equal(Object.hasOwn(imported, "scene"), false);
  imported.tiles[0].w = 1;
  assert.equal(doc.tiles[0].w, 3);
});

test("malformed layout imports fail atomically including duplicate ids and unsafe sizes", () => {
  const doc = original(); W.initializeWorkspaces(doc);
  const recipe = W.exportWorkspace(doc);
  for (const alter of [
    r => { r.version = 99; }, r => { r.tiles.push(r.tiles[0]); },
    r => { r.tiles[0].w = 20; }, r => { r.tiles[1].sensors = ["bad\nreading"]; },
    r => { r.pins = Array(5).fill("cpu"); }, r => { r.tiles[1].style = "invalid"; },
    r => { r.tiles[0].type = "script"; }, r => { r.name = ""; },
  ]) {
    const bad = structuredClone(recipe); alter(bad);
    assert.throws(() => W.importWorkspace(JSON.stringify(bad)));
  }
  assert.throws(() => W.importWorkspace(" ".repeat(100001)), /large/i);
});

test("starter workspaces retain real readings and never shrink the saved original", () => {
  const doc = original(); W.initializeWorkspaces(doc);
  const lighting = W.starterWorkspace(doc, "lighting");
  assert.equal(lighting.tiles[0].type, "lighting");
  assert.equal(lighting.tiles[0].w, 3);
  assert.ok(lighting.tiles.some(t => t.type === "presets"));
  const monitoring = W.starterWorkspace(doc, "monitoring");
  assert.ok(monitoring.tiles.some(t => t.type === "sensor" && t.sensors[0] === "auto/cpu_temp"));
  assert.equal(doc.tiles[0].w, 3);
});

test("server reconciliation keeps matching tile objects while removing obsolete optional fields", () => {
  const doc = original(), tile = doc.tiles[1]; W.initializeWorkspaces(doc);
  const next = structuredClone(doc); delete next.tiles[1].label;
  W.restoreOverview(doc, next);
  assert.equal(doc.tiles[1], tile);
  assert.equal(Object.hasOwn(tile, "label"), false);
  next.tiles[0].w = 1;
  assert.equal(doc.tiles[0].w, 3);
});
