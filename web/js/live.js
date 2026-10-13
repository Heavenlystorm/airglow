// Live updates from Airglow over one server-sent event stream (/api/stream): app state every couple of seconds,
// and LED frames while a view shows the live preview. The stream closes while the window is hidden in the tray,
// and reopens with frames on or off as views change.

import { emit } from "./core.js";

let source = null, withFrames = null, handlers = null, wantsFrames = () => false;

function desired() {
  if (document.visibilityState === "hidden") return null;      // nothing to show: no stream at all
  return wantsFrames();
}

export function sync() {
  const want = desired();
  if (want === null) { close(); return; }
  if (source && want === withFrames) return;
  close();
  withFrames = want;
  source = new EventSource(`/api/stream?frames=${want ? 1 : 0}`);
  source.addEventListener("state", (e) => handlers.state(JSON.parse(e.data)));
  source.addEventListener("frames", (e) => emit("frames:live", JSON.parse(e.data)));
  source.onerror = () => handlers.lost();                        // EventSource retries on its own
}

function close() {
  if (source) { source.close(); source = null; }
  withFrames = null;
}

// state(data) gets every state update; lost() is called when the connection drops; frames() says whether any
// visible view wants LED frames right now.
export function startLive({ state, lost, frames }) {
  handlers = { state, lost };
  wantsFrames = frames;
  document.addEventListener("visibilitychange", sync);
  window.addEventListener("hashchange", () => setTimeout(sync, 0));
  sync();
}
