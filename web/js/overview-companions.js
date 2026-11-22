import { lookbookCatalog } from "./lookbook-catalog.js";
import { DESK_FACE_KEY, DESK_TIMER_KEY, facePreferences, readTimer, remaining } from "./desk-state.js";

const chevron = '<svg aria-hidden="true"><use href="#i-chevron"/></svg>';
export function companionMarkup(type) {
  const heading = '<header class="tile-head"><span class="tile-name"></span></header>';
  if (type === "lookbook") {
    return heading + `<div class="ov-recipe-art" aria-hidden="true"></div><button type="button" data-companion="recipe" class="ov-recipe-open">
      <strong class="ov-recipe-name" aria-live="polite" aria-atomic="true">Open collection</strong><span class="ov-recipe-detail"></span><span class="ov-recipe-colors"></span>
      </button><div class="ov-recipe-controls" role="group" aria-label="Browse lighting recipes">
      <button type="button" data-recipe-step="-1" aria-label="Previous lighting recipe" title="Previous lighting recipe">${chevron}</button>
      <span class="ov-recipe-index" aria-hidden="true"></span>
      <button type="button" data-recipe-step="1" aria-label="Next lighting recipe" title="Next lighting recipe">${chevron}</button></div>`;
  }
  return heading + `<button type="button" class="ov-desk-open" data-companion="desk" aria-label="Open Desk Display">
    <span class="ov-desk-time"></span><span class="ov-desk-status"></span><span class="ov-desk-open-label">Open display ↗</span></button>`;
}

const text = (el, value) => { if (el.textContent !== value) el.textContent = value; };
let recipes = null;
function paintRecipe(el) {
  const item = recipes?.find(r => r.tile === Number(el.dataset.recipe));
  const button = el.querySelector(".ov-recipe-open");
  const controls = el.querySelector(".ov-recipe-controls");
  controls.hidden = !item;
  if (!item) {
    button.setAttribute("aria-label", "Open lighting lookbook");
    text(el.querySelector(".ov-recipe-detail"), el.dataset.recipeError ? "Collection unavailable" : "Loading collection…");
    return;
  }
  if (el._paintedRecipe === item) return;
  el._paintedRecipe = item;
  el.style.setProperty("--plate-x", (item.tile % 3) * 50 + "%");
  el.style.setProperty("--plate-y", Math.floor(item.tile / 3) * 100 + "%");
  el.setAttribute("data-has-art", "");
  button.setAttribute("aria-label", "Open recipe " + item.profile.name);
  text(el.querySelector(".ov-recipe-name"), item.profile.name);
  text(el.querySelector(".ov-recipe-detail"), item.motion + " · " + item.profile.lighting.brightness + "%");
  text(el.querySelector(".ov-recipe-index"), (recipes.indexOf(item) + 1) + " / " + recipes.length);
  const colors = el.querySelector(".ov-recipe-colors");
  colors.replaceChildren(...item.colors.map(color => {
    const swatch = document.createElement("i"); swatch.style.backgroundColor = color; return swatch;
  }));
  colors.setAttribute("aria-label", "Colours " + item.colors.join(", "));
}

export function updateLookbookTile(el, tile) {
  text(el.querySelector(".tile-name"), tile.label || "Lighting lookbook");
  if (!el.querySelector(".ov-recipe-colors").id) {
    const detail = el.querySelector(".ov-recipe-detail"), colors = el.querySelector(".ov-recipe-colors");
    detail.id = "ov-recipe-detail-" + tile.id; colors.id = "ov-recipe-colors-" + tile.id;
    el.querySelector(".ov-recipe-open").setAttribute("aria-describedby", detail.id + " " + colors.id);
  }
  el.dataset.recipe = String(tile.recipe ?? 0);
  paintRecipe(el);
  if (recipes || el._loadingRecipe || el.dataset.recipeError) return;
  el._loadingRecipe = true;
  lookbookCatalog().then(items => { recipes = items; }).catch(() => { el.dataset.recipeError = "true"; })
    .finally(() => { el._loadingRecipe = false; if (el.isConnected) paintRecipe(el); });
}

export function adjacentRecipe(current, direction) {
  if (!recipes?.length) return current;
  const index = recipes.findIndex(item => item.tile === (current ?? 0));
  return recipes[(index + direction + recipes.length) % recipes.length].tile;
}

let desk;
function deskPreferences() {
  if (desk) return desk;
  let face, timer, unavailable = false;
  try { face = JSON.parse(localStorage.getItem(DESK_FACE_KEY)); } catch { /* Use the system's time format. */ }
  try {
    timer = JSON.parse(localStorage.getItem(DESK_TIMER_KEY));
  } catch { unavailable = true; }
  const prefs = facePreferences(face);
  const format = { hour: "numeric", minute: "2-digit" };
  if (prefs.format !== "system") format.hourCycle = prefs.format === "24" ? "h23" : "h12";
  return desk = { clock: new Intl.DateTimeFormat(undefined, format),
    date: new Intl.DateTimeFormat(undefined, {weekday:"short", month:"short", day:"numeric"}), timer: readTimer(timer), unavailable };
}

export function deskStorageChanged(key = null) {
  if (key !== null && key !== DESK_FACE_KEY && key !== DESK_TIMER_KEY) return false;
  desk = null; return true;
}

// The ordinary Overview refresh drives the minute clock. The focus timer belongs to Desk Display.
export function updateDeskTile(el, tile) {
  const {clock, date, timer, unavailable} = deskPreferences(), now = new Date();
  const left = remaining(timer, now.getTime()), minutes = Math.ceil(left / 60000);
  const status = unavailable ? "Focus timer unavailable" : !timer ? date.format(now) : !left ? "Focus complete" :
    timer.mode === "paused" ? "Paused · " + minutes + " min left" : "Focus · " + minutes + " min left";
  text(el.querySelector(".tile-name"), tile.label || "Desk Display");
  const time = el.querySelector(".ov-desk-time"), state = el.querySelector(".ov-desk-status");
  if (!time.id) {
    time.id = "ov-desk-time-" + tile.id; state.id = "ov-desk-status-" + tile.id;
    el.querySelector(".ov-desk-open").setAttribute("aria-describedby", time.id + " " + state.id);
  }
  text(time, clock.format(now)); text(state, status);
}
