// Overview documents contain presentation only: never scene, fan or device settings.
const clone = (value) => structuredClone(value);
const snapshot = ({ tiles, pins }) => ({ tiles: clone(tiles), pins: [...pins] });
const byId = (doc, id) => doc.workspaces?.find((w) => w.id === id);
const nameOf = (value) => {
  if (typeof value !== "string" || !value.trim() || value.trim().length > 40 || /[\x00-\x1f]/.test(value)) {
    throw new Error("Give the workspace a name of 1–40 characters.");
  }
  return value.trim();
};

export function initializeWorkspaces(doc) {
  if (!doc.workspaces?.length) {
    doc.workspaces = [{ id: "my-workspace", name: "My workspace", ...snapshot(doc) }];
    doc.active_workspace = "my-workspace";
  }
  // A retained collection can have an unnamed current layout after an older client's reset.
  // Keep that layout intact; choosing a named workspace is an explicit user action.
  if (!byId(doc, doc.active_workspace)) doc.active_workspace = null;
  return doc;
}

export function syncWorkspace(doc) {
  const active = byId(doc, doc.active_workspace);
  if (active) Object.assign(active, snapshot(doc));
}

function uniqueId(doc, name) {
  const base = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 26) || "workspace";
  let id = base, n = 1;
  while (byId(doc, id)) id = `${base}-${++n}`;
  return id;
}

export function createWorkspace(doc, name, layout = doc) {
  name = nameOf(name);
  if (doc.workspaces.length >= 8) throw new Error("You can keep up to eight workspaces. Delete one first.");
  const next = validateLayout(layout); // Validate the entire replacement before mutating the document.
  const id = uniqueId(doc, name);
  syncWorkspace(doc);
  doc.workspaces.push({ id, name, ...snapshot(next) });
  Object.assign(doc, next, { active_workspace: id });
  return id;
}

export function switchWorkspace(doc, id) {
  const next = byId(doc, id);
  if (!next) throw new Error("That workspace is no longer available.");
  if (id === doc.active_workspace) return false;
  syncWorkspace(doc);
  Object.assign(doc, snapshot(next), { active_workspace: id });
  return true;
}

export function renameWorkspace(doc, id, name) {
  const value = nameOf(name), workspace = byId(doc, id);
  if (!workspace) throw new Error("That workspace is no longer available.");
  workspace.name = value;
}

export function deleteWorkspace(doc, id) {
  if (doc.workspaces.length <= 1) throw new Error("Keep at least the last workspace.");
  const at = doc.workspaces.findIndex((w) => w.id === id);
  if (at < 0) throw new Error("That workspace is no longer available.");
  doc.workspaces.splice(at, 1);
  if (doc.active_workspace === id) {
    const next = doc.workspaces[Math.max(0, at - 1)];
    Object.assign(doc, snapshot(next), { active_workspace: next.id });
  }
}

export function restoreOverview(doc, next) {
  const known = new Map(doc.tiles.map((t) => [t.id, t]));
  doc.tiles = next.tiles.map((t) => {
    const target = known.get(t.id) || {};
    for (const key of Object.keys(target)) delete target[key];
    return Object.assign(target, clone(t));
  });
  doc.pins = [...next.pins];
  if (next.workspaces) doc.workspaces = clone(next.workspaces);
  if (Object.hasOwn(next, "active_workspace")) doc.active_workspace = next.active_workspace;
  return doc;
}

export function createLayoutHistory(initial, limit = 50) {
  let entries = [clone(initial)], index = 0, lastGroup = null;
  return {
    get canUndo() { return index > 0; },
    get canRedo() { return index < entries.length - 1; },
    record(doc, group = null) {
      syncWorkspace(doc);
      if (JSON.stringify(entries[index]) === JSON.stringify(doc)) return false;
      entries.splice(index + 1);
      if (group && group === lastGroup && index > 0) entries[index] = clone(doc);
      else { entries.push(clone(doc)); index++; }
      if (entries.length > limit) { entries.shift(); index--; }
      lastGroup = group;
      return true;
    },
    endGroup() { lastGroup = null; },
    undo() { lastGroup = null; return index > 0 ? clone(entries[--index]) : null; },
    redo() { lastGroup = null; return index < entries.length - 1 ? clone(entries[++index]) : null; },
  };
}

function validateLayout(raw) {
  if (!raw || !Array.isArray(raw.tiles) || raw.tiles.length > 32) throw new Error("A workspace holds up to 32 tiles.");
  const seen = new Set();
  const sensor = (id) => {
    if (typeof id !== "string" || !id.length || id.length > 200 || /[\x00-\x1f]/.test(id)) throw new Error("Invalid reading in the workspace.");
    return id;
  };
  const tiles = raw.tiles.map((t) => {
    if (!t || typeof t.id !== "string" || !/^[a-z0-9-]{1,32}$/.test(t.id) || seen.has(t.id)) throw new Error("Tile names must be unique short IDs.");
    seen.add(t.id);
    if (!["sensor", "lighting", "presets", "fans", "control"].includes(t.type)) throw new Error("Unknown tile type.");
    const w = t.w ?? 1, h = t.h ?? 1;
    if (!Number.isInteger(w) || w < 1 || w > 4 || !Number.isInteger(h) || h < 1 || h > 3) throw new Error("Invalid tile size.");
    const tile = { id: t.id, type: t.type, w, h };
    if (t.type === "sensor") {
      if (!Array.isArray(t.sensors) || !t.sensors.length || t.sensors.length > 2) throw new Error("A tile shows one or two readings.");
      const style = t.style ?? "spark";
      if (!["spark", "ring", "graph", "bar"].includes(style)) throw new Error("Unknown tile style.");
      Object.assign(tile, { sensors: t.sensors.map(sensor), style });
      if (t.label != null && (typeof t.label !== "string" || t.label.length > 40)) throw new Error("Tile names can be up to 40 characters.");
      if (t.label?.trim()) tile.label = t.label.trim();
    }
    return tile;
  });
  const pins = raw.pins ?? [];
  if (!Array.isArray(pins) || pins.length > 4) throw new Error("Up to four readings can be pinned.");
  return { tiles, pins: [...new Set(pins.map(sensor))] };
}

export function exportWorkspace(doc) {
  return { format: "airglow-workspace", version: 1, name: byId(doc, doc.active_workspace)?.name || "My workspace", ...snapshot(doc) };
}

export function importWorkspace(text) {
  if (typeof text !== "string" || text.length > 100000) throw new Error("That workspace file is too large.");
  let value;
  try { value = JSON.parse(text); } catch { throw new Error("Choose a valid workspace JSON file."); }
  if (!value || value.format !== "airglow-workspace" || value.version !== 1) throw new Error("This is not a supported Airglow workspace.");
  return { name: nameOf(value.name), ...validateLayout(value) };
}

export function starterWorkspace(doc, kind) {
  if (kind === "current") return snapshot(doc);
  const sensorTiles = doc.tiles.filter((t) => t.type === "sensor");
  // Existing sensor IDs remain honest even on machines where a reading is temporarily missing.
  // These arrangements contain no assumptions about detected fan counts or chassis models.
  const tiles = kind === "monitoring"
    ? [...sensorTiles, ...doc.tiles.filter((t) => ["fans", "control"].includes(t.type))]
    : [doc.tiles.find((t) => t.type === "lighting") || { id: "lighting", type: "lighting", w: 3, h: 3 },
      ...sensorTiles.slice(0, 3), doc.tiles.find((t) => t.type === "presets") || { id: "presets", type: "presets", w: 3, h: 1 }];
  const result = { tiles: clone(tiles), pins: [...doc.pins] };
  if (kind === "lighting") {
    result.tiles[0].w = 3; result.tiles[0].h = 3;
    const used = new Set();
    for (const tile of result.tiles) {
      const base = tile.id; let n = 1;
      while (used.has(tile.id)) tile.id = `${base.slice(0, 27)}-${++n}`;
      used.add(tile.id);
    }
  }
  return result;
}
