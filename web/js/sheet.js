// One reusable modal sheet for dialogs and the first-run guide.
import { $ } from "./core.js";

export function openSheet(html) {
  const d = $("#sheet");
  d.innerHTML = html;
  if (!d.open) d.showModal();
  d.querySelectorAll("[data-close]").forEach((b) => b.addEventListener("click", closeSheet));
  return d;
}

export function closeSheet() {
  const d = $("#sheet");
  if (d.open) d.close();
}
