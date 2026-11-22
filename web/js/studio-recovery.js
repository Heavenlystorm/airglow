// Browser-local Studio workspace. The caller owns the preview renderer and document lifecycle.
import { esc } from "./core.js";
import { openSheet } from "./sheet.js";
import { compareStudioDocuments } from "./studio-document.js";

const dates = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
const when = at => `<time datetime="${new Date(at).toISOString()}">${esc(dates.format(at))}</time>`;
const titleOf = doc => doc.name.trim() || "Untitled effect";
const medium = doc => doc.mode === "code" ? "Code" : `${doc.layers.length} ${doc.layers.length === 1 ? "layer" : "layers"}`;
function swatch(doc) {
  const colors = doc.mode === "layers" ? doc.master?.palette?.filter(c => /^#[a-f0-9]{6}$/i.test(c)) : [];
  return `<span aria-hidden="true" class="st-recovery-swatch${colors?.length ? "" : " is-code"}">${colors?.length ? colors.map(c => `<i style="background:${c}"></i>`).join("") : "&lt;/&gt;"}</span>`;
}

export function draftShelf(drafts, temporary, hasCheckpoints) {
  if (!drafts.length && !temporary && !hasCheckpoints) return "";
  return `<section class="st-recovery" id="st-draft" aria-labelledby="st-recovery-title">
    <div class="st-library-head"><h2 class="st-h" id="st-recovery-title">Your workspace</h2>
      ${hasCheckpoints ? '<button class="btn quiet" id="st-home-checkpoints" data-st="checkpoints">Browse checkpoints</button>' : ""}</div>
    ${temporary ? '<p class="st-workflow-error" role="status">Local draft storage is unavailable. Recent changes are kept in this window. Save your work before closing Airglow.</p>' : ""}
    ${drafts.length ? `<ul class="st-drafts">${drafts.map(({ key, doc, at }) => `<li>
      <button class="st-draft-resume" data-draft="resume" data-key="${esc(key)}" aria-label="Resume ${esc(titleOf(doc))}">
        ${swatch(doc)}<span><strong>${esc(titleOf(doc))}</strong><small>${medium(doc)} · ${when(at)}</small></span>
        <span class="st-resume-arrow" aria-hidden="true">↗</span></button>
      <button class="icon-btn" data-draft="discard" data-key="${esc(key)}" aria-label="Discard draft ${esc(titleOf(doc))}"><svg><use href="#i-trash"/></svg></button>
      </li>`).join("")}</ul><p class="hint">Unsaved drafts stay here for seven days after their last edit. Named checkpoints stay until you delete them.</p>` :
    '<p class="hint">Open a checkpoint to start a working copy.</p>'}</section>`;
}

export function openCheckpoints({ doc, storage, sample, renderFrame, restore }) {
  let selected, field = "", alert = "", cancelFrame = null, view = null, pending = false, request = 0;
  const cleanFrames = () => { request++; cancelFrame?.(); cancelFrame = null; };
  const release = () => { cleanFrames(); document.removeEventListener("visibilitychange", visibility); };
  function visibility() {
    if (!view?.isConnected || !view.closest("dialog")?.open) return release();
    if (document.hidden) cleanFrames(); else paintFrames();
  }
  function paintFrames() {
    cleanFrames();
    if (!selected || document.hidden) return;
    const generation = request;
    const entries = [[view.querySelector("[data-snapshot]"), selected.doc], [view.querySelector("[data-working]"), doc]];
    const draw = async () => {
      for (const [canvas, document] of entries) {
        if (!canvas || !document || request !== generation || !canvas.isConnected) return;
        const job = renderFrame(canvas, document);
        cancelFrame = job.cancel;
        await job.done;
      }
      if (request === generation) cancelFrame = null;
    };
    void draw();
  }
  function render(id, focus) {
    cleanFrames();
    let rows = [], loadError = false;
    try { rows = storage.list(); }
    catch { loadError = true; alert = "Local checkpoints could not be read. Check local storage and reopen this panel."; }
    selected = rows.find(r => r.id === id) || rows[0];
    const delta = selected && doc ? compareStudioDocuments(selected.doc, doc) : [];
    const preview = (attr, caption) => `<figure><figcaption>${caption}</figcaption><div class="st-version-art"><canvas ${attr} aria-label="${caption} lighting preview"></canvas><span>Rendering preview…</span></div></figure>`;
    const dialog = openSheet(`<div class="body st-checkpoints"><div class="st-revision-heading"><div><h2>Checkpoints</h2>
      <p class="hint">Keep a version on this device and return to it as a new working copy.</p></div><button class="btn quiet" data-close>Close</button></div>
      ${doc ? `<form class="st-checkpoint-form"><label class="lab" for="st-checkpoint-name">Name a checkpoint of ${esc(titleOf(doc))}</label>
      <div><input class="text" id="st-checkpoint-name" maxlength="60" required placeholder="e.g. Before adding motion" value="${esc(field)}"><button class="btn" type="submit" ${loadError ? "disabled" : ""}>Keep checkpoint</button></div></form>` : ""}
      <p class="st-workflow-error" role="alert" ${alert ? "" : "hidden"}>${esc(alert)}</p>
      ${selected ? `<div class="st-version-browser"><nav aria-label="Saved checkpoints"><ul>${rows.map(row => `<li><button type="button" data-pick="${esc(row.id)}" aria-pressed="${selected.id === row.id}">
      ${swatch(row.doc)}<span><strong>${esc(row.name)}</strong><small>${esc(titleOf(row.doc))} · ${when(row.at)}</small></span></button></li>`).join("")}</ul></nav>
      <section aria-label="Selected checkpoint"><div class="st-version-title"><h3>${esc(selected.name)}</h3><p class="hint">${esc(titleOf(selected.doc))} · ${medium(selected.doc)} · ${when(selected.at)}</p></div>
      <div class="st-version-frames">${preview("data-snapshot", "Checkpoint")}${doc ? preview("data-working", "Current edit") : ""}</div>
      <p class="hint">${sample ? "Sample rig" : "Your rig"} · Still at 2 seconds with test audio</p>
      ${doc ? `<h4>Compared with current edit</h4>${delta.length ? `<ul class="st-version-delta">${delta.map(d => `<li>${esc(d)}</li>`).join("")}</ul>` : '<p class="hint">This version matches the current edit.</p>'}` : ""}
      <div class="btn-row"><button class="btn primary" data-restore>Open as a copy</button><button class="btn quiet" data-forget>Delete checkpoint</button></div></section></div>`
      : loadError ? "" : '<p class="hint st-checkpoints-empty">No checkpoints yet. Keep a version before trying another direction.</p>'}</div>`);
    view = dialog.querySelector(".st-checkpoints");
    const input = view.querySelector("input");
    input?.addEventListener("input", () => { field = input.value; });
    view.querySelector("form")?.addEventListener("submit", e => {
      e.preventDefault(); if (pending) return;
      try { const row = storage.capture(doc, field); field = ""; alert = ""; render(row.id, "pick"); }
      catch (error) { showError(error); input.focus(); }
    });
    view.querySelector("nav")?.addEventListener("click", e => {
      const pick = e.target.closest("[data-pick]");
      if (pick && !pending) { alert = ""; render(pick.dataset.pick, "pick"); }
    });
    view.querySelector("[data-forget]")?.addEventListener("click", () => {
      if (pending || !confirm(`Delete checkpoint “${selected.name}”? Your working drafts are kept.`)) return;
      try { storage.remove(selected.id); alert = ""; render(null, "pick"); }
      catch (error) { showError(error); }
    });
    view.querySelector("[data-restore]")?.addEventListener("click", async e => {
      if (pending) return;
      pending = true;
      e.currentTarget.disabled = true;
      try { await restore(selected.doc, () => dialog.open && view.isConnected); }
      catch (error) { if (view.isConnected && dialog.open) showError(error); }
      finally { pending = false; const b = view.querySelector("[data-restore]"); if (b) b.disabled = false; }
    });
    if (focus) (view.querySelector('[data-pick][aria-pressed="true"]') || input || view.querySelector("[data-close]"))?.focus();
    paintFrames();
    return dialog;
  }
  function showError(error) {
    const node = view.querySelector("[role=alert]");
    const storageError = ["QuotaExceededError", "SecurityError"].includes(error.name);
    node.textContent = storageError ? "Local storage is unavailable or full. Your current edits are still open." : error.message;
    node.hidden = false;
  }
  const dialog = render();
  document.addEventListener("visibilitychange", visibility);
  dialog.addEventListener("close", release, { once: true });
}
