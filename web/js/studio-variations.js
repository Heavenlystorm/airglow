// Browser-local master variations. No renderer, file-format or hardware dependencies.
export const VARIATION_KEY = "airglow.studio.variations.v1";
export const VARIATION_LIMIT = 8;
const CONTROLS = ["palette", "speed", "brightness", "react"];
const bounds = { speed: 200, brightness: 100, react: 100 };
const clone = value => structuredClone(value);
const identifier = () => globalThis.crypto?.randomUUID?.() || `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
const validKey = key => typeof key === "string" && /^(effect|draft):[a-zA-Z0-9:._-]{1,160}$/.test(key);
const locksOf = locks => Object.fromEntries(CONTROLS.map(key => [key, locks?.[key] === true]));

export function documentKey(doc) {
  if (typeof doc?.fx_id === "string" && validKey(`effect:${doc.fx_id}`)) return `effect:${doc.fx_id}`;
  if (!doc || typeof doc !== "object") return null;
  if (typeof doc._variationId !== "string" || !validKey(`draft:${doc._variationId}`)) doc._variationId = identifier();
  return `draft:${doc._variationId}`;
}

function cleanMaster(value) {
  if (!value || !Array.isArray(value.palette) || value.palette.length < 2 || value.palette.length > 4
    || !value.palette.every(c => typeof c === "string" && /^#[0-9a-f]{6}$/i.test(c))) return null;
  for (const [key, max] of Object.entries(bounds))
    if (!Number.isFinite(value[key]) || value[key] < 0 || value[key] > max) return null;
  return { palette: value.palette.map(c => c.toLowerCase()), speed: value.speed, brightness: value.brightness, react: value.react };
}

export function captureVariation(doc) {
  const master = cleanMaster(doc?.master);
  return doc?.mode === "layers" && master ? { scope: documentKey(doc), master } : null;
}

export function restoreVariation(doc, variation) {
  const master = cleanMaster(variation?.master);
  if (doc?.mode !== "layers" || variation?.scope !== documentKey(doc) || !master) return false;
  doc.master = master;
  return true;
}

const rgb = hex => [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16));
const hex = colors => "#" + colors.map(c => Math.max(0, Math.min(255, Math.round(c))).toString(16).padStart(2, "0")).join("");

export function interpolateVariation(doc, a, b, amount, locks = {}) {
  const scope = documentKey(doc), left = cleanMaster(a?.master), right = cleanMaster(b?.master);
  if (doc?.mode !== "layers" || a?.scope !== scope || b?.scope !== scope || !left || !right
    || left.palette.length !== right.palette.length || !Number.isFinite(amount)) return null;
  const t = Math.max(0, Math.min(1, amount));
  const current = cleanMaster(doc.master);
  if (!current) return null;
  const result = { palette: left.palette.map((color, i) => {
    const next = rgb(right.palette[i]); return hex(rgb(color).map((c, j) => c + (next[j] - c) * t));
  }) };
  for (const key of Object.keys(bounds)) result[key] = Math.round(left[key] + (right[key] - left[key]) * t);
  for (const key of CONTROLS) if (locks[key] === true) result[key] = clone(current[key]);
  return result;
}

export function varyMaster(master, locks = {}, random = Math.random) {
  const result = cleanMaster(master);
  if (!result) return null;
  const delta = () => (Math.max(0, Math.min(1, random())) * 2 - 1);
  for (const [key, max] of Object.entries(bounds)) if (locks[key] !== true)
    result[key] = Math.round(Math.max(0, Math.min(max, result[key] + delta() * max * .15)));
  if (locks.palette !== true) {
    // A shared, small channel shift keeps the palette's relationships recognizable.
    const shift = [delta() * 24, delta() * 24, delta() * 24];
    result.palette = result.palette.map(c => hex(rgb(c).map((channel, i) => channel + shift[i])));
  }
  return result;
}

function cleanVariations(entries, scope) {
  if (!Array.isArray(entries)) return [];
  const seen = new Set();
  return entries.slice(0, 64).flatMap(entry => {
    const master = cleanMaster(entry?.master);
    if (!master || entry.scope !== scope || typeof entry.name !== "string" || !entry.name.trim()
      || typeof entry.id !== "string" || !/^[a-z0-9-]{1,64}$/i.test(entry.id) || seen.has(entry.id)) return [];
    seen.add(entry.id);
    return [{ id: entry.id, name: entry.name.trim().slice(0, 32), scope, master }];
  }).slice(0, VARIATION_LIMIT);
}

export function addVariation(existing, name, snapshot) {
  name = typeof name === "string" ? name.trim().slice(0, 32) : "";
  if (!name) throw new Error("Name this variation first.");
  if (!validKey(snapshot?.scope) || !cleanMaster(snapshot?.master)) throw new Error("This effect cannot be captured yet.");
  return [{ id: identifier(), name, scope: snapshot.scope, master: cleanMaster(snapshot.master) },
    ...cleanVariations(existing, snapshot.scope)].slice(0, VARIATION_LIMIT);
}

export function createVariationStore(storage) {
  const empty = () => ({ locks: locksOf(), variations: [] });
  function load() {
    try {
      const raw = storage.getItem(VARIATION_KEY) || "null";
      if (raw.length > 128 * 1024) return [];
      const value = JSON.parse(raw);
      if (value?.version !== 1 || !Array.isArray(value.documents)) return [];
      const seen = new Set();
      return value.documents.slice(0, 64).flatMap(entry => {
        if (!validKey(entry?.key) || seen.has(entry.key)) return [];
        seen.add(entry.key);
        return [{ key: entry.key, locks: locksOf(entry.locks), variations: cleanVariations(entry.variations, entry.key) }];
      }).slice(0, 24);
    } catch { return []; }
  }
  function save(documents) {
    try { storage.setItem(VARIATION_KEY, JSON.stringify({ version: 1, documents: documents.slice(0, 24) })); return true; }
    catch { return false; }
  }
  return {
    read(key) { const record = load().find(entry => entry.key === key); return record ? { locks: record.locks, variations: record.variations } : empty(); },
    write(key, value) {
      if (!validKey(key)) return false;
      const record = { key, locks: locksOf(value?.locks), variations: cleanVariations(value?.variations, key) };
      return save([record, ...load().filter(entry => entry.key !== key)]);
    },
    move(from, to) {
      if (!validKey(from) || !validKey(to)) return false;
      if (from === to) return true;
      const documents = load(), record = documents.find(entry => entry.key === from);
      if (!record) return true;
      return save([{ ...record, key: to, variations: record.variations.map(entry => ({ ...entry, scope: to })) },
        ...documents.filter(entry => entry.key !== from && entry.key !== to)]);
    },
  };
}

export function canPlayScene({ hidden = false, reducedSystem = false, motion = "system" } = {}) {
  return !hidden && !reducedSystem && motion !== "reduced";
}
