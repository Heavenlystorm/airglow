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
    mode: doc.mode, layers: doc.layers, master: doc.master, expose: doc.expose, source: doc.source, values: doc.values });
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

export function createDraftStorage(storage, now = Date.now) {
  return {
    write(doc) {
      try {
        if (doc?.dirty) storage.setItem(DRAFT_KEY, JSON.stringify({ doc, at: now() }));
        else storage.removeItem(DRAFT_KEY);
        return true;
      } catch { return false; }
    },
    read() {
      try {
        const raw = storage.getItem(DRAFT_KEY) || "null";
        if (raw.length > 256 * 1024) return null;
        const d = JSON.parse(raw);
        if (!d?.doc || typeof d.doc.name !== "string" || !d.doc.name || !Number.isFinite(d.at)
          || now() - d.at > 7 * 864e5 || !["layers", "code"].includes(d.doc.mode)) return null;
        return normalizeDoc(d.doc);
      } catch { return null; }
    },
    clear() { try { storage.removeItem(DRAFT_KEY); return true; } catch { return false; } },
  };
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
    : "Edits appear in this preview. Save the effect, then apply it when you are ready." };
}
