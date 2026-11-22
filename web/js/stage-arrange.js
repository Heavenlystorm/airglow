import { esc } from "./core.js";

// The live Lighting inspector stays mounted while the layout inspector is open.
export function createArrangePanel(view, action) {
  const el = document.createElement("aside");
  el.className = "arrange-panel"; el.hidden = true;
  el.setAttribute("aria-labelledby", "arrange-title");
  el.innerHTML = `<header><h2 id="arrange-title">Arrange rig</h2>
    <p class="hint">Select zones below or Shift-click the stage to move them together.</p></header>
    <fieldset class="arrange-zones"><legend>Zones</legend><div class="arrange-picks"></div></fieldset>
    <div class="arrange-coords" aria-label="Selected zone position"></div>
    <div class="arrange-nudge"><label for="arrange-step">Nudge</label>
      <select id="arrange-step" class="select" aria-label="Nudge distance"><option value=".001">Fine · 0.001</option><option selected value=".01">Normal · 0.01</option><option value=".05">Coarse · 0.05</option></select>
      <div role="group" aria-label="Move selection"><button type="button" class="chip-btn" data-act="left" aria-label="Nudge left">←</button>
        <button type="button" class="chip-btn" data-act="up" aria-label="Nudge up">↑</button>
        <button type="button" class="chip-btn" data-act="down" aria-label="Nudge down">↓</button>
        <button type="button" class="chip-btn" data-act="right" aria-label="Nudge right">→</button></div></div>
    <div class="arrange-group" role="group" aria-label="Orientation">
      <button type="button" class="btn quiet" data-act="backturn" aria-label="Rotate 15 degrees anticlockwise">↶ 15°</button>
      <button type="button" class="btn quiet" data-act="turn" aria-label="Rotate 15 degrees clockwise">↷ 15°</button>
      <label class="arrange-snap"><input type="checkbox" data-snap> Snap drag to grid</label></div>
    <fieldset class="arrange-align"><legend>Align centres</legend>
      <div><button type="button" class="btn" data-act="row">Make a row</button><button type="button" class="btn" data-act="column">Make a column</button>
      <button type="button" class="btn" data-act="space-x">Space across</button><button type="button" class="btn" data-act="space-y">Space down</button></div></fieldset>
    <details class="arrange-help"><summary>Placement shortcuts</summary><p>Tab to a zone. Press Space to add or remove it, then use the arrow keys to nudge. Hold Shift with an arrow for a 0.001 step. Ctrl+Z undoes an edit and Ctrl+Shift+Z redoes it.</p></details>
    <div class="arrange-history"><button type="button" class="text-link" data-act="undo">Undo</button><button type="button" class="text-link" data-act="redo">Redo</button>
      <span class="arrange-count"></span></div>
    <p class="arrange-note" role="status"></p>
    <footer><button type="button" class="btn quiet" data-act="cancel">Cancel</button><button type="button" class="btn primary" data-act="save">Save layout</button></footer>`;
  view.append(el);
  const find = sel => el.querySelector(sel), picks = find(".arrange-picks"), note = find(".arrange-note");
  let signature = "";
  el.addEventListener("click", e => { const button = e.target.closest("[data-act]"); if (button) action(button.dataset.act); });
  el.addEventListener("change", e => {
    if (e.target.matches("[data-key]")) action("toggle", e.target.dataset.key);
    if (e.target.matches("[data-snap]")) action("snap", e.target.checked);
  });
  return {
    element: el,
    step: () => Number(find("#arrange-step").value),
    message(value) { note.textContent = value; },
    render(draft, zones, dragging) {
      const open = !!draft; view.classList.toggle("arrange-edit", open);
      el.hidden = !open;
      if (!open) { note.textContent = ""; signature = ""; return; }
      const ids = JSON.stringify(zones.map(z => [z.key, z.config.name, z.device?.name, z.device.online]));
      if (ids !== signature) {
        const focusKey = picks.contains(document.activeElement) ? document.activeElement.dataset.key : null;
        signature = ids;
        picks.innerHTML = zones.length ? zones.map(z => `<label><input type="checkbox" data-key="${esc(z.key)}"><span>${esc(z.config.name)}
          <small>${esc(z.device.name || "")}${z.device.online ? "" : " · Offline"}</small></span></label>`).join("") :
          '<p class="hint">Connect a light to arrange its position.</p>';
        if (focusKey) [...picks.querySelectorAll("input")].find(b => b.dataset.key === focusKey)?.focus({ preventScroll: true });
      }
      const online = new Set(zones.filter(z => z.device.online).map(z => z.key));
      const active = zones.filter(z => online.has(z.key) && draft.selected.has(z.key));
      const n = active.length;
      for (const input of picks.querySelectorAll("input")) {
        input.checked = draft.selected.has(input.dataset.key);
        input.disabled = draft.busy || !online.has(input.dataset.key);
      }
      const coords = find(".arrange-coords");
      if (n === 1) {
        const p = dragging?.get(active[0].key) || draft.pose(active[0]);
        coords.innerHTML = `<span>X <b>${p.x.toFixed(3)}</b></span><span>Y <b>${p.y.toFixed(3)}</b></span><span>Angle <b>${p.angle}°</b></span>`;
      } else coords.textContent = n ? `${n} zones selected` : "Select one or more zones";
      const amount = draft.changed.size;
      find(".arrange-count").textContent = amount ? `${amount} edited zone${amount === 1 ? "" : "s"}` : "No changes";
      for (const b of el.querySelectorAll("[data-act]")) {
        const k = b.dataset.act;
        const available = k === "cancel" || (k === "save" ? amount : k === "undo" ? draft.past.length : k === "redo" ? draft.future.length :
          k.startsWith("space-") ? n >= 3 : ["row","column"].includes(k) ? n >= 2 : n > 0);
        b.disabled = draft.busy || !available;
      }
      const snap = find("[data-snap]"); snap.checked = draft.snap; snap.disabled = draft.busy;
      find("#arrange-step").disabled = draft.busy;
      find('[data-act="save"]').textContent = draft.busy ? "Saving…" : "Save layout";
    },
  };
}
