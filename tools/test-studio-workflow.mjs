// Pure document and preview lifecycle regressions. Run: node --test tools/test-studio-workflow.mjs
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

async function moduleAt(name) {
  const source = await readFile(new URL(`../web/js/${name}.js`, import.meta.url), "utf8").catch(() => "");
  return import(`data:text/javascript;base64,${Buffer.from(source).toString("base64")}`);
}
const flush = () => new Promise(setImmediate);

test("document history retains the original gesture, bounds history and discards redo after an edit", async () => {
  const m = await moduleAt("studio-document");
  assert.equal(typeof m.createHistory, "function", "Studio needs a reusable document history");
  const h = m.createHistory();
  h.reset("original"); h.push("a", 100); h.push("b", 200);
  assert.equal(h.go(-1), "original"); assert.equal(h.go(1), "b");
  h.go(-1); h.push("branch", 800); assert.equal(h.go(1), null);
  for (let i = 0; i < 80; i++) h.push(String(i), 2000 + i * 600);
  assert.equal(h.stack.length, 60); assert.equal(h.go(-1), "78");
});

test("normalizing recovered documents keeps stable layer identities and isolates palette copies", async () => {
  const m = await moduleAt("studio-document");
  assert.equal(typeof m.newDoc, "function");
  const d = m.newDoc({ name: "Recovered", layers: [{ id: "same", type: "solid" }, { id: "same", type: "wave" }] });
  assert.equal(d.layers[0].id, "same"); assert.notEqual(d.layers[1].id, "same");
  d.master.palette[0] = "#112233";
  assert.equal(m.newDoc({}).master.palette[0], "#ff2fb4");
  assert.equal(JSON.parse(m.snapDoc({ ...d, fx_id: "fx:me.old", dirty: true })).fx_id, undefined);
});

test("draft persistence keeps failed work, rejects expired or malformed documents and reports storage failure", async () => {
  const m = await moduleAt("studio-document");
  assert.equal(typeof m.createDraftStorage, "function");
  const data = new Map();
  const storage = { getItem: k => data.get(k), setItem: (k, v) => data.set(k, v), removeItem: k => data.delete(k) };
  let now = 100;
  const draft = m.createDraftStorage(storage, () => now);
  const d = m.newDoc({ name: "My work", layers: [{ type: "solid" }] });
  assert.equal(draft.write(d), true); assert.equal(draft.read().name, "My work");
  now += 8 * 864e5; assert.equal(draft.read(), null);
  storage.setItem = () => { throw new Error("full"); };
  assert.equal(draft.write(d), false);
});

test("favorite palettes validate colours, deduplicate and stay bounded without changing the source palette", async () => {
  const m = await moduleAt("studio-document");
  assert.equal(typeof m.addPalette, "function");
  const colors = ["#ABCDEF", "#123456"];
  let list = m.addPalette([], "Evening", colors);
  colors[0] = "#000000";
  assert.deepEqual(list[0].colors, ["#abcdef", "#123456"]);
  list = m.addPalette(list, "Renamed", ["#abcdef", "#123456"]);
  assert.equal(list.length, 1); assert.equal(list[0].name, "Renamed");
  assert.throws(() => m.addPalette(list, "Invalid", ["url(x)", "#ffffff"]));
  for (let i = 0; i < 30; i++) list = m.addPalette(list, String(i), ["#000000", `#${i.toString(16).padStart(6, "0")}`]);
  assert.equal(list.length, 12);
});

test("workflow status uses current scene and trial rather than a saved document or requested switch", async () => {
  const m = await moduleAt("studio-document");
  assert.equal(typeof m.workflowStatus, "function");
  const doc = { fx_id: "fx:me.sun", dirty: false };
  assert.equal(m.workflowStatus({}, doc).label, "Preview only");
  assert.equal(m.workflowStatus({ scene: { effect: doc.fx_id }, effective: { effect: doc.fx_id, on: true } }, doc).label, "Saved effect applied");
  assert.equal(m.workflowStatus({ scene: { effect: doc.fx_id }, trial: { kind: "profile" } }, doc).label, "Preview only");
  assert.equal(m.workflowStatus({ scene: { effect: "fx:draft" }, effective: { effect: "fx:draft", on: true }, trial: { kind: "studio" } }, doc).label, "Live draft preview");
  assert.equal(m.workflowStatus({ scene: { effect: "solid" } }, doc).label, "Preview only");
  assert.equal(m.workflowStatus({ scene: { effect: "fx:me.sun" }, effective: { effect: "fx:me.sun", on: true } }, null).label, "Saved effect applied");
  assert.equal(m.workflowStatus({ scene: { effect: doc.fx_id }, effective: { effect: "solid", on: true } }, doc).label, "Preview only");
  assert.equal(m.workflowStatus({ scene: { effect: "fx:draft" }, effective: { effect: "solid", on: true }, trial: { kind: "studio" } }, doc).label, "Preview only");
  assert.equal(m.workflowStatus({ scene: { effect: "fx:draft" }, effective: { effect: "fx:draft", on: false }, trial: { kind: "studio" } }, doc).label, "Preview only");
});

test("leaving during a live-preview start waits for it then cleans up; obsolete edits cannot restart it", async () => {
  const m = await moduleAt("studio-preview");
  assert.equal(typeof m.createLivePreview, "function");
  const requests = [];
  const live = m.createLivePreview({ post: (path, body) => new Promise((resolve, reject) => requests.push({ path, body, resolve, reject })) });
  const start = live.set(true, { name: "A" }); await flush();
  const update = live.sync({ name: "B" }); const stop = live.set(false); await flush();
  assert.equal(requests.length, 1); assert.equal(live.state.confirmed, false);
  requests[0].resolve({ lights: true }); await flush();
  assert.equal(requests.length, 2); assert.deepEqual(requests[1].body, { lights: false });
  requests[1].resolve({ lights: false }); await Promise.all([start, update, stop]);
  assert.equal(live.state.confirmed, false); assert.equal(live.state.wanted, false);
});

test("failed cleanup remains visible and does not claim live preview is stopped", async () => {
  const m = await moduleAt("studio-preview");
  assert.equal(typeof m.createLivePreview, "function");
  const live = m.createLivePreview({ post: async (_path, body) => { if (!body.lights) throw new Error("offline"); return { lights: true }; } });
  await live.set(true, { name: "A" }); await live.set(false);
  assert.equal(live.state.confirmed, true); assert.match(live.state.error, /offline/);
  live.observe({ trial: null }); assert.equal(live.state.confirmed, false);
});

test("preview requests snapshot the edited values and a failed start stays preview only", async () => {
  const m = await moduleAt("studio-preview");
  let delivered;
  const live = m.createLivePreview({ post: async (_path, body) => { if (!body.lights) return { lights: false }; delivered = body; throw new Error("invalid effect"); } });
  const payload = { values: { speed: 10 } };
  const request = live.set(true, payload); payload.values.speed = 99;
  await request;
  assert.equal(delivered.values.speed, 10);
  assert.equal(live.state.confirmed, false); assert.equal(live.state.wanted, false);
  assert.match(live.state.error, /invalid effect/);
});

test("an uncertain failed start sends cleanup without replaying the failed draft", async () => {
  const m = await moduleAt("studio-preview");
  const bodies = [];
  const live = m.createLivePreview({ post: async (_path, body) => {
    bodies.push(body);
    if (body.lights) throw new Error("Request timed out");
    return { lights: false };
  } });
  await live.set(true, { name: "A" });
  assert.deepEqual(bodies, [{ name: "A", lights: true }, { lights: false }]);
  assert.equal(live.state.confirmed, false);
  assert.match(live.state.error, /timed out/);
});
