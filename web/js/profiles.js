// Profiles: saving, editing and sharing your own, and the community gallery (browse, preview, try, keep).
// A profile is the lighting look, per-area effects, optionally its own fan curve, and when shared, the automations
// that switch to it. The gallery lives in profiles/ in the Airglow repository; sharing opens GitHub in the browser.

import { $, $$, S, curveLabel, effectName, emit, esc, fail, get, post, sceneSwatch, toast } from "./core.js";
import { sentence } from "./automations.js";
import { closeSheet, openSheet } from "./sheet.js";
import { paintMini } from "./stage.js";

const AREA_LABEL = { front: "Front", top: "Top", rear: "Rear", bottom: "Bottom", side: "Side panel", cooler: "CPU cooler", ram: "Memory", board: "Motherboard", gpu: "Graphics card", desk: "Desk" };
const BADGES = { music: "Music", screen: "Screen", fans: "Fan curve", automations: "Automations", areas: "Per-area" };
const remember = (k, v) => { try { if (v === undefined) return localStorage.getItem(k) || ""; localStorage.setItem(k, v); } catch { return ""; } return v; };

const swatch = (p) => sceneSwatch({ effect: p.effect ?? p.lighting?.effect, params: p.params ?? p.lighting?.params });
const rulesFor = (id) => S.rules.filter((r) => r.then.type === "preset" && r.then.preset === id);

function lookLine(p) {
  const L = p.lighting || p;
  const areas = Object.entries(p.areas || {});
  return `${effectName(L.effect)} at ${L.brightness}%${areas.length ? `, plus ${areas.map(([k, a]) => `${effectName(a.effect)} on ${(AREA_LABEL[k] || k).toLowerCase()}`).join(", ")}` : ""}`;
}

// ------------------------------------------------------------ save current
export function saveProfileSheet(done) {
  const curve = S.state.fans.base_curve;
  const named = Object.keys(S.curves.curves).includes(curve);
  openSheet(`<form id="sp-form"><h2>Save as profile</h2>
    <p class="hint">Saves the current effect, its settings and brightness, and any effects you gave single zones, by where they sit in your case. Shared, they land on the same areas of someone else's rig.</p>
    <label class="field"><span class="lab">Name</span><input class="text" id="sp-name" required maxlength="40" autocomplete="off" placeholder="Movie night"></label>
    <label class="field"><span class="lab">Description <span class="hint">(optional)</span></span><input class="text" id="sp-desc" maxlength="200" autocomplete="off" placeholder="What it looks like, when you use it"></label>
    <label class="check"><input type="checkbox" id="sp-fans" checked> Switch fans to ${named ? `the ${esc(curveLabel(curve))} curve` : "the current curve"} too</label>
    <p class="err" id="sp-err" hidden></p>
    <div class="actions"><span class="spacer"></span><button type="button" class="btn quiet" data-close>Cancel</button><button class="btn primary">Save profile</button></div></form>`);
  $("#sp-name").focus();
  $("#sp-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const name = $("#sp-name").value.trim();
    try {
      const r = await post("/api/presets/from-current", { name, description: $("#sp-desc").value.trim(), include_fans: $("#sp-fans").checked });
      S.presets = r.presets;
      S.state.active_preset = r.id;
      closeSheet(); done?.(); toast(`${name} saved`);
    } catch (err) { $("#sp-err").textContent = err.message; $("#sp-err").hidden = false; }
  });
}

// ------------------------------------------------------------ edit (rename, describe, fans, delete, share)
export function editProfilesSheet(done) {
  const draft = structuredClone(S.presets);
  const curves = Object.keys(S.curves.curves);
  const fcProfiles = S.settings?.fan_profiles || [];
  const row = (p, i) => `<div class="pf-row" data-row="${i}">
      <span class="pf-dot" style="background:${swatch(p)}"></span>
      <div class="pf-fields">
        <input class="text" data-name value="${esc(p.name)}" maxlength="40" aria-label="Profile name">
        <input class="text" data-desc value="${esc(p.description || "")}" maxlength="200" placeholder="Description (optional)" aria-label="Description">
        <div class="pf-line">
          <select class="select" data-curve aria-label="Fan curve"><option value="">Fans unchanged</option>
            ${p.fan_points ? `<option value="own" selected>Its own fan curve</option>` : ""}
            ${curves.map((c) => `<option value="${c}"${!p.fan_points && c === p.fan_curve ? " selected" : ""}>${curveLabel(c)} fans</option>`).join("")}</select>
          ${fcProfiles.length ? `<select class="select" data-fc aria-label="FanControl profile"><option value="">Motherboard fans unchanged</option>${fcProfiles.map((f) => `<option value="${esc(f)}"${f === p.fan_profile ? " selected" : ""}>FanControl: ${esc(f.replace(/\.json$/i, ""))}</option>`).join("")}</select>` : ""}
        </div>
        <span class="hint">${esc(lookLine(p))}</span>
      </div>
      <div class="pf-btns"><button type="button" class="btn" data-share="${i}">Share</button>
        <button type="button" class="icon-btn" data-del="${i}" title="Delete ${esc(p.name)}"><svg><use href="#i-trash"/></svg></button></div>
    </div>`;
  const collect = () => $$("#ep-rows [data-row]").forEach((r) => {
    const p = draft[+r.dataset.row];
    p.name = r.querySelector("[data-name]").value.trim();
    p.description = r.querySelector("[data-desc]").value.trim();
    const c = r.querySelector("[data-curve]").value;
    if (c !== "own") { p.fan_curve = c || null; delete p.fan_points; }
    const fc = r.querySelector("[data-fc]");
    if (fc) p.fan_profile = fc.value || null;
  });
  const save = async () => {
    collect();
    S.presets = await post("/api/presets", { presets: draft });
    emit("refresh");
  };
  const render = () => {
    openSheet(`<form id="ep-form" class="wide"><h2>Profiles</h2><p class="hint">Rename, describe, set their fans, share or delete them. To change a profile's lighting, apply it, adjust, and save it again.</p>
      <div class="pf-rows" id="ep-rows">${draft.map(row).join("") || `<p class="hint">No profiles yet.</p>`}</div>
      <p class="err" id="ep-err" hidden></p>
      <div class="actions"><button type="button" class="btn quiet" id="ep-gallery">Browse the gallery</button><span class="spacer"></span><button type="button" class="btn quiet" data-close>Cancel</button><button class="btn primary">Save profiles</button></div></form>`);
    $("#ep-rows").addEventListener("click", async (e) => {
      const d = e.target.closest("[data-del]");
      if (d) { collect(); draft.splice(+d.dataset.del, 1); render(); return; }
      const s = e.target.closest("[data-share]");
      if (s) {
        try { await save(); shareSheet(S.presets[+s.dataset.share], () => editProfilesSheet(done)); }
        catch (err) { $("#ep-err").textContent = err.message; $("#ep-err").hidden = false; }
      }
    });
    $("#ep-gallery").addEventListener("click", () => openGallery());
    $("#ep-form").addEventListener("submit", async (e) => {
      e.preventDefault();
      try { await save(); closeSheet(); done?.(); toast("Profiles saved"); }
      catch (err) { $("#ep-err").textContent = err.message; $("#ep-err").hidden = false; }
    });
  };
  render();
}

// ------------------------------------------------------------ share: opens GitHub with the profile filled in
function shareSheet(p, back) {
  const rules = rulesFor(p.id);
  const hasFans = !!(p.fan_points || p.fan_curve);
  openSheet(`<form id="sh-form"><h2>Share ${esc(p.name)}</h2>
    <p class="lede" style="margin:0">Your profile goes to the community gallery as a pull request on GitHub. GitHub opens in your browser with everything filled in; press <b>Propose new file</b> there. It shows up in everyone's gallery once it's reviewed.</p>
    <label class="field"><span class="lab">Your GitHub name</span><input class="text" id="sh-author" required maxlength="40" autocomplete="off" value="${esc(remember("airglow.githubName"))}" placeholder="octocat"></label>
    <label class="field"><span class="lab">Description</span><input class="text" id="sh-desc" maxlength="200" value="${esc(p.description || "")}" placeholder="What it looks like, when you use it"></label>
    <div class="pf-look"><span class="pf-dot" style="background:${swatch(p)}"></span><span>${esc(lookLine(p))}</span></div>
    ${hasFans ? `<label class="check"><input type="checkbox" id="sh-fans" checked> Include its fan curve${p.fan_points ? "" : ` (${esc(curveLabel(p.fan_curve))})`}</label>` : ""}
    ${rules.length ? `<label class="check"><input type="checkbox" id="sh-auto" checked> Include ${rules.length === 1 ? "the automation" : `the ${rules.length} automations`} that switch to it</label>
      <ul class="pf-rules">${rules.map((r) => `<li>${sentence(r)}</li>`).join("")}</ul>` : ""}
    <p class="hint">Only what's listed here is shared: no device names, no layout, nothing about your PC.</p>
    <p class="err" id="sh-err" hidden></p>
    <div class="actions"><button type="button" class="btn quiet" id="sh-back">Back</button><span class="spacer"></span><button class="btn primary">Open GitHub</button></div></form>`);
  $("#sh-back").addEventListener("click", back);
  $("#sh-author").focus();
  $("#sh-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const author = remember("airglow.githubName", $("#sh-author").value.trim());
    try {
      const r = await post("/api/profiles/share", { id: p.id, author, description: $("#sh-desc").value.trim(),
        include_fans: !!$("#sh-fans")?.checked, include_automations: !!$("#sh-auto")?.checked, open: false });
      // GitHub only takes about 8 KB in an address: a longer profile goes on the clipboard, to paste on the page
      let copied = false;
      if (!r.filled) { try { await navigator.clipboard.writeText(r.source); copied = true; } catch { copied = false; } }
      await post("/api/profiles/open-share", { url: r.url });
      openSheet(`<div class="body"><h2>GitHub is open in your browser</h2>
        ${r.filled ? `<p class="lede" style="margin:0">Sign in if it asks, then press <b>Propose new file</b> and <b>Create pull request</b>. That's it: it's checked automatically and appears in the gallery once it's merged.</p>`
          : `<p class="lede" style="margin:0">Your profile is too big to fill in for you, so only the file name <span class="path">${esc(r.path)}</span> is there. ${copied ? "The profile is on your clipboard:" : "Copy the profile below,"} sign in if it asks, click into the empty file, press <b>Ctrl+V</b>, then <b>Propose new file</b> and <b>Create pull request</b>.</p>
            ${copied ? "" : `<textarea class="text" rows="6" readonly>${esc(r.source)}</textarea>`}`}
        <p class="hint">Browser didn't open? <a href="${esc(r.url)}" target="_blank" rel="noopener" id="sh-link">Open the page</a>.</p>
        <div class="actions"><span class="spacer"></span><button class="btn primary" data-close>Done</button></div></div>`);
    } catch (err) { $("#sh-err").textContent = err.message; $("#sh-err").hidden = false; }
  });
}

// ------------------------------------------------------------ gallery
const G = { list: null, filter: new Set(), picked: null, profile: null, timer: null, keep: null };

function stopPreview() {
  clearInterval(G.timer); G.timer = null;
  clearInterval(G.keep); G.keep = null;
  post("/api/preview", {}).catch(() => {});
}

async function startPreview(profile) {
  stopPreview();
  const send = () => post("/api/preview", { profile }).catch(() => {});
  await send();
  G.keep = setInterval(send, 5000);
  G.timer = setInterval(async () => {
    const c = $("#gl-stage");
    if (!c) { stopPreview(); return; }
    if (document.visibilityState === "hidden") return;      // closed to the tray: nothing to draw, nothing to fetch
    try { paintMini(c, (await get("/api/preview/frames")).zones); } catch { /* next frame */ }
  }, 90);
}

export async function openGallery() {
  G.filter.clear(); G.picked = null; G.profile = null;
  openSheet(`<div class="body wide gallery"><div class="su-top"><span class="su-where"><b>Profile gallery</b></span><button class="icon-btn" data-close aria-label="Close"><svg aria-hidden="true"><use href="#i-close"/></svg></button></div>
    <p class="hint" style="margin:0">Loading profiles from GitHub…</p></div>`);
  $("#sheet").addEventListener("close", stopPreview, { once: true });
  try { G.list = await get("/api/gallery"); }
  catch (e) {
    openSheet(`<div class="body"><h2>The gallery didn't load</h2><p class="lede" style="margin:0">${esc(e.message)}</p>
      <div class="actions"><span class="spacer"></span><button class="btn quiet" data-close>Close</button><button class="btn primary" id="gl-retry">Try again</button></div></div>`);
    $("#gl-retry").addEventListener("click", openGallery);
    return;
  }
  renderGallery();
  const first = G.list.profiles[0];
  if (first) pick(first.id);
}

function visible() {
  return G.list.profiles.filter((p) => [...G.filter].every((f) => p.badges.includes(f)));
}

function renderGallery() {
  const items = visible();
  const featured = new Set(G.list.featured);
  openSheet(`<div class="body wide gallery">
    <div class="su-top"><span class="su-where"><b>Profile gallery</b><span>${G.list.profiles.length} profiles from the community</span></span><button class="icon-btn" data-close aria-label="Close"><svg aria-hidden="true"><use href="#i-close"/></svg></button></div>
    <div class="gl-filters" role="group" aria-label="Filter">${Object.entries(BADGES).map(([k, v]) => `<button class="chip-btn" data-filter="${k}" aria-pressed="${G.filter.has(k)}">${v}</button>`).join("")}</div>
    <div class="gl-main">
      <div class="gl-list" role="listbox" aria-label="Profiles">${items.map((p) => `<button class="gl-card" role="option" data-pick="${esc(p.id)}" aria-selected="${G.picked === p.id}">
          <strong>${esc(p.name)}${featured.has(p.id) ? ` <span class="tag new">Featured</span>` : ""}</strong>
          <span class="gl-by">by ${esc(p.author || "someone")}</span>
          <span class="gl-badges">${p.badges.map((b) => `<i>${BADGES[b] || b}</i>`).join("")}</span></button>`).join("") || `<p class="hint">Nothing matches these filters.</p>`}</div>
      <div class="gl-detail" id="gl-detail">${detailHTML()}</div>
    </div>
    <p class="hint" style="margin:0">Have one to share? Profiles, Edit, then Share next to it.</p>
  </div>`);
  $$("#sheet [data-filter]").forEach((b) => b.addEventListener("click", () => {
    const f = b.dataset.filter;
    G.filter.has(f) ? G.filter.delete(f) : G.filter.add(f);
    renderGallery();
  }));
  $$("#sheet [data-pick]").forEach((b) => b.addEventListener("click", () => pick(b.dataset.pick)));
  wireDetail();
}

function detailHTML() {
  const p = G.profile;
  if (!p) return `<div class="gl-stage-wrap"><canvas id="gl-stage"></canvas></div><p class="hint">${G.picked ? "Loading…" : "Pick a profile to see it on your own layout."}</p>`;
  const rules = p.automations || [];
  return `<div class="gl-stage-wrap"><canvas id="gl-stage" aria-label="Preview of ${esc(p.name)} on your layout"></canvas><span class="gl-live">Preview on your layout</span></div>
    <div><h2>${esc(p.name)}</h2><p class="gl-by">by ${esc(p.author || "someone")}</p></div>
    ${p.description ? `<p class="ins-desc" style="margin:0">${esc(p.description)}</p>` : ""}
    <p class="hint" style="margin:0">${esc(lookLine(p))}</p>
    ${p.fans ? `<p class="hint" style="margin:0">Comes with its own fan curve. You choose whether to use it.</p>` : ""}
    ${rules.length ? `<div><p class="hint" style="margin:0">Comes with ${rules.length === 1 ? "an automation" : `${rules.length} automations`}:</p><ul class="pf-rules">${rules.map((r) => `<li>${sentence(r, true, { self: p.name })}</li>`).join("")}</ul></div>` : ""}
    <div class="actions"><span class="spacer"></span><button class="btn primary" id="gl-try">Try it on my lights</button></div>`;
}

function wireDetail() {
  $("#gl-try")?.addEventListener("click", () => tryProfile(G.profile));
}

async function pick(id) {
  G.picked = id; G.profile = null;
  $$("#sheet [data-pick]").forEach((b) => b.setAttribute("aria-selected", String(b.dataset.pick === id)));
  $("#gl-detail").innerHTML = detailHTML();
  try {
    const p = await get(`/api/gallery/profile?id=${encodeURIComponent(id)}`);
    if (G.picked !== id) return;
    G.profile = p;
    $("#gl-detail").innerHTML = detailHTML();
    wireDetail();
    startPreview(p);
  } catch (e) { fail(e); }
}

// ------------------------------------------------------------ trying one on: Keep or Discard, from any tab
const T = { profile: null, fans: false, autos: true };
try { T.profile = JSON.parse(sessionStorage.getItem("airglow.trial") || "null"); } catch { /* none */ }

async function tryProfile(p) {
  stopPreview();
  try {
    await post("/api/profiles/try", { profile: p, include_fans: false });
    T.profile = p; T.fans = false; T.autos = true;
    try { sessionStorage.setItem("airglow.trial", JSON.stringify(p)); } catch { /* fine */ }
    closeSheet();
    emit("refresh");
  } catch (e) { fail(e); }
}

function endTrial() {
  T.profile = null;
  try { sessionStorage.removeItem("airglow.trial"); } catch { /* fine */ }
}

export function renderTrial() {
  const t = S.state?.trial?.kind === "studio" ? null : S.state?.trial;     // the Studio has its own switch
  let d = $("#trial-dock");
  if (!t) { if (d) d.hidden = true; if (T.profile) endTrial(); return; }
  if (!d) {
    d = document.createElement("aside");
    d.id = "trial-dock";
    d.className = "guide-dock trial-dock";
    d.setAttribute("aria-label", "Trying a profile");
    document.body.append(d);
    d.addEventListener("click", onTrialClick);
    d.addEventListener("change", onTrialChange);
  }
  const p = T.profile;
  const rules = p?.automations || [];
  const key = JSON.stringify([t, !!p, T.fans, T.autos]);
  if (d._key === key && !d.hidden) return;
  d._key = key;
  d.hidden = false;
  d.innerHTML = `<div><h2>Trying ${esc(t.name)}</h2><p class="lede">It's on your lights now. Keep it to add it to your profiles, or discard it and everything goes back as it was.</p></div>
    ${p?.fans ? `<label class="check"><input type="checkbox" data-t="fans"${T.fans ? " checked" : ""}> Use its fan curve</label>` : ""}
    ${rules.length ? `<label class="check"><input type="checkbox" data-t="autos"${T.autos ? " checked" : ""}> Add ${rules.length === 1 ? "its automation" : `its ${rules.length} automations`}</label>
      <ul class="pf-rules">${rules.map((r) => `<li>${sentence(r, true, { self: p.name })}</li>`).join("")}</ul>` : ""}
    <div class="actions"><button class="btn quiet" data-t="discard">Discard</button><span class="spacer"></span><button class="btn primary" data-t="keep"${p ? "" : " disabled"}>Keep it</button></div>`;
}

async function onTrialChange(e) {
  const k = e.target.dataset.t;
  if (k === "autos") { T.autos = e.target.checked; return; }
  if (k === "fans" && T.profile) {
    T.fans = e.target.checked;
    try { await post("/api/profiles/try", { profile: T.profile, include_fans: T.fans }); emit("refresh"); } catch (err) { fail(err); }
  }
}

async function onTrialClick(e) {
  const k = e.target.closest("[data-t]")?.dataset.t;
  if (k === "discard") {
    try { await post("/api/profiles/discard"); endTrial(); toast("Back to how it was"); emit("refresh"); } catch (err) { fail(err); }
  } else if (k === "keep" && T.profile) {
    const name = T.profile.name;
    try {
      const r = await post("/api/profiles/keep", { profile: T.profile, include_fans: T.fans, include_automations: T.autos });
      S.presets = r.presets;
      if (T.autos && T.profile.automations?.length) S.rules = await get("/api/automations");
      endTrial(); toast(`${name} added to your profiles`); emit("refresh");
    } catch (err) { fail(err); }
  }
}
