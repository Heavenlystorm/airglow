import { $, esc, get } from "./core.js";
import { fmtText } from "./readings.js";

const key = "airglow.desk.readings.v1";
const normalize = saved => {
  const entries = [];
  for (const row of Array.isArray(saved?.entries) ? saved.entries : []) {
    if (typeof row?.id !== "string" || !row.id || row.id.length > 256 || entries.some(m=>m.id===row.id)) continue;
    entries.push({ id:row.id, name:typeof row.name === "string" ? row.name : row.id, unit:typeof row.unit === "string" ? row.unit : "" });
    if (entries.length===4) break;
  }
  return {mode:saved?.mode==="desk" ? "desk" : "overview",entries};
};
function stored() {
  try { return normalize(JSON.parse(localStorage.getItem(key))); } catch { return normalize(null); }
}

export function displayPins(changed, current) {
  let saved=stored(), draft=null, catalog=[], lookupSeq=0, openedWith="", loadError="", loading=false;
  const dialog=$("#desk-pick"), mode=$("#desk-pick-mode"), custom=$("#desk-pick-custom"),
    search=$("#desk-pick-search"), list=$("#desk-pick-list"), order=$("#desk-pick-order"), status=$("#desk-pick-message"),
    retry=$("#desk-pick-retry");
  const updated=()=>{ $("#desk-reading-title").textContent=saved.mode==="desk" ? "Display readings" : "Pinned readings"; changed(); };
  function redraw() {
    custom.hidden=mode.value!=="desk";
    if(custom.hidden){status.textContent="Uses the readings pinned on your Overview.";return;}
    order.innerHTML=draft.map((m,i)=>`<div class="desk-pick-row"><span>${esc(m.name)}</span>
      <button type="button" data-up="${i}" aria-label="Move ${esc(m.name)} up"${i===0?" disabled":""}>↑</button>
      <button type="button" data-down="${i}" aria-label="Move ${esc(m.name)} down"${i===draft.length-1?" disabled":""}>↓</button>
      <button type="button" data-remove="${i}" aria-label="Remove ${esc(m.name)}">×</button></div>`).join("") || "<p>No readings selected.</p>";
    const words=search.value.trim().toLocaleLowerCase().split(/\s+/);
    const available=catalog.filter(m=>words.every(w=>[m.name,m.short,m.group,m.unit].join(" ").toLocaleLowerCase().includes(w)));
    list.innerHTML=available.map(m=>{
      const picked=draft.some(x=>x.id===m.id);
      return `<label class="desk-pick-sensor"><input type="checkbox" data-meter-id="${esc(m.id)}"${picked?" checked":""}${!picked && draft.length===4?" disabled":""}>
      <span><strong>${esc(m.short||m.name)}</strong><small>${esc(m.group||m.name)}</small></span><span>${esc(fmtText(m,m.value))}</span></label>`;
    }).join("") || (loading ? "<p>Loading readings…</p>" : loadError ? "" :
      search.value.trim() ? "<p>No matching readings.</p>" : "<p>No sensor readings are available yet.</p>");
    retry.hidden=!loadError;
    status.textContent=loadError || `${draft.length} of 4 selected`;
  }
  async function catalogLoad() {
    const seq=++lookupSeq;
    loading=true;loadError="";redraw();
    try {
      const result=await get("/api/sensors");if(!dialog.open || seq!==lookupSeq)return;
      catalog=result.sensors;
    } catch(error){
      if(!dialog.open || seq!==lookupSeq)return;
      catalog=[];loadError=`Could not load available readings. ${error.message}`;
    }
    const focus=document.activeElement?.dataset.meterId;
    loading=false;redraw();
    if(focus)list.querySelector('input[data-meter-id="'+CSS.escape(focus)+'"]')?.focus({preventScroll:true});
  }
  function open() {
    saved=stored();openedWith=JSON.stringify(saved);
    mode.value=saved.mode;draft=structuredClone(saved.entries);catalog=[];loading=false;loadError="";
    if(!draft.length && saved.mode==="overview")draft=(current()?.readings || []).map(m=>({id:m.id,name:m.short||m.name,unit:m.unit})).slice(0,4);
    search.value=""; redraw();dialog.showModal();if(mode.value==="desk")catalogLoad();
  }
  $("#desk-choose-readings").onclick=open;
  dialog.querySelector(".desk-close").onclick=()=>dialog.close();
  dialog.addEventListener("close",()=>{
    if(dialog.open)return;
    lookupSeq++;$("#desk-choose-readings").focus({preventScroll:true});
  });
  retry.onclick=()=>{search.focus({preventScroll:true});catalogLoad();};
  mode.onchange=()=>{
    if(mode.value==="desk"){catalogLoad();return;}
    lookupSeq++;loading=false;redraw();
  };
  search.oninput=redraw;
  list.onchange=e=>{
    const id=e.target.dataset.meterId, m=catalog.find(x=>x.id===id);if(!m)return;
    if(e.target.checked && draft.length<4 && !draft.some(x=>x.id===id))draft.push({id,name:m.short||m.name,unit:m.unit});
    else draft=draft.filter(x=>x.id!==id);
    redraw();list.querySelector('input[data-meter-id="'+CSS.escape(id)+'"]')?.focus({preventScroll:true});
  };
  order.onclick=e=>{
    const button=e.target.closest("button");if(!button)return;
    let index;
    if(button.hasAttribute("data-remove")){index=Number(button.dataset.remove);draft.splice(index,1);}
    else{
      const up=button.hasAttribute("data-up"), from=Number(up?button.dataset.up:button.dataset.down);
      index=from+(up?-1:1);if(index<0 || index>=draft.length)return;
      [draft[index],draft[from]]=[draft[from],draft[index]];
    }
    redraw(); const row=order.children[Math.min(index,draft.length-1)];
    (row?.querySelector("button:not(:disabled)")||search).focus({preventScroll:true});
  };
  $("#desk-pick-save").onclick=()=>{
    if(JSON.stringify(stored())!==openedWith){
      status.textContent="Display readings changed in another window. Close and reopen this list to review them.";
      return;
    }
    const next=normalize({mode:mode.value,entries:draft});
    try{localStorage.setItem(key,JSON.stringify(next));}catch{status.textContent="Could not save these display readings. Local storage is unavailable.";return;}
    saved=next;dialog.close();updated();
  };
  window.addEventListener("storage",e=>{if(e.key===key || e.key===null){saved=stored();updated();}});
  $("#desk-reading-title").textContent=saved.mode==="desk" ? "Display readings" : "Pinned readings";
  return {
    query(){
      if(saved.mode==="overview")return "";
      const params=new URLSearchParams({source:"desk"});for(const m of saved.entries)params.append("reading",m.id);
      return "?"+params;
    },
    label(m){
      if(m.known!==false || saved.mode!=="desk")return m;
      const cached=saved.entries.find(x=>x.id===m.id);return cached ? {...m,name:cached.name,unit:cached.unit} : m;
    },
    get emptyMessage(){return saved.mode==="desk" ? "Choose display readings with Edit." : "Pin a reading on the Overview to see it here.";},
  };
}
