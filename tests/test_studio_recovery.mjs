import { test } from "node:test";
import assert from "node:assert/strict";
import { createDraftStorage, createCheckpointStorage, compareStudioDocuments, DRAFT_KEY, newDoc, snapDoc } from "../web/js/studio-document.js";

function memory() {
  const data = new Map();
  return { data, getItem: key => data.get(key), setItem: (key, value) => data.set(key, value), removeItem: key => data.delete(key) };
}
const draft = () => newDoc({ name: "Same name", layers: [{ id: "sky", type: "gradient" }] });

test("legacy drafts, duplicate names, promotion and individual expiration survive reload", () => {
  let t = 100;
  const storage = memory(), open = () => createDraftStorage(storage, () => t);
  storage.setItem(DRAFT_KEY, JSON.stringify({ doc: draft(), at: t }));
  const a = open(), first = a.list()[0], second = draft();
  assert.equal(first.key, "draft:legacy-100");
  t += 100; a.write(second); assert.equal(a.list().length, 2);
  const b = open(); b.remove(b.list()[0].key);
  assert.equal(JSON.parse(storage.getItem(DRAFT_KEY)).doc._variationId, "legacy-100");
  t += 6 * 864e5;
  b.write(second); t += 2 * 864e5;
  assert.deepEqual(open().list().map(e => e.doc._variationId), [second._variationId]);
});

test("unavailable storage protects drafts across navigation and retries without losing other creations", () => {
  const storage = memory(), store = createDraftStorage(storage);
  const write = storage.setItem, read = storage.getItem;
  const a = draft(), b = draft();
  store.write(a);
  storage.setItem = () => { throw Error("QuotaExceeded"); };
  assert.equal(store.write(b), false);
  b.name = "Changed after the write";
  assert.equal(store.temporary, true);
  assert.equal(store.list().filter(r => r.doc.name === "Same name").length, 2);
  assert.equal(createDraftStorage(storage).list().length, 1);
  storage.getItem = () => { throw Error("Access denied"); };
  assert.equal(store.write(b), false);
  assert.equal(store.list().length, 2);
  storage.getItem = read; storage.setItem = write;
  assert.equal(store.write(b), true);
  assert.equal(store.temporary, false);
  assert.deepEqual(new Set(createDraftStorage(storage).list().map(r => r.doc.name)), new Set(["Same name", "Changed after the write"]));
});

test("checkpoints persist independent full versions, exclude live identities and survive draft expiry", () => {
  let t = 100; const storage = memory(), store = createCheckpointStorage(storage, () => t);
  const d = draft();
  Object.assign(d, { paintPending: true, _variationId: "abc", fx_id: "fx:original", tags: ["ambient"] });
  d.layers[0].paint = { v: 1, points: [[500, 600]] };
  const painted = store.capture(d, " Painted ");
  Object.assign(d, { mode: "code", source: "A\nB", values: { density: 30 }, fromLayers: true });
  t += 30 * 864e5; const code = store.capture(d, "Before conversion");
  const rows = createCheckpointStorage(storage).list();
  assert.equal(rows[1].id, painted.id); assert.equal(rows[1].name, "Painted");
  assert.equal(snapDoc(rows[0].doc), snapDoc(d));
  for (const key of ["paintPending", "_variationId", "fx_id", "dirty"]) assert.equal(Object.hasOwn(rows[0].doc, key), false);
  assert.deepEqual(rows[1].doc.layers[0].paint.points, [[500, 600]]);
  storage.setItem = () => { throw Error("Full"); };
  assert.throws(() => store.capture(d, "Cannot save"), /Full/);
  assert.throws(() => store.remove(code.id), /Full/);
  assert.deepEqual(store.list().map(r => r.id), [code.id, painted.id]);
});

test("the comparison distinguishes order, paint, layer settings, master controls, metadata and code changes", () => {
  const a = newDoc({ name: "Before", layers: [{ id: "a", type: "plasma" }, { id: "b", type: "wave" }] });
  const b = structuredClone(a); b.layers.reverse(); b.layers[1].paint = { v: 1, points: [] };
  b.layers[0].opacity = 40; b.expose.brightness = "Glow"; b.master.brightness = 47;
  b.layers.push({ id: "c", type: "gradient" }); b.name = "After";
  assert.deepEqual(compareStudioDocuments(a, b), ["Layer settings changed: wave", "Paint mask changed: plasma", "Added layer: gradient", "Layer order changed", "Brightness: 100 → 47", "Exposed controls changed", "Effect name changed"]);
  const code = newDoc({ mode: "code", source: "first\nsecond" });
  assert.deepEqual(compareStudioDocuments(code, { ...code, source: "first\nthird\nfourth", values: { x: 4 } }), [
    "Source changed from line 2 (2 → 3 lines)", "Control values changed"]);
  assert.equal(compareStudioDocuments(a, code)[0], "Editing mode: layers → code");
});
