import { esc } from "./core.js";
import { extractPhotoStops, photoHistogram, pixelAt } from "./photo-sampling.js";

const SAMPLE = new URL("../art/firefly-dusk.webp", import.meta.url);
const types = ["image/jpeg", "image/png", "image/webp"];
const maxSide = 1120;

// A workbench is scoped to one colour editor. Its file and pixel buffer never leave the WebView.
export function createPhotoWorkbench(form, { single, min, max, count, scope, use, save }) {
  const sheet = form.closest("dialog"), opener = form.querySelector("[data-photo-open]");
  const panel = document.createElement("section");
  panel.className = "photo-palette"; panel.hidden = true;
  panel.innerHTML = `<header class="photo-heading"><div><h2 id="photo-title">Photo palette</h2>
    <p class="hint">${esc(scope)}</p></div><button class="btn quiet" type="button" data-back>Back to editor</button></header>
    <div class="photo-source"><span class="photo-filename"></span><div class="btn-row">
    <button type="button" class="text-link" data-sample hidden>Use sample image</button>
    <button type="button" class="btn" data-choose>Choose photo…</button>
    <input type="file" accept=".jpg,.jpeg,.png,.webp,image/jpeg,image/png,image/webp" aria-label="Choose a local photo" hidden></div></div>
    <div class="photo-viewport"><button type="button" class="photo-image" aria-label="Pick a colour from the photo"
        aria-describedby="photo-sampling-help" hidden><canvas aria-hidden="true"></canvas><span class="photo-pins" aria-hidden="true"></span>
        <span class="photo-cursor" aria-hidden="true" hidden></span></button>
      <div class="photo-placeholder">Choose or drop a photo</div></div>
    <p class="hint photo-guide" id="photo-sampling-help">Select a stop, then click the photo to sample it. With the photo focused, use the arrows to move and Enter to pick.</p>
    <div class="photo-settings"><div><label for="photo-count">Colours</label> <select class="select" id="photo-count" data-count>${Array.from({ length: max - min + 1 }, (_, i) =>
      `<option value="${min + i}"${min + i === count ? " selected" : ""}>${min + i}</option>`).join("")}</select></div>
      <div><label for="photo-emphasis">Emphasis</label> <select class="select" id="photo-emphasis" data-emphasis><option value="balanced">Balanced</option><option value="highlights">Highlights</option><option value="vivid">Vivid colours</option></select></div>
      <button class="text-link" type="button" data-extract>Extract again</button></div>
    <div class="photo-stops" role="radiogroup" aria-label="Palette stops"></div>
    <div class="photo-tools"><div class="photo-shift"><button class="btn quiet" type="button" data-move="-1" aria-label="Move selected stop earlier" title="Move selected stop earlier">← Earlier</button>
      <button class="btn quiet" type="button" data-move="1" aria-label="Move selected stop later" title="Move selected stop later">Later →</button></div>
      <button type="button" class="text-link" data-save-toggle aria-expanded="false" aria-controls="photo-library">Save to library…</button></div>
    <div class="photo-library" id="photo-library" hidden><label>Palette name<input class="text" data-name maxlength="32" autocomplete="off"></label>
      <button class="btn" type="button" data-save>Save palette</button></div>
    <p class="photo-message" role="status"></p>
    <footer class="photo-actions"><span class="hint">JPEG, PNG or WebP · Images stay on this PC</span>
      <button class="btn primary" type="button" data-use>${single ? "Use selected colour in draft" : "Use palette in draft"}</button></footer>`;
  form.append(panel);
  const find = selector => panel.querySelector(selector);
  const imageButton = find(".photo-image"), canvas = find("canvas"), pins = find(".photo-pins"), cursor = find(".photo-cursor");
  const library = find("#photo-library"), name = find("[data-name]"), message = find(".photo-message");
  const countBox = find("[data-count]"), emphasis = find("[data-emphasis]"), stopsBox = find(".photo-stops");
  const fileInput = find("[type=file]"), useButton = find("[data-use]"), back = find("[data-back]");
  const children = new AbortController();
  const listen = (el, type, run) => el.addEventListener(type, run, { signal: children.signal });
  let image = null, bins = [], stops = [], selected = 0, point = null, serial = 0, loading = false, disposed = false, editorScroll = 0;
  const opened = () => !disposed && !panel.hidden && sheet.open && form.isConnected;
  const pointStyle = p => `left:${(p.x + .5) / image.width * 100}%;top:${(p.y + .5) / image.height * 100}%`;
  function note(text, error = false) {
    message.textContent = text;
    message.classList.toggle("err", error);
  }
  function controls() {
    const ready = !!image && !loading;
    countBox.disabled = !ready || max === min;
    for (const el of [emphasis, imageButton, useButton, find("[data-save-toggle]"), find("[data-extract]")]) el.disabled = !ready;
    find("[data-save]").disabled = !ready || !name.value.trim();
    for (const move of panel.querySelectorAll("[data-move]")) {
      const next = selected + Number(move.dataset.move);
      move.disabled = !ready || next < 0 || next >= stops.length;
    }
    stopsBox.querySelectorAll("button").forEach(b => { b.disabled = !ready; });
  }
  function showCursor() {
    cursor.hidden = !point || document.activeElement !== imageButton;
    if (point) cursor.style.cssText = pointStyle(point);
  }
  function paint() {
    if (stopsBox.childElementCount !== stops.length) {
      stopsBox.innerHTML = stops.map((_, i) => `<button type="button" role="radio" data-stop-index="${i}"><i aria-hidden="true"></i><span></span></button>`).join("");
    }
    stops.forEach((stop, i) => {
      const button = stopsBox.children[i];
      button.style.setProperty("--photo-stop", stop.color);
      button.querySelector("span").textContent = stop.color.toUpperCase();
      button.tabIndex = i === selected ? 0 : -1;
      button.setAttribute("aria-checked", String(i === selected));
      button.setAttribute("aria-label", `Stop ${i + 1}, ${stop.color}`);
    });
    pins.innerHTML = stops.map((p, i) => `<i style="${pointStyle(p)};--photo-stop:${p.color}"${i === selected ? " data-selected" : ""}></i>`).join("");
    showCursor();
    controls();
  }
  function extract() {
    if (!image) return;
    stops = extractPhotoStops(bins, Number(countBox.value), emphasis.value);
    selected = Math.max(0, Math.min(selected, stops.length - 1));
    point = stops[selected]; paint();
    const distinct = new Set(stops.map(s => s.color)).size;
    note(distinct < stops.length ? `Found ${distinct} distinct colour${distinct === 1 ? "" : "s"}. Repeated stops keep the palette true to the photo.` : "");
  }
  function select(index, focus = false) {
    selected = index; point = stops[index]; paint(); note("");
    if (focus) stopsBox.children[index].focus();
  }

  async function loadPhoto(file) {
    const request = ++serial, isSample = !file;
    const current = () => opened() && request === serial;
    loading = true; controls();
    note(isSample ? "Opening the built-in sample…" : "Reading photo…");
    let bitmap;
    try {
      if (file && (file.type ? !types.includes(file.type) : !/\.(?:jpe?g|png|webp)$/i.test(file.name))) {
        throw new Error("Choose a JPEG, PNG or WebP image.");
      }
      let blob = file;
      if (isSample) {
        const result = await fetch(SAMPLE, { signal: children.signal });
        if (!result.ok) throw new Error("The built-in sample could not be opened. Choose a photo from this PC.");
        blob = await result.blob();
      }
      if (!current()) return;
      bitmap = await createImageBitmap(blob);
      if (!current()) return;
      const ratio = Math.min(1, maxSide / Math.max(bitmap.width, bitmap.height));
      const buffer = document.createElement("canvas");
      buffer.width = Math.max(1, Math.round(bitmap.width * ratio)); buffer.height = Math.max(1, Math.round(bitmap.height * ratio));
      const ctx = buffer.getContext("2d", { willReadFrequently: true });
      if (!ctx) throw new Error("Image sampling is unavailable in this window.");
      ctx.drawImage(bitmap, 0, 0, buffer.width, buffer.height);
      const sampled = ctx.getImageData(0, 0, buffer.width, buffer.height), candidates = photoHistogram(sampled);
      if (!candidates.length) throw new Error("This image is transparent. Choose a photo with visible colours.");
      image = sampled; bins = candidates; selected = 0;
      canvas.width = sampled.width; canvas.height = sampled.height;
      canvas.getContext("2d").drawImage(buffer, 0, 0);
      imageButton.style.setProperty("--photo-ratio", String(sampled.width / sampled.height));
      imageButton.hidden = false; find(".photo-placeholder").hidden = true;
      find(".photo-filename").textContent = isSample ? "Firefly dusk · Built-in sample" : file.name;
      name.value = (isSample ? "Firefly dusk" : file.name.replace(/\.[^.]+$/, "")).slice(0, 32);
      find("[data-sample]").hidden = isSample;
      extract();
    } catch (err) {
      if (current()) note(err.name === "InvalidStateError" || err.name === "EncodingError" ?
        "This image could not be decoded. Try a different JPEG, PNG or WebP." : (err.message || "Could not open this photo."), true);
    } finally {
      bitmap?.close();
      if (current()) { loading = false; controls(); }
    }
  }

  function leave() {
    serial++; loading = false;
    panel.hidden = true; delete form.dataset.photo;
    panel.classList.remove("drop-ready");
    sheet.setAttribute("aria-labelledby", "sheet-title");
    sheet.scrollTop = editorScroll;
    opener.focus({ preventScroll: true });
  }
  function dispose() {
    if (disposed) return;
    disposed = true; serial++;
    children.abort(); replacement.disconnect();
    image = null; bins = []; stops = []; point = null; fileInput.value = "";
    canvas.width = canvas.height = 1;
    panel.remove();
  }
  const replacement = new MutationObserver(() => { if (!form.isConnected) dispose(); });
  replacement.observe(sheet, { childList: true });
  listen(sheet, "close", () => { if (!sheet.open || !form.isConnected) dispose(); });
  listen(sheet, "cancel", e => { if (opened()) { e.preventDefault(); leave(); } });
  listen(back, "click", leave);
  listen(find("[data-choose]"), "click", () => fileInput.click());
  listen(fileInput, "change", () => { const file = fileInput.files[0]; fileInput.value = ""; if (file) void loadPhoto(file); });
  listen(find("[data-sample]"), "click", () => { void loadPhoto(); });
  const hasFiles = e => e.dataTransfer?.types.includes("Files");
  listen(panel, "dragover", e => { if (hasFiles(e)) { e.preventDefault(); e.dataTransfer.dropEffect = "copy"; panel.classList.add("drop-ready"); } });
  listen(panel, "dragleave", e => { if (!panel.contains(e.relatedTarget)) panel.classList.remove("drop-ready"); });
  listen(panel, "drop", e => {
    e.preventDefault(); panel.classList.remove("drop-ready");
    const file = e.dataTransfer?.files[0]; if (file) void loadPhoto(file);
  });
  listen(countBox, "change", extract);
  listen(emphasis, "change", extract);
  listen(find("[data-extract]"), "click", extract);
  listen(stopsBox, "click", e => {
    const b = e.target.closest("[data-stop-index]"); if (b) select(Number(b.dataset.stopIndex));
  });
  listen(stopsBox, "keydown", e => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key) || !stops.length) return;
    e.preventDefault();
    const index = e.key === "Home" ? 0 : e.key === "End" ? stops.length - 1 :
      (selected + (e.key === "ArrowLeft" ? -1 : 1) + stops.length) % stops.length;
    select(index, true);
  });
  for (const move of panel.querySelectorAll("[data-move]")) listen(move, "click", () => {
    const index = selected + Number(move.dataset.move);
    if (index < 0 || index >= stops.length) return;
    [stops[selected], stops[index]] = [stops[index], stops[selected]];
    select(index, true);
    note(`Moved ${stops[index].color} to stop ${index + 1}.`);
  });
  function pick() {
    if (!point || !image) return;
    const sampled = pixelAt(image, point.x, point.y);
    if (!sampled) { note("That point is transparent. Choose a visible part of the photo."); return; }
    stops[selected] = sampled; paint(); note(`Stop ${selected + 1} now uses ${sampled.color}.`);
  }
  listen(imageButton, "click", e => {
    if (e.detail) {
      const box = canvas.getBoundingClientRect();
      point = { x: Math.min(image.width - 1, Math.max(0, Math.floor((e.clientX - box.left) / box.width * image.width))),
        y: Math.min(image.height - 1, Math.max(0, Math.floor((e.clientY - box.top) / box.height * image.height))) };
    }
    pick(); showCursor();
  });
  listen(imageButton, "focus", showCursor);
  listen(imageButton, "blur", showCursor);
  listen(imageButton, "keydown", e => {
    const move = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] }[e.key];
    if (!move || !point) return;
    e.preventDefault();
    const step = e.shiftKey ? 10 : 1;
    point = { x: Math.min(image.width - 1, Math.max(0, point.x + move[0] * step)),
      y: Math.min(image.height - 1, Math.max(0, point.y + move[1] * step)) };
    showCursor();
    note(`Pixel ${point.x + 1}, ${point.y + 1} · ${pixelAt(image, point.x, point.y)?.color || "transparent"}`);
  });
  const saveToggle = find("[data-save-toggle]");
  listen(saveToggle, "click", () => {
    library.hidden = !library.hidden; saveToggle.setAttribute("aria-expanded", String(!library.hidden));
    if (!library.hidden) { name.focus(); name.select(); }
  });
  function savePhotoPalette() {
    try { const saved = save(name.value, stops.map(s => s.color)); note(`Saved ${saved} to Lighting and Studio.`); }
    catch (err) { note(err.message, true); }
  }
  listen(name, "input", controls);
  listen(name, "keydown", e => { if (e.key === "Enter") { e.preventDefault(); if (!find("[data-save]").disabled) savePhotoPalette(); } });
  listen(find("[data-save]"), "click", savePhotoPalette);
  listen(useButton, "click", () => {
    const colors = single ? [stops[selected].color] : stops.map(s => s.color);
    leave(); use(colors);
  });
  return {
    open() {
      editorScroll = sheet.scrollTop;
      panel.hidden = false; form.dataset.photo = "open";
      sheet.setAttribute("aria-labelledby", "photo-title");
      sheet.scrollTop = 0;
      find("[data-choose]").focus({ preventScroll: true });
      if (!image) void loadPhoto();
      else { controls(); note(""); }
    },
  };
}
