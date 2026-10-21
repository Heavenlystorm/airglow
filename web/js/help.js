// The keyboard reference: one place that lists the shortcuts already built into Airglow.
import { $ } from "./core.js";
import { openSheet } from "./sheet.js";

const ROWS = [
  [`<kbd>Ctrl</kbd> + <kbd>1</kbd>…<kbd>7</kbd>`, "Switch between the seven sections"],
  [`<kbd>Ctrl</kbd> + <kbd>S</kbd>`, "Save the effect you are editing in the Studio"],
  [`<kbd>?</kbd> or <kbd>F1</kbd>`, "Open this list"],
  [`<kbd>Esc</kbd>`, "Close a dialog, or leave Arrange and Customize"],
  [`<kbd>↑</kbd> <kbd>↓</kbd> <kbd>←</kbd> <kbd>→</kbd>`, "Fine-tune a selected fan-curve point. In Customize, move the tile; Shift + arrows resize it"],
  [`<kbd>Delete</kbd>`, "Remove the selected fan-curve point or Overview tile"],
  [`<kbd>Enter</kbd> / <kbd>Space</kbd>`, "Select a lighting zone, a layer, or a focused control"],
];

export function openShortcuts() {
  openSheet(`<div class="body"><h2>Keyboard shortcuts</h2>
    <table class="shortcuts"><tbody>${ROWS.map(([keys, what]) => `<tr><td>${keys}</td><td>${what}</td></tr>`).join("")}</tbody></table>
    <p class="hint">Shortcuts work when focus is not inside a text field. Escape always closes a dialog first.</p>
    <div class="actions"><span class="spacer"></span><button class="btn primary" data-close>Done</button></div></div>`);
}

export function initShortcuts() {
  document.addEventListener("keydown", (e) => {
    if (e.key !== "?" && e.key !== "F1") return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if ($("#sheet").open) return;
    const t = e.target;
    if (t?.matches?.("input, textarea, select, [contenteditable=true]") || t?.closest?.(".CodeMirror")) return;
    e.preventDefault();
    openShortcuts();
  });
}
