import { $, S, effectName, emit, esc, post } from "./core.js";
import { openSheet } from "./sheet.js";
import { mountProfileFrames } from "./profile-preview.js";
import { openProfileLibrary } from "./profile-library.js";
import { lookbookCatalog } from "./lookbook-catalog.js";

const art = tile => `style="--plate-x:${(tile % 3)*50}%;--plate-y:${Math.floor(tile/3)*100}%"`;
const swatches = colors => `<span class="lb-colors" aria-label="${esc(colors.join(", "))}">${colors.map(c=>`<i style="background:${esc(c)}"></i>`).join("")}</span>`;

export async function openLookbook(tile = null) {
  const sheet = openSheet(`<section class="body lookbook"><header class="lb-head"><div><h2>Lighting lookbook</h2>
    <p>Six lighting recipes included with Airglow.</p></div><button class="btn quiet" data-close>Close</button></header>
    <div class="lb-content"></div><p class="lb-status" role="status" aria-live="polite">Opening collection…</p></section>`);
  const root = sheet.querySelector(".lookbook"), content = $(".lb-content",root), status = $(".lb-status",root);
  let items, selected, draft, includeAreas, closed=false, busy=false, compare="", cancel=null, timer;
  const stopFrame = () => { clearTimeout(timer); cancel?.(); cancel=null; };
  const active = () => !closed && root.isConnected && sheet.open;
  const visibility = () => { if(document.hidden) stopFrame(); else if(selected) frame(); };
  const onClose = () => { if(!sheet.open || !root.isConnected) stop(); };
  function stop() {
    if(closed) return;
    closed=true; stopFrame(); observer.disconnect(); sheet.removeEventListener("close",onClose); document.removeEventListener("visibilitychange",visibility);
  }
  const observer=new MutationObserver(()=>{if(!root.isConnected) stop();});
  observer.observe(sheet,{childList:true}); sheet.addEventListener("close",onClose); document.addEventListener("visibilitychange",visibility);
  function browse(restore) {
    stopFrame(); selected=null; status.textContent="";
    content.innerHTML=`<div class="lb-shelf">${items.map(({tile,motion,colors,profile:p})=>`<button class="lb-card" type="button" data-plate="${tile}" aria-label="Preview ${esc(p.name)}">
      <span class="lb-illustration" ${art(tile)} aria-hidden="true"></span>
      <span class="lb-caption"><span><strong>${esc(p.name)}</strong><small>${esc(motion)} · ${p.lighting.brightness}%</small></span>${swatches(colors)}</span>
      </button>`).join("")}</div><div class="lb-library-link"><button type="button" class="text-link" data-lb-library>Open saved profiles</button></div>`;
    if(restore!==undefined) content.querySelector('[data-plate="'+restore+'"]')?.focus({preventScroll:true});
  }
  function previewSelection() {
    const p=selected.profile;
    const target={name:p.name, ...draft, areas:includeAreas ? p.areas : {}};
    const other=S.presets.find(x=>x.id===compare);
    return other ? [target,other] : [target];
  }
  function frame() {
    stopFrame(); if(!active() || document.hidden || !selected) return;
    const profiles=previewSelection(), portraits=$(".pl-portraits",content);
    portraits.dataset.count=String(profiles.length);
    portraits.innerHTML=profiles.map(p=>`<figure class="pl-portrait" data-frame="loading"><canvas aria-label="${esc(p.name)} lighting preview"></canvas>
      <output role="status">Rendering…</output><figcaption><strong>${esc(p.name)}</strong><span>${p.brightness}%</span></figcaption></figure>`).join("");
    cancel=mountProfileFrames(content,profiles,2);
  }
  function experiment(item) {
    selected=item; const p=item.profile; draft=structuredClone(p.lighting); includeAreas=!!p.areas; compare="";
    status.textContent="";
    const areaNames={cooler:"CPU cooler",rear:"Rear",top:"Top"};
    const areas=Object.entries(p.areas || {}).map(([where,scene])=>`${areaNames[where] || where} · ${effectName(scene.effect)}`).join(", ");
    content.innerHTML=`<button class="text-link lb-back" data-lb-back>← All six looks</button>
      <div class="lb-feature"><div class="lb-cover"><div class="lb-illustration" ${art(item.tile)} aria-hidden="true"></div>
        <h3>${esc(p.name)}</h3>${swatches(item.colors)}<p>${esc(p.description)}</p></div>
        <div class="lb-bench"><div class="lb-stage-head"><strong>Lighting preview</strong><small data-rig-source></small></div>
        <div class="pl-portraits"></div>
        <label class="lb-adjust" for="lb-bright"><span><span id="lb-bright-label">Brightness</span><output for="lb-bright">${draft.brightness}%</output></span>
          <input id="lb-bright" type="range" aria-labelledby="lb-bright-label" aria-valuetext="${draft.brightness}%" min="0" max="100" step="1" value="${draft.brightness}"></label>
        ${areas ? `<label class="check"><input id="lb-areas" type="checkbox" checked> Include area accents <span class="hint">${esc(areas)}</span></label>`:""}
        <label class="lb-compare"><span>Compare with</span><select id="lb-compare" class="select"><option value="">No comparison</option>
          ${S.presets.map(other=>`<option value="${esc(other.id)}">${esc(other.name)}</option>`).join("")}</select></label>
        <form class="lb-save"><label for="lb-name">Profile name</label><div><input id="lb-name" class="text" maxlength="40" required value="${esc(p.name)}" autocomplete="off">
          <button class="btn primary" type="submit">Save to library</button></div></form></div></div>`;
    $(".lb-back",content).focus({preventScroll:true});frame();
    $("#lb-bright",content).oninput = e => {
      draft.brightness=Number(e.target.value); content.querySelector('output[for="lb-bright"]').textContent=draft.brightness+"%";
      e.target.setAttribute("aria-valuetext", draft.brightness+"%");
      clearTimeout(timer); timer=setTimeout(frame,160);
    };
    const areaInput=$("#lb-areas",content); if(areaInput) areaInput.onchange=e=>{includeAreas=e.target.checked;frame();};
    $("#lb-compare",content).onchange=e=>{compare=e.target.value;frame();};
    $(".lb-save",content).onsubmit=async e=>{
      e.preventDefault(); if(busy)return;
      const name=$("#lb-name",content).value.trim();
      if(!name){status.textContent="Give the profile a name."; $("#lb-name",content).focus();return;}
      const profile={...p,name,lighting:{...draft}}; if(!includeAreas) delete profile.areas;
      const form=e.currentTarget; busy=true; form.querySelector("button").disabled=true;
      try{
        const result=await post("/api/profiles/import",{profile});
        S.presets=result.presets;emit("refresh");
        if(!active())return;
        openProfileLibrary(result.id);
        const notice=sheet.querySelector(".pl-feedback");if(notice)notice.textContent=`${name} added to your library.`;
      }catch(error){if(active())status.textContent=error.message;}
      finally{busy=false;form.querySelector("button").disabled=false;}
    };
  }
  content.onclick=e=>{
    const button=e.target.closest("button"); if(!button||busy)return;
    if(button.hasAttribute("data-lb-library")){openProfileLibrary();return;}
    if(button.hasAttribute("data-lb-back")){browse(selected.tile);return;}
    if(button.hasAttribute("data-plate")) experiment(items.find(i=>i.tile===Number(button.dataset.plate)));
  };
  try{
    items=await lookbookCatalog();
    if(active()){
      const chosen = items.find(item=>item.tile===tile);
      if(chosen)experiment(chosen); else browse();
    }
  }
  catch(error){if(active()){status.textContent=error.message;content.innerHTML='<button class="btn" data-lb-retry>Try again</button>';
      content.querySelector("button").onclick=openLookbook;}}
}
