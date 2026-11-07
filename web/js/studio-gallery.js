// Community browsing and publishing. The editor supplies only explicit document/open/save callbacks.
import { $, $$, S, emit, esc, fail, get, post, setHTML, toast } from "./core.js";
import { closeSheet, openSheet } from "./sheet.js";
const LICENSES = ["MIT", "Apache-2.0", "BSD-3-Clause", "MPL-2.0", "GPL-3.0-or-later"];
export function createStudioGallery({ H, E, box, remember, save, queueStills, addStill, effectRunnable }) {
let sheetGallery = null, generation = 0;
const isOpen = () => H.view === "community" && !!box() && !box().closest(".view")?.hidden;
function deactivate() {
  generation++;
  H.io?.disconnect(); H.io = null;
  clearTimeout(H.t);
}
async function renderCommunity() {
  deactivate();
  const requestGeneration = generation;
  setHTML(box(), `<div class="st-home"><div class="page-head"><div><h1>Community effects</h1>
    <p class="lede">Scenes made in Studio. Preview a scene on your rig, then make it your own.</p></div>
    <div class="btn-row"><button class="btn" data-st="learn">Learn</button><a class="btn" href="#studio">Back to the Studio</a></div></div>
    <p class="hint">Loading the gallery from GitHub…</p></div>`);
  try {
    const result = await get("/api/studio/gallery");
    if (requestGeneration !== generation || !isOpen()) return;
    H.gallery = result;
    for (const r of H.gallery.revoked_now || []) toast(`${r.name} was taken down from the gallery and is off now${r.reason ? `: ${r.reason}` : ""}.`, true);
  } catch (e) {
    if (requestGeneration !== generation || !isOpen()) return;
    setHTML(box(), `<div class="st-home"><div class="page-head"><div><h1>Community effects</h1>
      <p class="lede">The gallery didn't load: ${esc(e.message)}</p></div><div class="btn-row"><a class="btn" href="#studio">Back</a>
      <button class="btn primary" data-st="community-retry">Try again</button></div></div></div>`);
    return;
  }
  paintCommunity();
}

function paintCommunity() {
  if (!isOpen()) return;
  const g = H.gallery;
  const q = H.search.toLowerCase();
  const tags = [...new Set(g.effects.flatMap((e) => e.tags))].sort();
  const items = g.effects.filter((e) => (!H.tag || (H.tag === "music" ? e.audio : e.tags.includes(H.tag)))
    && (!q || `${e.name} ${e.author} ${e.description} ${e.tags.join(" ")}`.toLowerCase().includes(q)));
  setHTML(box(), `<div class="st-home"><div class="page-head"><div><h1>Community effects</h1>
    <p class="lede">${g.effects.length} shared scenes. Hover, focus or press Play preview to see one on your rig.</p></div>
    <div class="btn-row"><button class="btn" data-st="learn">Learn</button><a class="btn" href="#studio">Back to the Studio</a></div></div>
    <div class="st-filter"><input class="text" id="st-search" placeholder="Search names, authors, tags" value="${esc(H.search)}" spellcheck="false">
      <div class="chips"><button class="chip-btn" data-tag="" aria-pressed="${!H.tag}">All</button><button class="chip-btn" data-tag="music" aria-pressed="${H.tag === "music"}">Music</button>
      ${tags.map((t) => `<button class="chip-btn" data-tag="${esc(t)}" aria-pressed="${H.tag === t}">${esc(t)}</button>`).join("")}</div></div>
    ${items.length ? `<div class="st-grid">${items.map((e) => `<div class="st-card" data-card="${esc(e.id)}"><canvas class="st-live" aria-hidden="true"></canvas>
      <div class="st-cap"><strong>${esc(e.name)}${e.featured ? ` <span class="tag new">Featured</span>` : ""}</strong><span>by ${esc(e.author)}${e.audio ? ` <i class="st-badge">Music</i>` : ""}</span>
      ${e.description ? `<p class="st-desc">${esc(e.description)}</p>` : ""}</div>
      <div class="st-acts"><button class="btn quiet st-scene-play" data-preview="${esc(e.id)}" aria-pressed="false" aria-label="Play preview of ${esc(e.name)}">Play preview</button>${e.installed && !e.update ? `<button class="btn" disabled>Installed</button>` : `<button class="btn primary" data-install="${esc(e.id)}">${e.update ? "Update" : "Install"}</button>`}
        <button class="btn" data-gremix="${esc(e.id)}">Remix</button>
        <button class="btn quiet" data-gdet="${esc(e.id)}">How it's made</button></div></div>`).join("")}</div>`
      : `<p class="hint">${g.effects.length ? "Nothing matches." : "No shared effects yet. Make one and press Share: yours could be the first."}</p>`}
  </div>`);
  $("#st-search")?.addEventListener("input", (e) => {
    H.search = e.target.value; clearTimeout(H.t);
    const start = e.target.selectionStart, end = e.target.selectionEnd;
    H.t = setTimeout(() => {
      if (H.view !== "community") return;
      const focused = document.activeElement?.id === "st-search";
      paintCommunity();
      if (focused) { const input = $("#st-search"); input.focus(); input.setSelectionRange(start, end); }
    }, 250);
  });
  // stills need each effect's file: fetched as cards come into view
  queueStills([]);
  H.io?.disconnect();                 // every repaint (a search keystroke) used to leave an observer watching old cards
  const io = (H.io = new IntersectionObserver(async (entries) => {
    for (const en of entries) {
      if (!isOpen() || H.io !== io) return;
      if (!en.isIntersecting) continue;
      io.unobserve(en.target);
      const id = en.target.dataset.card;
      try {
        const { effect } = await get(`/api/studio/gallery/effect?id=${encodeURIComponent(id)}`);
        if (!isOpen() || H.io !== io || !en.target.isConnected) continue;
        addStill({ key: id, eff: effectRunnable(effect) });
      } catch { /* the card stays dark */ }
    }
  }));
  $$(".st-card", box()).forEach((c) => io.observe(c));
}

async function shareIt(fxId) {
  let eff = null;
  if (E.doc && !fxId) {
    const d = E.doc;
    if (d.dirty || !d.fx_id) { eff = await save(); if (!eff || E.doc !== d) return; }
    fxId = eff?.fx_id || d.fx_id;
    if (!fxId) return;
  }
  const e = (H.lib?.effects || []).find((x) => x.fx_id === fxId) || E.doc || {};
  const desc = e.description || "";
  openSheet(`<form class="body" id="sh-form"><h2>Share ${esc(e.name)}</h2>
    <p class="lede" style="margin:0">GitHub opens in your browser with the effect filled in. Press <b>Propose changes</b> there and it goes to the community gallery. Once it's checked and approved, everyone can use it.</p>
    <div class="field"><label class="lab" for="sh-login"><span>Your GitHub user name</span></label><input class="text" id="sh-login" required value="${esc(remember("airglow.github"))}" spellcheck="false" autocomplete="username"></div>
    <div class="field"><label class="lab" for="sh-d"><span>Description</span></label><textarea class="text" id="sh-d" rows="2" maxlength="200" required placeholder="What does it look like?">${esc(desc)}</textarea></div>
    <div class="field"><label class="lab" for="sh-tags"><span>Tags</span></label><input class="text" id="sh-tags" value="${esc((e.tags || []).join(", "))}" placeholder="ambient, warm, music" spellcheck="false"></div>
    <div class="field"><label class="lab" for="sh-lic"><span>Licence</span></label><select class="select" id="sh-lic">${LICENSES.slice(0, 5).map((l) => `<option${l === "MIT" ? " selected" : ""}>${l}</option>`).join("")}</select></div>
    <label class="check"><input type="checkbox" id="sh-ok" required> I made this (or remixed something open), and it may be shared under this licence.</label>
    <p class="hint">Shared: the effect itself, your GitHub name and the text above. Nothing about your PC.</p>
    <div class="actions"><button type="button" class="btn quiet" data-close>Cancel</button><span class="spacer"></span><button class="btn primary">Open GitHub</button></div></form>`);
  $("#sh-form").addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const login = $("#sh-login").value.trim().replace(/^@/, "");
    remember("airglow.github", login);
    try {
      const r = await post("/api/studio/share", { fx_id: fxId, author: login, description: $("#sh-d").value, open: false,
        tags: $("#sh-tags").value.split(",").map((t) => t.trim().toLowerCase()).filter(Boolean).slice(0, 5), license: $("#sh-lic").value });
      let copied = false;
      if (!r.filled) { try { await navigator.clipboard.writeText(r.source); copied = true; } catch { copied = false; } }
      await post("/api/studio/open-share", { url: r.url });
      openSheet(`<div class="body"><h2>Almost there</h2>
        ${r.filled ? `<p class="lede" style="margin:0">GitHub is open in your browser with <span class="path">${esc(r.path)}</span> filled in. Press <b>Propose changes</b>, then <b>Create pull request</b>.</p>`
          : `<p class="lede" style="margin:0">GitHub is open in your browser with the file name filled in. ${copied ? "The effect is on your clipboard:" : "Copy the effect below,"} click into the empty file there, press <b>Ctrl+V</b>, then <b>Propose changes</b> and <b>Create pull request</b>.</p>
            ${copied ? "" : `<textarea class="text" rows="6" readonly>${esc(r.source)}</textarea>`}`}
        ${r.flags.length ? `<p class="hint">The reviewer will ask about this: it ${esc(r.flags.join("; "))}.</p>` : ""}
        <p class="hint">Didn't open? <a href="${esc(r.url)}" target="_blank" rel="noopener">Open it here</a>.</p>
        <div class="actions"><span class="spacer"></span><button class="btn primary" data-close>Done</button></div></div>`);
    } catch (err) { fail(err); }
  });
}

async function galleryDetail(id) {
  try {
    const { effect } = await get(`/api/studio/gallery/effect?id=${encodeURIComponent(id)}`);
    sheetGallery = { ...effect, id };
    const controls = (effect.schema || []).map((s) => `<li><b>${esc(s.label)}</b> <span>${esc(s.type)}</span></li>`).join("");
    const layers = effect.kind === "layers" && effect.layers
      ? `<ul class="st-api">${effect.layers.map((l) => `<li>${esc(H.info.types[l.type]?.name || l.type)}${l.palette ? " · palette" : ""}${(l.react?.amount || 0) > 0 ? " · follows the music" : ""}</li>`).join("")}</ul>`
      : `<p class="hint">Written in code. Remix it to read and change the script.</p>`;
    openSheet(`<div class="body"><h2>${esc(effect.name)}</h2>
      <p class="lede" style="margin:0">${esc(effect.description || "No description.")}</p>
      <p class="hint">by ${esc(effect.author || "unknown")}${effect.remix_of ? ` · a remix of ${esc(effect.remix_of)}` : ""}${(effect.permissions || []).includes("audio") ? " · uses the music" : ""}</p>
      <div class="sub-head">Controls</div>${controls ? `<ul class="st-api">${controls}</ul>` : `<p class="hint">No controls: its look is fixed.</p>`}
      <div class="sub-head">How it's made</div>${layers}
      <div class="actions"><button class="btn" data-gd="install">Install</button><button class="btn" data-gd="remix">Remix it</button><span class="spacer"></span><button class="btn quiet" data-close>Close</button></div></div>`);
  } catch (e) { fail(e); }
}


return { renderCommunity, paintCommunity, shareIt, galleryDetail, deactivate, getSheetEffect: () => sheetGallery };
}
