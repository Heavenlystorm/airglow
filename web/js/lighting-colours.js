import { esc, toast } from "./core.js";
import { closeSheet, openSheet } from "./sheet.js";
import { addPalette, cleanPalettes, PALETTE_KEY } from "./studio-document.js";

const RECENT_KEY = "airglow.lighting.recent-colours.v1";
const presets = [
  { name: "Firefly", colors: ["#38243d", "#ffb276", "#a4edd3"] },
  { name: "Polar night", colors: ["#10244d", "#4473bd", "#83e6cc"] },
  { name: "Ember", colors: ["#50162f", "#dd5639", "#ffc875"] },
  { name: "After rain", colors: ["#344857", "#849fb7", "#d0d3ec"] },
];

function readList(key) {
  try {
    const raw = localStorage.getItem(key);
    if (!raw || raw.length > 8192) return [];
    const list = JSON.parse(raw);
    return Array.isArray(list) ? list : [];
  } catch { return []; }
}

function recentColours() {
  return readList(RECENT_KEY).filter(c => typeof c === "string" && /^#[\da-f]{6}$/i.test(c)).slice(0, 8);
}

export function rememberColours(colors) {
  const valid = colors.filter(c => typeof c === "string" && /^#[\da-f]{6}$/i.test(c)).map(c => c.toLowerCase());
  const recent = [...new Set([...valid, ...recentColours()])].slice(0, 8);
  try { localStorage.setItem(RECENT_KEY, JSON.stringify(recent)); } catch { /* storage is optional */ }
}

function parseColours(text, min, max) {
  const words = text.trim().split(/[\s,]+/);
  if (words.length < min || words.length > max || !words.every(w => /^#?(?:[\da-f]{3}|[\da-f]{6})$/i.test(w))) return null;
  return words.map(w => {
    const hex = w.replace(/^#/, "").toLowerCase();
    return "#" + (hex.length === 3 ? [...hex].map(c => c + c).join("") : hex);
  });
}

const gradient = colors => colors.length === 1 ? colors[0] : `linear-gradient(90deg, ${colors.join(", ")})`;

// A sheet owns a draft. Saving to the palette library and cancelling never send a lighting command.
export function openColourEditor(spec, current, scope, apply) {
  const multi = spec.type === "colors";
  const min = multi ? spec.min : 1, max = multi ? spec.max : 1;
  const initial = multi ? [...current] : [current];
  const saved = () => cleanPalettes(readList(PALETTE_KEY));
  const choices = () => [...presets, ...saved()].filter(p => p.colors.length >= min && p.colors.length <= max);
  const recent = recentColours();
  const hint = multi ? `Enter ${min}–${max} hex colours separated by spaces or commas.` : "Enter a 3- or 6-digit hex colour.";
  const sheet = openSheet(`<form class="colour-editor" id="colour-editor">
    <div><h2>${multi ? "Edit palette" : "Edit colour"}</h2><p class="hint">${esc(scope)}</p></div>
    <div class="colour-preview" aria-hidden="true"></div>
    <label class="field"><span class="lab">${multi ? "Hex colours" : "Hex colour"}</span>
      <input class="text" data-hex value="${esc(initial.join(" "))}" maxlength="${max * 10}" aria-describedby="colour-format" spellcheck="false" autocomplete="off">
      <span class="hint" id="colour-format">${hint}</span></label>
    <div class="btn-row"><button type="button" class="btn" data-photo-open>From a photo…</button>${multi ? '<button type="button" class="btn quiet" data-reverse>Reverse order</button>' : ""}
      <button type="button" class="text-link" data-default>Use effect default</button></div>
    ${multi ? `<div class="field"><span class="lab">Palettes</span><div class="colour-palettes"></div></div>
      <details class="colour-save"><summary>Save for Lighting and Studio</summary>
        <div><label class="field"><span class="lab">Palette name</span><input class="text" data-name maxlength="32" autocomplete="off"></label>
        <button type="button" data-save class="btn">Save palette</button></div><p class="hint" data-saved role="status"></p></details>`
      : recent.length ? `<div class="field"><span class="lab">Recent colours</span><div class="colour-recents">${recent.map(c =>
        `<button type="button" data-recent="${c}" style="--colour:${c}" aria-label="Use ${c}" title="${c}"></button>`).join("")}</div></div>` : ""}
    <p class="err" data-error role="alert" hidden></p>
    <div class="actions"><span class="spacer"></span><button type="button" class="btn quiet" data-close>Cancel</button>
      <button class="btn primary" data-apply>${multi ? "Apply palette" : "Apply colour"}</button></div>
    </form>`);
  const form = sheet.querySelector("#colour-editor");
  const find = selector => form.querySelector(selector);
  const input = find("[data-hex]"), preview = find(".colour-preview"), error = find("[data-error]");
  let busy = false;
  function showError(message) {
    error.textContent = message;
    error.hidden = !message;
  }
  function draft() { return parseColours(input.value, min, max); }
  function paint() {
    const colors = draft();
    preview.style.background = colors ? gradient(colors) : "";
    input.setAttribute("aria-invalid", String(!colors));
    showError(colors ? "" : hint);
    find("[data-apply]").disabled = busy || !colors || colors.join() === initial.join();
    if (multi) {
      find("[data-reverse]").disabled = !colors;
      find("[data-save]").disabled = !colors || !find("[data-name]").value.trim();
      find("[data-saved]").textContent = "";
    }
    for (const button of form.querySelectorAll("[data-colours], [data-recent]")) {
      button.setAttribute("aria-pressed", String((button.dataset.colours || button.dataset.recent) === colors?.join(" ")));
    }
  }
  function choose(colors) {
    input.value = colors.join(" ");
    paint();
  }
  function paintPalettes() {
    find(".colour-palettes").innerHTML = choices().map(p =>
      `<button type="button" class="btn colour-palette" data-colours="${p.colors.join(" ")}">
        <i aria-hidden="true" style="background:${gradient(p.colors)}"></i><span>${esc(p.name)}</span></button>`).join("");
  }
  if (multi) paintPalettes();
  paint();
  let photo = null;
  function saveNamedPalette(value, colors) {
    const list = addPalette(saved(), value, colors);
    localStorage.setItem(PALETTE_KEY, JSON.stringify(list));
    if (multi) paintPalettes();
    return list[0].name;
  }
  const photoButton = find("[data-photo-open]");
  photoButton.addEventListener("click", async () => {
    if (photoButton.hasAttribute("aria-busy")) return;
    photoButton.setAttribute("aria-busy", "true");
    try {
      const { createPhotoWorkbench } = await import("./photo-palette.js");
      if (!ownsSheet() || busy) return;
      photo ||= createPhotoWorkbench(form, { single: !multi, scope, min: multi ? min : 2, max: multi ? max : 4,
        count: multi ? (draft() || initial).length : 3, save: saveNamedPalette,
        use(colors) { choose(colors); (find("[data-apply]").disabled ? input : find("[data-apply]")).focus(); } });
      photo.open();
    } catch (err) { if (ownsSheet()) showError(err.message); }
    finally { photoButton.removeAttribute("aria-busy"); }
  });
  input.addEventListener("input", paint);
  find("[data-default]").addEventListener("click", () => choose(multi ? spec.default : [spec.default]));
  find("[data-reverse]")?.addEventListener("click", () => choose(draft().reverse()));
  form.addEventListener("click", event => {
    const button = event.target.closest("[data-colours], [data-recent]");
    if (button) choose((button.dataset.colours || button.dataset.recent).split(" "));
  });
  find("[data-name]")?.addEventListener("input", paint);
  function savePalette() {
    try {
      const name = saveNamedPalette(find("[data-name]").value, draft());
      paint(); find("[data-saved]").textContent = `Saved ${name}.`;
    } catch (err) { showError(err.message); }
  }
  find("[data-save]")?.addEventListener("click", savePalette);
  find("[data-name]")?.addEventListener("keydown", e => {
    if (e.key === "Enter") { e.preventDefault(); if (!find("[data-save]").disabled) savePalette(); }
  });
  const ownsSheet = () => sheet.open && form.parentElement === sheet;
  const preventCancel = event => { if (ownsSheet()) event.preventDefault(); };
  async function applyDraft(colors) {
    if (busy) return;
    busy = true;
    if (ownsSheet()) {
      for (const control of form.querySelectorAll("input, button")) control.disabled = true;
      form.setAttribute("aria-busy", "true");
      find("[data-apply]").textContent = "Applying…";
      sheet.addEventListener("cancel", preventCancel);
    }
    try {
      await apply(multi ? colors : colors[0]);
      rememberColours(colors);
      if (ownsSheet()) closeSheet();
      else toast(multi ? "Palette applied" : "Colour applied");
    } catch (err) {
      if (ownsSheet()) {
        for (const control of form.querySelectorAll("input, button")) control.disabled = false;
        form.removeAttribute("aria-busy");
        find("[data-apply]").textContent = multi ? "Apply palette" : "Apply colour";
        busy = false;
        paint(); showError(err.message);
        find("[data-apply]").focus();
      } else {
        toast(`${scope}: ${err.message}`, { isError: true, action: { label: "Retry", onClick: () => applyDraft(colors) } });
      }
    } finally {
      busy = false;
      sheet.removeEventListener("cancel", preventCancel);
    }
  }
  form.addEventListener("submit", event => {
    event.preventDefault();
    if (form.dataset.photo === "open") return;
    const colors = draft();
    if (colors && colors.join() !== initial.join()) void applyDraft(colors);
  });
  input.focus(); input.select();
}
