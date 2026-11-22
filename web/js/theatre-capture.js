import { saveLocalFile } from "./save-local-file.js";

const formats = { landscape: [1920, 1080], portrait: [1080, 1920], square: [1080, 1080] };
const slug = name => String(name).normalize("NFKD").replace(/[^a-z\d]+/gi, "-").replace(/^-|-$/g, "").slice(0, 40).toLowerCase() || "rig";
const clock = time => new Date(time).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit" });

function backdrop(ctx, w, h, firefly) {
  ctx.fillStyle = firefly ? "#191520" : "#121219"; ctx.fillRect(0, 0, w, h);
  const colors = firefly ? ["#4b3340", "#283b3c"] : ["#363140", "#233239"];
  [[.08, .05], [.9, .86]].forEach(([x, y], i) => {
    const gradient = ctx.createRadialGradient(x * w, y * h, 0, x * w, y * h, Math.max(w, h) * .72);
    gradient.addColorStop(0, colors[i]); gradient.addColorStop(1, colors[i] + "00");
    ctx.fillStyle = gradient; ctx.fillRect(0, 0, w, h);
  });
}

// Moments and shapes are fixed for this composition. Live stream updates can continue behind it.
export function createTheatreCapture(root, { camera, moments, shapes, focus, render, onBack }) {
  const sheet = root.closest("dialog"), panel = document.createElement("section");
  const priorTitle = sheet.getAttribute("aria-labelledby");
  panel.className = "theatre-capture";
  panel.innerHTML = `<header class="theatre-head"><div class="theatre-heading"><svg aria-hidden="true"><use href="#i-effect"/></svg><h2 id="capture-title">Frame capture</h2></div>
    <button type="button" class="btn" data-back>Back to theatre</button></header>
    <div class="capture-composition"><div class="capture-caption"><span class="capture-scope"></span><h3></h3></div>
      <div class="capture-viewport"><div class="capture-frame"><canvas aria-label="PNG composition preview" role="img"></canvas></div></div>
      <div class="capture-imprint"><span data-time></span><span data-size></span></div></div>
    <div class="capture-tools">
      <div class="capture-recent" hidden><label for="capture-scrub">Recent frames</label><input type="range" id="capture-scrub" min="0" step="1">
        <output for="capture-scrub"></output></div>
      <div class="capture-options">
        <div><label for="capture-format">Framing</label><select class="select" id="capture-format"><option value="landscape">Landscape · 16:9</option><option value="portrait">Portrait · 9:16</option><option value="square">Square · 1:1</option></select></div>
        <div><label for="capture-camera">Camera</label><select class="select" id="capture-camera"><option value="oblique">Oblique</option><option value="front">Face-on</option></select></div>
        <div><label for="capture-bg">Backdrop</label><select class="select" id="capture-bg"><option value="theatre">Theatre</option><option value="transparent">Transparent</option></select></div>
        <button type="button" class="btn primary" data-save>Save PNG…</button></div>
      <p class="capture-feedback" role="status"></p><p class="capture-live" hidden></p></div>`;
  root.append(panel); root.classList.add("in-capture"); sheet.setAttribute("aria-labelledby", "capture-title");
  const find = s => panel.querySelector(s), canvas = find("canvas"), ctx = canvas.getContext("2d");
  const format = find("#capture-format"), angle = find("#capture-camera"), bg = find("#capture-bg");
  const scrub = find("#capture-scrub"), save = find("[data-save]"), back = find("[data-back]");
  const listeners = new AbortController(), listen = (el, type, fn) => el.addEventListener(type, fn, { signal: listeners.signal });
  let open = true, saving = false;
  angle.value = camera; scrub.max = String(moments.length - 1); scrub.value = scrub.max;
  find(".capture-recent").hidden = moments.length < 2;
  const leds = shapes.reduce((n, z) => n + z.positions.length, 0);
  find(".capture-scope").textContent = `${focus} · ${shapes.length} zone${shapes.length === 1 ? "" : "s"} · ${leds} LED${leds === 1 ? "" : "s"}`;
  function paint() {
    const [w, h] = formats[format.value], moment = moments[Number(scrub.value)];
    if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
    ctx.clearRect(0, 0, w, h);
    const transparent = bg.value === "transparent";
    if (!transparent) backdrop(ctx, w, h, document.documentElement.dataset.design === "firefly");
    render(ctx, w, h, moment, angle.value);
    panel.style.setProperty("--capture-aspect", String(w / h));
    panel.classList.toggle("capture-alpha", transparent);
    find("h3").textContent = moment.name;
    const stamp = clock(moment.time), offset = (moments.at(-1).time - moment.time) / 1000;
    find("[data-time]").textContent = `Frame held at ${stamp}`;
    find("[data-size]").textContent = `${w} × ${h} px`;
    const relative = offset ? `${offset.toFixed(1)} s earlier` : "Latest";
    find("output").textContent = relative;
    scrub.style.setProperty("--fill", `${100 * Number(scrub.value) / Math.max(1, moments.length - 1)}%`);
    scrub.setAttribute("aria-valuetext", `${relative}, ${stamp}`);
    canvas.setAttribute("aria-label", `${moment.name}, ${focus}, ${w} by ${h} pixels, ${bg.selectedOptions[0].text} backdrop, ${stamp}.`);
    find(".capture-feedback").textContent = "";
  }
  function dispose() {
    if (!open) return;
    open = false; listeners.abort();
    root.classList.remove("in-capture"); panel.remove(); canvas.width = canvas.height = 1;
    if (root.isConnected && sheet.getAttribute("aria-labelledby") === "capture-title") sheet.setAttribute("aria-labelledby", priorTitle);
  }
  function leave() { if (saving) return; dispose(); onBack(); }
  for (const field of [format, angle, bg]) listen(field, "change", paint);
  listen(scrub, "input", paint);
  listen(back, "click", leave);
  listen(sheet, "cancel", e => { if (open) { e.preventDefault(); leave(); } });
  listen(save, "click", async () => {
    if (saving) return;
    saving = true;
    const controls = panel.querySelectorAll("input,select,button");
    for (const field of controls) field.disabled = true;
    const moment = moments[Number(scrub.value)], filename = `airglow-${slug(moment.name)}-${new Date(moment.time).toISOString().replace(/[:.]/g, "-")}.png`;
    const feedback = find(".capture-feedback");
    feedback.textContent = "Preparing PNG…";
    try {
      const result = await saveLocalFile({ filename, mime: "image/png", description: "PNG image",
        data: () => {
          if (!open) throw new Error("This capture was closed.");
          return new Promise((resolve, reject) => canvas.toBlob(blob => blob ? resolve(blob) : reject(new Error("The PNG could not be encoded.")), "image/png"));
        } });
      if (open) feedback.textContent = result === "cancelled" ? "Save cancelled." : result === "saved" ? "PNG saved." : "PNG download started.";
    } catch (err) { if (open) feedback.textContent = err.message || "Could not save this PNG."; }
    finally { saving = false; if (open) { for (const field of controls) field.disabled = false; save.focus({ preventScroll: true }); } }
  });
  paint(); back.focus({ preventScroll: true });
  return { get open() { return open; }, dispose,
    liveState(gate) {
      if (!open) return;
      const note = find(".capture-live");
      note.hidden = !gate;
      note.textContent = gate ? `${gate}. This preview keeps the captured frame.` : "";
    },
  };
}
