// The interface designs Settings offers, and the one place that puts one on the document.
// The ids must match DESIGNS in airglow/config.py; tests/test_designs.py checks that they do.

export const DESIGNS = [
  { id: "graphite", name: "Graphite", blurb: "The bench instrument: graphite faceplates and one brass accent." },
  { id: "ascii", name: "ASCII", blurb: "A P1 phosphor serial console: monospace, character frames, block meters." },
  { id: "paper", name: "Paper", blurb: "A service manual in daylight: warm paper, graphite ink, hairline panels." },
  { id: "amber", name: "Amber CRT", blurb: "A P3 amber display: scanlines, square corners, glow on the numbers only." },
  { id: "contrast", name: "High contrast", blurb: "Black and white with a warning yellow, 2px rules and thick focus rings." },
  { id: "neon", name: "Neon", blurb: "Neon tube signage: flat ink-blue panels, magenta accents, cyan data." },
  { id: "kiln", name: "Kiln", blurb: "A kiln door open a crack: near-black, cream serif figures, heat in the curves." },
  { id: "lumen", name: "Lumen", blurb: "A night console with a literary voice: violet-black, lowercase serif heads, brass." },
  { id: "midnight", name: "Midnight", blurb: "Midnight blue to every edge: wide light heads, square surfaces, one azure signal." },
  { id: "spore", name: "Spore", blurb: "A cellar at night: coffee-black, parchment serif, grained wells, oyster-pink fill." },
  { id: "rim", name: "Rim", blurb: "An instrument at night: warm black, hairline rules, every measurement in mono." },
  { id: "darkroom", name: "Darkroom", blurb: "A print room unlit: warm black, hairline condensed capitals, one amber safelight." },
];

export const DESKTOP = "graphite";
const KEY = "airglow.design";

export const isDesign = (id) => DESIGNS.some((d) => d.id === id);
export const currentDesign = () => document.documentElement.dataset.design || DESKTOP;

// Apply a design for this session and remember it for the pre-paint script. Unknown ids fall back to Graphite:
// the CSS then paints the default rather than a half-styled page.
export function applyDesign(id) {
  const chosen = isDesign(id) ? id : DESKTOP;
  document.documentElement.dataset.design = chosen;
  try { localStorage.setItem(KEY, chosen); } catch (e) { /* storage off: the server value still applies next boot */ }
  return chosen;
}
