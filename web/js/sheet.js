// One reusable modal sheet for dialogs and the first-run guide.
import { $ } from "./core.js";

let wired = false;

export function openSheet(html) {
  const d = $("#sheet");
  d.innerHTML = html;
  const h = d.querySelector("h2");
  if (h) {
    if (!h.id) h.id = "sheet-title";
    d.setAttribute("aria-labelledby", h.id);
  } else {
    d.removeAttribute("aria-labelledby");
  }
  if (!wired) {
    wired = true;
    d.addEventListener("keydown", (e) => {
      if (e.key !== "Tab") return;
      const items = [...d.querySelectorAll("button, [href], input, select, textarea, [tabindex]:not([tabindex='-1'])")]
        .filter((el) => !el.disabled && el.getClientRects().length);
      if (!items.length) return;
      const first = items[0], last = items.at(-1);
      if (!d.contains(document.activeElement)) { e.preventDefault(); first.focus(); }
      else if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    });
  }
  if (!d.open) d.showModal();
  d.querySelectorAll("[data-close]").forEach((b) => b.addEventListener("click", closeSheet));
  return d;
}

export function closeSheet() {
  const d = $("#sheet");
  if (d.open) d.close();
}
