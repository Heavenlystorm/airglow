import { $, fail, post } from "./core.js";

export async function openDeskDisplay() {
  const result = await post("/api/desk-display/open", {});
  if (!result.native) location.assign("/desk.html");
}

export const workspaceTools = [
  { id: "desk-display", name: "Desk Display", detail: "Clock, focus timer and pinned readings" },
  { id: "recordings", name: "Recordings", detail: "Record and compare sensor sessions" },
  { id: "profile-library", name: "Profile library", detail: "Preview, compare and import saved looks" },
  { id: "lookbook", name: "Lighting lookbook", detail: "Explore and remix six included lighting recipes" },
  { id: "wind-down", name: "Wind down", detail: "Open the lighting fade timer or release its hold" },
  { id: "control-center", name: "Control center", detail: "Check sensors, lighting and device status" },
  { id: "help", name: "Airglow help", detail: "Task guide, first steps and keyboard shortcuts" },
];

export async function openWorkspaceTool(id) {
  if (id === "desk-display") return openDeskDisplay();
  if (id === "profile-library") { const m = await import("./profile-library.js"); return m.openProfileLibrary(); }
  if (id === "lookbook") { const m = await import("./lookbook.js"); return m.openLookbook(); }
  if (id === "wind-down") { const m = await import("./wind-down.js"); return m.openWindDown(); }
  if (id === "recordings") { const m = await import("./session-lab.js"); return m.openSessionLab(); }
  if (id === "control-center") { const m = await import("./control-center.js"); return m.openControlCenter(); }
  if (id === "help") { const m = await import("./help.js"); return m.openGuide(); }
}

export function initWorkspaceTools() {
  const menu = $(".ov-more"), toggle = $("summary", menu);
  document.addEventListener("click", e => {
    if (e.target.closest("[data-profile-library]")) openWorkspaceTool("profile-library").catch(fail);
    if (e.target.closest("[data-desk-display]")) openDeskDisplay().catch(fail);
    if (menu.open && (!menu.contains(e.target) || e.target.closest(".ov-more-panel button"))) {
      menu.open = false;
      if (menu.contains(document.activeElement)) toggle.focus({ preventScroll: true });
    }
  });
  menu.addEventListener("keydown", e => {
    if (menu.open && e.key === "Escape") { e.preventDefault(); menu.open = false; toggle.focus(); }
    if (e.target === toggle && e.key === "ArrowDown") {
      e.preventDefault(); menu.open = true; $("button", menu).focus();
    }
  });
}
