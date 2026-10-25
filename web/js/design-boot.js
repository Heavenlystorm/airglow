// Runs before the stylesheet so the last design is on <html> for the first paint. A classic script, not a module:
// the CSS must already know the answer when it arrives. The server value still wins once boot() has fetched it.
(() => {
  try {
    const saved = localStorage.getItem("airglow.design");
    if (saved) document.documentElement.setAttribute("data-design", saved);
  } catch (e) {
    // Private mode or storage switched off: the page simply paints the default until the settings arrive.
  }
})();
