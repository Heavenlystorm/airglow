import { $, S, curveLabel, effectName, emit, esc, fmtTemp, get, on, post, sceneSwatch } from "./core.js";
import { favoriteButton, isFavorite } from "./quick-preferences.js";
import { applyProfile, profileToast } from "./profile-undo.js";
import { closeSheet, openSheet } from "./sheet.js";
import { mountProfileFrames } from "./profile-preview.js";
import { saveLocalFile } from "./save-local-file.js";

const cooling = p => !!(p.fan_points?.length || p.fan_curve || p.fan_profile);
const areaNames = { front:"Front", top:"Top", rear:"Rear", bottom:"Bottom", side:"Side panel", cooler:"CPU cooler",
  ram:"Memory", board:"Motherboard", gpu:"Graphics card", desk:"Desk" };
const human = s => areaNames[s] || s.replace(/[_-]/g, " ").replace(/^./, x => x.toUpperCase());
const pairs = obj => Object.entries(obj || {}).sort(([a],[b]) => a.localeCompare(b));
const valueText = value => Array.isArray(value) ? value.map(valueText).join(" · ") :
  typeof value === "object" && value !== null ? pairs(value).map(([k,v]) => `${human(k)} ${valueText(v)}`).join(", ") : String(value);
const curveText = points => points.map(p => `${fmtTemp(p[0])} → ${p[1]}%`).join(" · ");
// null records a missing named curve. undefined leaves unnamed curves out of the request.
const namedPoints = (p,curves) => p.fan_curve && !p.fan_points?.length
  ? (Object.hasOwn(curves,p.fan_curve) ? curves[p.fan_curve] : null) : undefined;
const linkedRules = (p,rules) => rules.filter(r => r.then.type === "preset" && r.then.preset === p.id);
let refreshFavorites = null, currentClose = null, lastId = null;
on("quick:changed", () => refreshFavorites?.());

function details(p, libraryRules, curves) {
  const points = p.fan_points?.length ? p.fan_points : namedPoints(p,curves);
  const rows = new Map([["Effect", effectName(p.effect)], ["Brightness", `${p.brightness}%`],
    ["Fan curve", p.fan_points?.length ? "Custom curve" : p.fan_curve ? curveLabel(p.fan_curve) : "Keep current"],
    ["FanControl", p.fan_profile || "Keep current"]]);
  if (points !== undefined) rows.set("Curve points", points === null ? "Unavailable" : curveText(points));
  const addParams = (effect, params, prefix = "") => {
    const spec = S.effects.find(e => e.id === effect);
    for (const [k,v] of pairs(params)) {
      const field = spec?.params?.find(x=>x.id===k);
      const choice = field?.options?.find(option=>option.value===v);
      const text = choice?.label ?? (field?.unit==="°C" ? fmtTemp(v) :
        field?.type==="toggle" ? (v ? "On" : "Off") : valueText(v) + (field?.unit || ""));
      rows.set(prefix + (field?.label || human(k)), text);
    }
  };
  addParams(p.effect, p.params, "Lighting · ");
  for (const [area, scene] of pairs(p.areas)) {
    const name = human(area);
    rows.set(name, effectName(scene.effect));
    addParams(scene.effect, scene.params, name + " · ");
  }
  const rules = linkedRules(p,libraryRules).map(r=>r.name || r.id);
  if (rules.length) rows.set("Used by rules", rules.join(", "));
  return rows;
}

export function openProfileLibrary(pick = null) {
  currentClose?.();
  let first = S.presets.find(p => p.id === (pick || lastId || S.state?.active_preset))?.id || S.presets[0]?.id;
  let second = "", phase = 2, differences = false, cancelFrames = null, drawTask, closed = false;
  let staged = null, importSerial = 0, busy = false;
  let displayed = [];
  let displayedRules = [];
  let libraryRules = structuredClone(S.rules || []);
  let libraryCurves = structuredClone(S.curves?.curves || {});
  const sheet = openSheet('<div class="body profile-library"><header class="sheet-head"><h2>Profile library</h2><button class="btn quiet" data-close>Close</button></header>' +
    '<div class="pl-workspace"><aside class="pl-catalog"><label class="pl-search"><span class="sr-only">Search profiles</span><input type="search" class="text" id="pl-search" placeholder="Find a saved profile" autocomplete="off"></label>' +
    '<label><span class="sr-only">Filter profiles</span><select class="select" id="pl-filter"><option value="all">All saved profiles</option><option value="favorites">Favorites</option><option value="lighting">Lighting only</option><option value="cooling">With cooling</option></select></label>' +
    '<div id="pl-list" class="pl-list" role="group" aria-label="Saved profiles"></div><button class="btn" type="button" id="pl-lookbook">Lighting lookbook</button><button class="btn quiet" type="button" id="pl-import">Import profile file</button><input type="file" id="pl-file" accept=".json,application/json" hidden></aside>' +
    '<section class="pl-detail" aria-label="Profile details"><div id="pl-import-review" hidden></div><div id="pl-detail"></div></section></div><p class="pl-feedback" role="status" aria-live="polite"></p></div>');
  const root = sheet.querySelector(".profile-library"), list = $("#pl-list",root), detail = $("#pl-detail",root);
  const search = $("#pl-search",root), filter = $("#pl-filter",root), review = $("#pl-import-review",root), file = $("#pl-file",root);
  const feedback = message => { if (!closed) root.querySelector(".pl-feedback").textContent = message; };
  const lookup = id => S.presets.find(p => p.id === id);
  const match = () => {
    const words = search.value.toLocaleLowerCase().trim().split(/\s+/);
    return S.presets.filter(p => {
      const text = [p.name,p.description,effectName(p.effect),...Object.keys(p.areas || {}).map(human),p.fan_curve].filter(Boolean).join(" ").toLocaleLowerCase();
      return words.every(w => text.includes(w)) && (filter.value === "all" || (filter.value === "favorites" ? isFavorite("profile",p.id) : (filter.value === "cooling") === cooling(p)));
    });
  };
  function drawList() {
    const visible = match();
    const html = visible.map(p => `<button type="button" class="pl-card" data-pl-id="${esc(p.id)}" aria-pressed="${p.id === first}">
      <span class="pl-swatch" aria-hidden="true" style="background:${esc(sceneSwatch(p))}"></span><span><strong>${esc(p.name)}</strong><small>${esc(effectName(p.effect))} · ${p.brightness}%</small></span></button>`).join("");
    list.innerHTML = html || `<p class="hint">${S.presets.length ? "No matching profiles." : "Save a look from Lighting or import a profile."}</p>`;
  }
  function frame() {
    clearTimeout(drawTask); cancelFrames?.(); cancelFrames = null;
    if (closed || document.hidden || review.hidden === false) return;
    if (!displayed.length) return;
    for (const p of detail.querySelectorAll(".pl-portrait")) { p.dataset.frame = "loading"; p.querySelector("output").textContent = "Rendering…"; }
    cancelFrames = mountProfileFrames(detail, displayed, phase);
  }
  function table() {
    const [a,b] = displayed;
    if (!a) return;
    const left = details(a,libraryRules,libraryCurves), right = b ? details(b,libraryRules,libraryCurves) : new Map();
    const keys = [...new Set([...left.keys(),...right.keys()])].filter(k => !b || !differences || left.get(k) !== right.get(k));
    const t = detail.querySelector(".pl-table-body");
    t.innerHTML = keys.length ? keys.map(key => `<tr><th scope="row">${esc(key)}</th><td>${esc(left.get(key) ?? "—")}</td>${b ? `<td>${esc(right.get(key) ?? "—")}</td>` : ""}</tr>`).join("") :
      '<tr><td colspan="3">These saved settings match.</td></tr>';
  }
  function drawDetail() {
    const a = lookup(first), b = lookup(second); cancelFrames?.(); cancelFrames = null;
    displayed = [a,b].filter(Boolean).map(p => structuredClone(p));
    displayedRules = a ? structuredClone(linkedRules(a,libraryRules)) : [];
    if (!a) { detail.innerHTML = "<h3>Your saved looks</h3><p class='hint'>Your first profile will appear here.</p>"; return; }
    const profiles = [a,b].filter(Boolean);
    let curveImpact = "";
    if (a.fan_points?.length) curveImpact = "Applies its custom fan curve";
    else if (a.fan_curve) {
      const name = curveLabel(a.fan_curve);
      curveImpact = namedPoints(a,libraryCurves) === null ? `The ${name} fan curve is unavailable` : `Applies the ${name} fan curve`;
    }
    detail.innerHTML = `<div class="pl-title"><h3>${esc(a.name)}</h3>${favoriteButton("profile",a.id,a.name)}</div>
      ${a.description ? `<p class="pl-description">${esc(a.description)}</p>` : ""}
      <label class="pl-comparator">Compare with <select class="select" id="pl-compare"><option value="">Single profile</option>${S.presets.filter(p=>p.id!==a.id).map(p=>`<option value="${esc(p.id)}"${b?.id===p.id?" selected":""}>${esc(p.name)}</option>`).join("")}</select></label>
      <div class="pl-portraits" data-count="${profiles.length}">${profiles.map(p=>`<figure class="pl-portrait"><canvas aria-label="${esc(p.name)} lighting preview"></canvas><output role="status">Rendering…</output><figcaption><strong>${esc(p.name)}</strong><span>${p.brightness}%</span></figcaption></figure>`).join("")}</div>
      <div class="pl-timeline"><span data-rig-source></span><label>Sample time <input type="range" id="pl-time" min="0" max="12" step=".5" value="${phase}"><output for="pl-time">${phase}s</output></label><button class="text-link" data-pl-retry>Refresh frame</button></div>
      <div class="pl-actions"><button class="btn primary" data-pl-apply>Apply ${esc(a.name)}</button><button class="btn" data-pl-export>Export file</button></div>
      ${displayedRules.length ? '<label class="check"><input type="checkbox" data-pl-export-rules> Include linked automation rules in the exported file</label>' : ""}
      <p class="pl-impact">${esc(cooling(a) ? [curveImpact,
        a.fan_profile ? "Loads FanControl profile " + a.fan_profile : ""].filter(Boolean).join(". ") + "." : "Applies lighting only.")}</p>
      <details class="pl-settings"${b?" open":""}><summary>${b?"Compare settings":"Saved settings"}</summary>
      ${b?`<label class="check"><input type="checkbox" id="pl-diffs"${differences?" checked":""}> Differences only</label>`:""}
      <table><thead><tr><th scope="col">Setting</th>${profiles.map(p=>`<th scope="col">${esc(p.name)}</th>`).join("")}</tr></thead><tbody class="pl-table-body"></tbody></table></details>`;
    table(); frame();
  }
  function leaveReview() {
    importSerial++; staged = null; review.hidden = true; review.replaceChildren(); detail.hidden = false; file.value = "";
  }
  function select(id) {
    const hadReview = !review.hidden;
    leaveReview(); if (id === second) second = first;
    first = id; lastId = id; feedback(""); drawList(); drawDetail();
    if (hadReview) detail.querySelector("h3")?.scrollIntoView({block:"nearest"});
  }
  async function reloadProfiles() {
    const previous = S.presets;
    const [rows,rules,{curves}] = await Promise.all([get("/api/presets"),get("/api/automations"),get("/api/curves")]);
    if (closed || busy || staged || S.presets !== previous) return;
    if (JSON.stringify(rows) === JSON.stringify(previous) && JSON.stringify(rules) === JSON.stringify(libraryRules)
      && JSON.stringify(curves) === JSON.stringify(libraryCurves)) return;
    libraryRules = rules;
    libraryCurves = curves;
    S.presets = rows;
    first = lookup(first)?.id || rows[0]?.id;
    second = lookup(second)?.id === first ? "" : lookup(second)?.id || "";
    drawList(); drawDetail();
  }
  list.onclick = e => {
    const button = e.target.closest("[data-pl-id]");
    if (button && !busy) { select(button.dataset.plId); list.querySelector('[aria-pressed="true"]')?.focus({preventScroll:true}); }
  };
  search.oninput = filter.onchange = drawList;
  search.onkeydown = e => {
    if (e.key === "Escape" && search.value) {
      e.preventDefault(); e.stopPropagation(); search.value = ""; drawList();
    }
  };
  refreshFavorites = () => { if (filter.value === "favorites") drawList(); };
  detail.onchange = e => {
    if (e.target.id === "pl-compare") { second = e.target.value; drawDetail(); $("#pl-compare",detail).focus({preventScroll:true}); }
    if (e.target.id === "pl-diffs") { differences = e.target.checked; table(); }
  };
  detail.oninput = e => {
    if (e.target.id !== "pl-time") return;
    phase = Number(e.target.value); detail.querySelector('output[for="pl-time"]').textContent = phase+"s";
    clearTimeout(drawTask); drawTask = setTimeout(frame,160);
  };
  detail.onclick = async e => {
    const button = e.target.closest("button"), p = displayed[0];
    if (!button || !p || busy) return;
    const reviewedCurvePoints = namedPoints(p,libraryCurves);
    if (button.hasAttribute("data-pl-retry")) { frame(); return; }
    if (button.hasAttribute("data-pl-apply")) {
      busy = true; button.disabled = true; feedback(`Applying ${p.name}…`);
      try { const result = await applyProfile(p,{ reviewed:true, reviewedCurvePoints }); emit("refresh");
        if (!closed) closeSheet(); profileToast(p,result);
      } catch (error) { feedback(error.message); }
      finally { busy = false; button.disabled = false; }
    }
    if (button.hasAttribute("data-pl-export")) {
      const rules = !!detail.querySelector("[data-pl-export-rules]")?.checked;
      const reviewedRules = displayedRules;
      button.disabled = true;
      try { const result = await saveLocalFile({ filename:p.id+".airglow-profile.json", mime:"application/json",
        description:"Airglow profile", data:async () => JSON.stringify((await post("/api/profiles/export",
          {id:p.id,reviewed:p,reviewed_curve_points:reviewedCurvePoints,include_automations:rules,...(rules?{reviewed_rules:reviewedRules}:{})})).profile,null,2) });
        if (result !== "cancelled") feedback(p.fan_profile ? "Exported lighting and portable fan curves. FanControl assignments are local to this PC." : "Profile file exported.");
      }
      catch (error) { feedback(error.message); }
      finally { button.disabled = false; }
    }
  };
  $("#pl-import",root).onclick = () => { if (!busy) file.click(); };
  $("#pl-lookbook",root).onclick = async () => { if (!busy) { const m = await import("./lookbook.js"); if (!closed) m.openLookbook(); } };
  file.onchange = async () => {
    const chosen = file.files[0], ticket = ++importSerial; if (!chosen) return;
    try {
      if (chosen.size > 256*1024) throw new Error("Choose an Airglow profile file under 256 KB.");
      let content;
      try { content = JSON.parse(await chosen.text()); } catch { throw new Error("This file does not contain valid JSON."); }
      const p = await post("/api/profiles/inspect-file",{profile:content});
      if (closed || ticket !== importSerial) return;
      staged = p; cancelFrames?.(); clearTimeout(drawTask); detail.hidden = true; review.hidden = false;
      review.innerHTML = `<h3>Import ${esc(p.name)}</h3><p>${esc(p.description || effectName(p.lighting.effect))}</p>
        <p>${esc(effectName(p.lighting.effect))} · ${p.lighting.brightness}% brightness</p>
        ${p.fans ? '<label class="check"><input type="checkbox" data-pl-fans checked> Include its fan curve</label>' : ""}
        ${p.automations?.length ? '<label class="check"><input type="checkbox" data-pl-rules> Import associated rules as paused</label>' : ""}
        <p class="hint">Adds a new copy to your library. You can apply it when you’re ready.</p>
        <div class="btn-row"><button class="btn primary" data-pl-install>Add to library</button><button class="btn quiet" data-pl-back>Cancel</button></div>`;
      feedback(""); review.querySelector("button").focus();
    } catch (error) { if (ticket===importSerial) feedback(error.message); }
    finally { if(ticket===importSerial) file.value=""; }
  };
  review.onclick = async e => {
    if (busy) return;
    if (e.target.closest("[data-pl-back]")) { leaveReview(); drawDetail(); $("#pl-import",root).focus(); return; }
    const button=e.target.closest("[data-pl-install]");
    if (!button || !staged) return;
    busy=true; button.disabled=true;
    try {
      const data = await post("/api/profiles/import",{profile:staged,include_fans:!!review.querySelector("[data-pl-fans]")?.checked,
        include_automations:!!review.querySelector("[data-pl-rules]")?.checked});
      S.presets=data.presets;
      if (data.paused_rules) { libraryRules = data.rules; emit("rules:imported", data.rules); }
      emit("refresh"); if (closed) return;
      search.value=""; filter.value="all"; select(data.id); feedback(data.paused_rules ? `Added to library with ${data.paused_rules} paused rules.` : "Added to library.");
      list.querySelector('[aria-pressed="true"]')?.focus({preventScroll:true});
    } catch(error) { feedback(error.message); } finally { busy=false; button.disabled=false; }
  };
  const visibility = () => { if (document.hidden) { clearTimeout(drawTask); cancelFrames?.(); } else frame(); };
  const onClose = () => { if (!sheet.open || !root.isConnected) stop(); };
  function stop() {
    if (closed) return;
    closed=true; clearTimeout(drawTask); cancelFrames?.(); observer.disconnect();
    sheet.removeEventListener("close",onClose); document.removeEventListener("visibilitychange",visibility);
    refreshFavorites=null; currentClose=null;
  }
  const observer = new MutationObserver(() => { if (!root.isConnected) stop(); });
  observer.observe(sheet,{childList:true}); sheet.addEventListener("close",onClose); document.addEventListener("visibilitychange",visibility);
  currentClose=stop; drawList(); drawDetail(); search.focus({preventScroll:true});
  reloadProfiles().catch(error => feedback("Could not refresh saved profiles. " + error.message));
}
