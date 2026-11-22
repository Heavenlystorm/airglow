import { $, esc, post, setHTML } from "./core.js";
import { elapsedText } from "./session-traces.js";

const drafts = new Map();
const runName = r => r?.name || r?.annotation || "Untitled workload";
const textOf = r => ({name: r.name || "", notes: r.notes || ""});
const same = (a, b) => a.name === b.name && a.notes === b.notes;
const identity = () => [...crypto.getRandomValues(new Uint8Array(16))].map(n => n.toString(16).padStart(2, "0")).join("");

export const liveMarkerMarkup = `<div class="session-live-marker" data-live-marker hidden>
  <label for="live-marker-label">Mark an event in the running recording</label>
  <div class="session-mark-row"><input id="live-marker-label" maxlength="120" placeholder="e.g. Launched game"><button type="button" class="btn" data-mark-now>Mark now</button></div>
  <p class="hint" data-live-marker-count></p></div>`;

export const notebookMarkup = `<aside class="session-notebook" data-session-notebook hidden>
  <h3>Run notes</h3><p data-notes-run></p><div data-notes-editor>
  <details class="session-notes-details"><summary>Edit name and notes</summary><div class="session-notes-fields">
    <label for="session-name">Recording name</label><input id="session-name" maxlength="80" placeholder="Use original workload label">
    <label for="session-notes">Notes</label><textarea id="session-notes" rows="4" maxlength="4000" placeholder="Workload, conditions or observations"></textarea>
    <div data-note-conflict hidden><p>Saved text changed in another window.</p><pre data-latest-note></pre>
      <div class="btn-row"><button type="button" class="btn quiet" data-note-reload>Use saved text</button><button type="button" class="btn quiet" data-note-rebase>Keep my edits</button></div></div>
    <button type="button" class="btn" data-note-save>Save name and notes</button>
  </div></details><p class="session-note-preview" data-notes-preview></p></div>
  <div class="session-event-nav"><label for="session-events">Event markers</label><select id="session-events"></select>
    <p data-event-detail class="hint"></p><div class="btn-row"><button type="button" class="btn quiet" data-event-jump>Go to event</button>
      <button type="button" class="btn quiet" data-event-remove>Remove event</button></div>
    <div class="session-event-confirm" data-event-confirm hidden><span>Remove this event from Run A?</span><div class="btn-row"><button type="button" class="btn danger" data-event-delete>Remove</button><button type="button" class="btn quiet" data-event-keep>Keep event</button></div></div>
  </div>
  <div class="session-add-event" data-place-group hidden><label for="session-place-label">Mark the inspected time in Run A</label>
    <div class="session-mark-row"><input id="session-place-label" maxlength="120" placeholder="Event label"><button type="button" class="btn quiet" data-event-place>Add event</button></div>
    <p class="hint" data-place-time></p></div>
</aside><p class="session-write-status" role="status" data-notebook-status></p>`;

export function mountNotebook(root, { current, refresh, seek }) {
  const name = $("#session-name", root), notes = $("#session-notes", root), events = $("#session-events", root);
  const liveLabel = $("#live-marker-label", root), placeLabel = $("#session-place-label", root);
  const status = $("[data-notebook-status]", root);
  let primary = null, comparison = null, active = null, paused = false, busy = false, entries = [], inspected = null;
  let pending = null, confirm = "", lastRun = "", note = null, connected = true;
  const getEntry = () => entries.find(e => e.key === events.value);
  function controls() {
    const entry = getEntry(), blocked = busy || !connected;
    $("[data-mark-now]", root).disabled = blocked || paused || !active || !liveLabel.value.trim() || (active.marker_count || 0) >= 64;
    $("[data-event-place]", root).disabled = blocked || !primary?.ended_reason || inspected === null || inspected > primary.duration_seconds
      || !placeLabel.value.trim() || (primary.markers?.length || 0) >= 64;
    $("[data-note-save]", root).disabled = blocked || !note || same(note, note.base) || !same(note.base, textOf(primary));
    $("[data-event-jump]", root).disabled = !entry;
    $("[data-event-remove]", root).disabled = blocked || !entry || entry.run !== primary;
    $("[data-event-delete]", root).disabled = blocked || !entry || entry.key !== confirm;
    const conflict = !!(note && !same(note.base, textOf(primary)) && !same(note, note.base));
    $("[data-note-conflict]", root).hidden = !conflict;
    if (conflict) $("[data-latest-note]", root).textContent = [runName(primary), primary.notes].filter(Boolean).join("\n");
    $("[data-event-detail]", root).textContent = entry ? `${entry.marker.kind === "live" ? "Marked during recording" : "Placed after recording"} · ${elapsedText(entry.marker.elapsed, true)}` : "";
  }
  function updateRuns(a, b) {
    primary = a; comparison = b;
    $("[data-session-notebook]", root).hidden = !a;
    const id = a?.id || "", before = events.value;
    if (id !== lastRun) { confirm = ""; placeLabel.value = ""; lastRun = id; status.textContent = ""; }
    $("[data-event-confirm]", root).hidden = !confirm;
    note = null;
    if (a) {
      $("[data-notes-run]", root).textContent = `Run A · ${runName(a)}`;
      $("[data-notes-editor]", root).hidden = !a.ended_reason;
      $("[data-place-group]", root).hidden = !a.ended_reason;
      $("[data-notes-preview]", root).textContent = a.notes || "";
      if (a.ended_reason) {
        const saved = textOf(a);
        note = drafts.get(id);
        if (!note || same(note, note.base) || same(note, saved)) {
          note = {...saved, base: saved}; drafts.set(id, note);
        }
        if (name.value !== note.name) name.value = note.name;
        if (notes.value !== note.notes) notes.value = note.notes;
      }
    }
    entries = [primary, comparison].flatMap((run, index) => (run?.markers || []).map(marker => ({
      key: JSON.stringify([run.id, marker.id]), run, index, marker,
    }))).sort((a,b) => a.marker.elapsed - b.marker.elapsed || a.index - b.index);
    setHTML(events, `<option value="">${entries.length ? "Choose an event" : "No event markers"}</option>` + entries.map(e =>
      `<option value="${esc(e.key)}">${e.index ? "B" : "A"} · ${elapsedText(e.marker.elapsed, true)} · ${esc(e.marker.label)}</option>`).join(""));
    events.value = entries.some(e => e.key === before) ? before : "";
    if (confirm && events.value !== confirm) { confirm = ""; $("[data-event-confirm]", root).hidden = true; }
    events.disabled = !entries.length;
    controls();
  }
  function selectMarker(key) {
    const entry = entries.find(e => e.key === key);
    if (!entry) return;
    events.value = key; confirm = ""; $("[data-event-confirm]", root).hidden = true;
    seek(entry.marker.elapsed); controls();
  }
  async function write(operation, complete) {
    if (busy || !connected) return;
    busy = true; status.textContent = "Saving…"; controls();
    try {
      const result = await operation();
      const message = complete(result);
      if (current()) { await refresh(true); if (current()) status.textContent = message; }
    } catch (error) {
      if (current()) { await refresh(true); if (current()) status.textContent = error.message || "Could not save. Try again."; }
    } finally { busy = false; if (current()) controls(); }
  }
  function mark(live) {
    const run = live ? active : primary, input = live ? liveLabel : placeLabel;
    if (!run || !input.value.trim()) return;
    const label = input.value, time = live ? null : inspected;
    const signature = JSON.stringify([run.id, label, time]);
    if (pending?.signature !== signature) pending = {signature, id: identity()};
    const markerId = pending.id;
    write(() => post("/api/cooling/mark", {id: run.id, marker_id: markerId, label, ...(live ? {} : {elapsed: time})}), result => {
      const marker = result.markers.find(m => m.id === markerId);
      if (pending?.id === markerId) pending = null;
      if (input.value === label && (live ? active?.id : primary?.id) === run.id) input.value = "";
      return marker ? `Event saved at ${elapsedText(marker.elapsed, true)}.` : "Event saved.";
    });
  }
  for (const input of [name, notes]) input.addEventListener("input", () => {
    if (!note) return;
    note.name = name.value; note.notes = notes.value; status.textContent = "Unsaved text in this window."; controls();
  });
  for (const [input, live] of [[liveLabel,true],[placeLabel,false]]) {
    input.addEventListener("input", controls);
    input.addEventListener("keydown", e => {
      if (e.key === "Enter" && !e.isComposing && !$(live ? "[data-mark-now]" : "[data-event-place]", root).disabled) {
        e.preventDefault(); mark(live);
      }
    });
  }
  events.addEventListener("change", () => {
    confirm = ""; $("[data-event-confirm]", root).hidden = true;
    if (events.value) selectMarker(events.value); else controls();
  });
  root.addEventListener("click", e => {
    const b = e.target.closest("button");
    if (!b || b.disabled) return;
    if (b.matches("[data-mark-now], [data-event-place]")) return mark(b.matches("[data-mark-now]"));
    if (b.matches("[data-event-jump]")) return selectMarker(events.value);
    if (b.matches("[data-event-remove]")) {
      confirm = events.value; $("[data-event-confirm]", root).hidden = false; controls(); $("[data-event-keep]", root).focus(); return;
    }
    if (b.matches("[data-event-keep]")) {
      confirm = ""; $("[data-event-confirm]", root).hidden = true; $("[data-event-remove]", root).focus(); return;
    }
    if (b.matches("[data-event-delete]")) {
      const entry = getEntry();
      if (!entry || entry.key !== confirm || entry.run !== primary) return;
      write(() => post("/api/cooling/unmark", {id: entry.run.id, marker_id: entry.marker.id}), () => {
        confirm = ""; if (current()) { $("[data-event-confirm]", root).hidden = true; events.focus(); } return "Event removed.";
      }); return;
    }
    if (b.matches("[data-note-reload], [data-note-rebase]") && note) {
      const saved = textOf(primary); note.base = saved;
      if (b.matches("[data-note-reload]")) Object.assign(note, saved);
      updateRuns(primary, comparison); status.textContent = b.matches("[data-note-reload]") ? "Loaded saved text." : "Review your edits, then save."; return;
    }
    if (b.matches("[data-note-save]") && note && primary) {
      const id = primary.id, original = note, edited = {name: note.name, notes: note.notes};
      write(() => post("/api/cooling/annotate", {id, ...edited, expected: original.base}), result => {
        const saved = textOf(result);
        original.base = saved;
        if (same(original, edited)) Object.assign(original, saved);
        return "Recording name and notes saved.";
      });
    }
  });
  return {
    updateRuns, selectMarker,
    setTime(time) {
      inspected = Number.isFinite(time) ? time : null;
      const available = primary && inspected !== null && inspected <= primary.duration_seconds;
      $("[data-place-time]", root).textContent = available ? `Selected time · ${elapsedText(inspected, true)}` : "Choose a time within Run A.";
      controls();
    },
    setRecorder(run, locked, online) {
      if (active?.id !== run?.id) liveLabel.value = "";
      active = run; paused = locked; connected = online;
      $("[data-live-marker]", root).hidden = !run;
      $("[data-live-marker-count]", root).textContent = run ? `${runName(run)} · ${run.marker_count || 0} / 64 markers` : "";
      controls();
    },
    retain(ids) { for (const id of drafts.keys()) if (!ids.includes(id)) drafts.delete(id); },
  };
}
