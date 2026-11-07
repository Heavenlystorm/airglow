// Runs before the stylesheet so the last design is on <html> for the first paint. A classic script, not a module:
// the CSS must already know the answer when it arrives. The server value still wins once boot() has fetched it.
(() => {
  try {
    const saved = localStorage.getItem("airglow.design");
    if (saved) document.documentElement.setAttribute("data-design", saved);
    const appearance = JSON.parse(localStorage.getItem("airglow.appearance") || "{}");
    if (appearance && typeof appearance === "object") {
      const root = document.documentElement;
      root.dataset.density = appearance.density === "compact" ? "compact" : "comfortable";
      root.dataset.rig = appearance.rig === "minimal" ? "minimal" : "frame";
      root.dataset.motion = appearance.motion === "reduced" ? "reduced" : "system";
      const scale = [90, 100, 110, 120].includes(appearance.scale) ? appearance.scale : 100;
      root.dataset.scale = String(scale);
      root.style.setProperty("--ui-scale", String(scale / 100));
    }
  } catch (e) {
    // Private mode or storage switched off: the page simply paints the default until the settings arrive.
  }
})();
