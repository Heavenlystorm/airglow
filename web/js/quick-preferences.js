import { $$, S, emit, esc, on, post, toast } from "./core.js";

const empty = { favorites: [], recents: [] };
let writes = Promise.resolve();
const same = (a, b) => a.kind === b.kind && a.id === b.id;
export const isFavorite = (kind, id) => (S.settings?.settings?.quick_actions?.favorites || []).some(item => same(item, { kind, id }));

export function favoriteButton(kind, id, name) {
  const pinned = isFavorite(kind, id);
  const label = esc(`${pinned ? "Unpin" : "Pin"} ${name}`);
  return `<button type="button" class="favorite-button" data-favorite-id="${esc(id)}" data-favorite-kind="${kind}" data-favorite-name="${esc(name)}" aria-pressed="${pinned}" aria-label="${label}" title="${label}"><svg aria-hidden="true"><use href="#i-favorite"/></svg></button>`;
}

// Use the live catalog to resolve names, and omit entries that no longer exist.
export function groupedActions(items, prefs = S.settings?.settings?.quick_actions || empty) {
  const seen = new Set();
  function resolve(entries) {
    const ordered = [];
    for (const entry of entries) {
      const item = items.find(candidate => same(candidate, entry));
      if (item && !seen.has(item)) { seen.add(item); ordered.push(item); }
    }
    return ordered;
  }
  const favorites = resolve(prefs.favorites || []);
  const recents = resolve(prefs.recents || []);
  return [["Favorites", favorites], ["Recently used", recents], ["All actions", items.filter(item => !seen.has(item))]];
}

function update(operation, kind, id) {
  const send = async () => {
    const saved = await post("/api/quick-actions", { operation, kind, id });
    if (S.settings?.settings) S.settings.settings.quick_actions = saved;
    for (const button of $$("[data-favorite-id]")) {
      const pinned = isFavorite(button.dataset.favoriteKind, button.dataset.favoriteId);
      button.setAttribute("aria-pressed", String(pinned));
      button.title = `${pinned ? "Unpin" : "Pin"} ${button.dataset.favoriteName}`;
      button.setAttribute("aria-label", button.title);
    }
    emit("quick:changed");
  };
  const result = writes.then(send);
  writes = result.catch(() => {});
  return result;
}

export const setFavorite = (kind, id, pinned) => update(pinned ? "pin" : "unpin", kind, id);

export function initQuickPreferences() {
  document.addEventListener("click", async event => {
    const button = event.target.closest("[data-favorite-id]");
    if (!button || button.getAttribute("aria-busy") === "true") return;
    const { favoriteKind: kind, favoriteId: id } = button.dataset;
    button.setAttribute("aria-busy", "true");
    try { await setFavorite(kind, id, !isFavorite(kind, id)); }
    catch (error) { toast(`Could not save favorite: ${error.message}`, true); }
    finally { button.removeAttribute("aria-busy"); }
  });
  for (const kind of ["effect", "profile"]) {
    on(kind === "profile" ? "profile:used" : "effect:applied", id => {
      if (id) update("use", kind, id).catch(error => toast(`Could not save recent ${kind}: ${error.message}`, true));
    });
  }
}
