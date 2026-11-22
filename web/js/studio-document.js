// Studio documents, bounded history and browser-local recovery. No renderer or hardware access.
export const MASTER = { palette: ["#ff2fb4", "#2f5bff"], speed: 100, brightness: 100, react: 50 };
export const DRAFT_KEY = "airglow.studio.unsaved.v1";
export const PALETTE_KEY = "airglow.studio.palettes.v1";
export const uid = () => "k" + Math.random().toString(36).slice(2, 6);

export function newDoc(d = {}) {
  return normalizeDoc({ fx_id: null, name: "Untitled effect", author: "", license: "", description: "", tags: [],
    remix_of: "", mode: "layers", layers: [], master: structuredClone(MASTER), expose: {}, source: "", values: {},
    ...structuredClone(d), dirty: !d.fx_id });
}

export function normalizeDoc(doc) {
  doc.master = { ...structuredClone(MASTER), ...(doc.master || {}) };
  doc.master.palette = validPalette(doc.master.palette) ? doc.master.palette.slice() : structuredClone(MASTER.palette);
  for (const key of ["speed", "brightness", "react"]) {
    const value = Number(doc.master[key]);
    doc.master[key] = Number.isFinite(value) ? value : MASTER[key];
  }
  doc.expose = doc.expose && typeof doc.expose === "object" ? doc.expose : {};
  doc.tags = Array.isArray(doc.tags) ? doc.tags : [];
  doc.values = doc.values && typeof doc.values === "object" ? doc.values : {};
  const seen = new Set();
  doc.layers = (Array.isArray(doc.layers) ? doc.layers : []).map((layer, i) => {
    let id = layer.id;
    if (!id || !/^[a-z0-9][a-z0-9-]{0,15}$/.test(id) || seen.has(id)) id = String(i);
    while (seen.has(id)) id = uid();
    seen.add(id);
    return { ...layer, id, enabled: layer.enabled !== false, opacity: layer.opacity ?? 100,
      blend: layer.blend || "normal", mask: layer.mask || "all", palette: !!layer.palette,
      react: { what: layer.react?.what === "speed" ? "speed" : "strength", amount: Number(layer.react?.amount) || 0 },
      params: layer.params || {}, expose: layer.expose || {} };
  });
  return doc;
}

export function snapDoc(doc) {
  if (!doc) return "null";
  return JSON.stringify({ name: doc.name, author: doc.author, license: doc.license,
    description: doc.description, tags: doc.tags, remix_of: doc.remix_of,
    mode: doc.mode, layers: doc.layers, master: doc.master, expose: doc.expose, source: doc.source, values: doc.values,
    fromLayers: doc.fromLayers || undefined });
}

export function createHistory() {
  return {
    stack: [], i: -1, at: 0,
    reset(snap) { this.stack = [snap]; this.i = 0; this.at = 0; },
    push(snap, now) {
      if (this.stack[this.i] === snap) return;
      if (this.i > 0 && this.i === this.stack.length - 1 && now - this.at < 500) this.stack[this.i] = snap;
      else {
        this.stack = this.stack.slice(0, this.i + 1); this.stack.push(snap);
        if (this.stack.length > 60) this.stack.shift();
        this.i = this.stack.length - 1;
      }
      this.at = now;
    },
    go(dir) {
      const next = this.i + dir;
      if (next < 0 || next >= this.stack.length) return null;
      this.i = next; return this.stack[next];
    },
  };
}

const WEEK = 7 * 864e5;
const MAX_DOCUMENT = 256 * 1024;
const MAX_SHELF = 2 * 1024 * 1024;
const CHECKPOINT_KEY = "airglow.studio.checkpoints.v1";
const identifier = () => globalThis.crypto?.randomUUID?.() || `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

function recoveryKey(doc) {
  if (doc.fx_id) return `effect:${doc.fx_id}`;
  doc._variationId ||= identifier();
  return `draft:${doc._variationId}`;
}

function readableDoc(doc) {
  return !!doc && typeof doc.name === "string" && ["layers", "code"].includes(doc.mode)
    && (!doc.layers || (Array.isArray(doc.layers) && doc.layers.every(l => l && typeof l.type === "string")))
    && (doc.mode !== "code" || typeof doc.source === "string");
}

// Keep the newest draft in the original {doc, at} envelope so an upgrade can recover the previous session.
// A failed write overlays the persisted shelf in memory until a subsequent write succeeds.
export function createDraftStorage(storage, now = Date.now) {
  let cached = [], unavailable = false;
  const pending = new Map();
  function entries() {
    let raw;
    try {
      raw = storage.getItem(DRAFT_KEY); unavailable = false;
    } catch { unavailable = true; }
    if (!unavailable) {
      let head = null;
      try { if (raw && raw.length <= MAX_SHELF) head = JSON.parse(raw); } catch { /* The previous version ignored malformed drafts. */ }
      cached = (!head ? [] : [{ doc: head.doc, at: head.at }, ...(Array.isArray(head.others) ? head.others : [])])
        .filter(e => readableDoc(e?.doc) && Number.isFinite(e.at) && Number.isFinite(new Date(e.at).getTime())).map(e => {
          if (!e.doc.fx_id && !e.doc._variationId) e.doc._variationId = `legacy-${e.at}`;
          e.doc.dirty = true;
          return { doc: normalizeDoc(e.doc), key: recoveryKey(e.doc), at: e.at };
        });
    }
    const changed = [...pending.values()].reverse().filter(Boolean);
    return [...changed, ...cached.filter(e => !pending.has(e.key))].filter(e => now() - e.at <= WEEK).sort((a, b) => b.at - a.at);
  }
  function flush() {
    const rows = entries();
    if (unavailable) return false;
    try {
      const records = rows.map(({ doc, at }) => ({ doc, at }));
      if (records.length) {
        const [head, ...others] = records;
        const json = JSON.stringify({ ...head, others });
        if (json.length > MAX_SHELF || records.some(e => JSON.stringify(e.doc).length > MAX_DOCUMENT)) return false;
        storage.setItem(DRAFT_KEY, json);
      } else storage.removeItem(DRAFT_KEY);
      cached = rows; pending.clear(); return true;
    } catch { return false; }
  }
  return {
    list: entries,
    get temporary() { return pending.size > 0 || unavailable; },
    write(doc) {
      if (!doc) return false;
      const key = recoveryKey(doc);
      pending.delete(key);
      pending.set(key, doc.dirty ? { key, doc: structuredClone(doc), at: now() } : null);
      return flush();
    },
    read() { const row = entries()[0]; return row ? structuredClone(row.doc) : null; },
    remove(...keys) { for (const key of keys) pending.set(key, null); return flush(); },
  };
}

// Named versions are deliberately independent of draft expiry, undo, and automatic saves.
export function createCheckpointStorage(storage, now = Date.now) {
  function list() {
    const raw = storage.getItem(CHECKPOINT_KEY);
    if (!raw) return [];
    if (raw.length > MAX_SHELF) throw new Error("This checkpoint collection is too large to read.");
    const data = JSON.parse(raw);
    if (!Array.isArray(data)) throw new Error("This checkpoint collection could not be read.");
    return data.filter(x => typeof x?.id === "string" && typeof x.name === "string" && Number.isFinite(x.at)
      && Number.isFinite(new Date(x.at).getTime()) && readableDoc(x.doc))
      .map(x => ({ ...x, doc: normalizeDoc(x.doc) }));
  }
  function write(rows) {
    const raw = JSON.stringify(rows);
    if (raw.length > MAX_SHELF) throw new Error("The checkpoint shelf is full. Delete an older checkpoint and try again.");
    if (rows.length) storage.setItem(CHECKPOINT_KEY, raw); else storage.removeItem(CHECKPOINT_KEY);
  }
  return {
    list,
    capture(doc, title) {
      const name = String(title || "").trim().slice(0, 60);
      if (!name) throw new Error("Enter a checkpoint name.");
      const snapshot = snapDoc(doc);
      if (!readableDoc(doc) || snapshot.length > MAX_DOCUMENT) throw new Error("This document is too large or incomplete to keep as a checkpoint.");
      const row = { id: identifier(), name, at: now(), doc: JSON.parse(snapshot) };
      write([row, ...list()]); return row;
    },
    remove(id) { write(list().filter(row => row.id !== id)); },
  };
}

export function compareStudioDocuments(before, after) {
  const changes = [];
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  if (before.mode !== after.mode) changes.push(`Editing mode: ${before.mode} → ${after.mode}`);
  if (before.mode === "layers" && after.mode === "layers") {
    const old = new Map(before.layers.map(l => [l.id, l])), next = new Map(after.layers.map(l => [l.id, l]));
    for (const layer of before.layers) if (!next.has(layer.id)) changes.push(`Removed layer: ${layer.type}`);
    for (const layer of after.layers) {
      if (!old.has(layer.id)) changes.push(`Added layer: ${layer.type}`);
      else if (!same(old.get(layer.id), layer)) {
        const prior = old.get(layer.id);
        if (!same(prior.paint, layer.paint)) changes.push(`Paint mask changed: ${layer.type}`);
        if (!same({ ...prior, paint: null }, { ...layer, paint: null })) changes.push(`Layer settings changed: ${layer.type}`);
      }
    }
    if (!same(before.layers.filter(l => next.has(l.id)).map(l => l.id), after.layers.filter(l => old.has(l.id)).map(l => l.id)))
      changes.push("Layer order changed");
    for (const [key, label] of Object.entries({ palette: "Palette", brightness: "Brightness", speed: "Speed", react: "Reactivity" })) {
      if (!same(before.master[key], after.master[key])) changes.push(key === "palette" ? "Palette changed" : `${label}: ${before.master[key]} → ${after.master[key]}`);
    }
    if (!same(before.expose, after.expose)) changes.push("Exposed controls changed");
  }
  if (before.mode === "code" && after.mode === "code") {
    if (!!before.fromLayers !== !!after.fromLayers || before.fromLayers && !same(
      [before.layers, before.master, before.expose], [after.layers, after.master, after.expose])) changes.push("Return-to-layers settings changed");
    if (before.source !== after.source) {
      const a = before.source.split("\n"), b = after.source.split("\n");
      let line = 0;
      while (line < a.length && line < b.length && a[line] === b[line]) line++;
      changes.push(`Source changed from line ${line + 1} (${a.length} → ${b.length} lines)`);
    }
    if (!same(before.values, after.values)) changes.push("Control values changed");
  }
  for (const [key, label] of Object.entries({ name: "Effect name", author: "Author", license: "License", description: "Description", tags: "Tags", remix_of: "Remix credit" }))
    if (!same(before[key], after[key])) changes.push(`${label} changed`);
  return changes;
}

const validPalette = colors => Array.isArray(colors) && colors.length >= 2 && colors.length <= 4
  && colors.every(c => typeof c === "string" && /^#[0-9a-f]{6}$/i.test(c));

export function cleanPalettes(value) {
  if (!Array.isArray(value)) return [];
  return value.filter(p => p && typeof p.name === "string" && p.name.trim() && validPalette(p.colors))
    .slice(0, 12).map(p => ({ name: p.name.trim().slice(0, 32), colors: p.colors.map(c => c.toLowerCase()) }));
}

export function addPalette(existing, name, colors) {
  if (!validPalette(colors)) throw new Error("A palette needs two to four hex colours.");
  name = String(name || "").trim().slice(0, 32);
  if (!name) throw new Error("Name this palette so you can find it again.");
  const palette = { name, colors: colors.map(c => c.toLowerCase()) };
  return [palette, ...cleanPalettes(existing).filter(p => p.colors.join() !== palette.colors.join())].slice(0, 12);
}

export function workflowStatus(state, doc) {
  const output = state?.effective;
  if (output?.on && state?.trial?.kind === "studio" && output.effect === "fx:draft")
    return { kind: "live", label: "Live draft preview", detail: "Your lights are trying the draft. Stop preview to restore the previous scene." };
  const current = output?.effect;
  if (output?.on && !state?.trial && current?.startsWith("fx:") && current !== "fx:draft" && (!doc || doc.fx_id === current))
    return { kind: "applied", label: "Saved effect applied", detail: doc?.dirty ? "Your saved effect is applied. These edits are still unsaved." : "The saved effect is selected on your lights." };
  const overridden = state?.trial?.kind === "studio" || (doc?.fx_id && state?.scene?.effect === doc.fx_id);
  return { kind: "preview", label: "Preview only", detail: overridden
    ? "The draft or saved effect is selected, but current lighting is off or controlled by an override. Stop preview to restore your scene."
    : doc ? "Edits appear in this preview. Save the effect, then apply it when you are ready."
      : "Open an effect to edit it on screen. Apply it when you're ready." };
}
