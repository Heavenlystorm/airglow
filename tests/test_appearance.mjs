import assert from "node:assert/strict";
import { test } from "node:test";
import { normalizeAppearance, parseLookRecipe, encodeLookRecipe, normalizeLooks,
  createAppearanceTransaction, serializeWrites, accentPalette, contrastRatio } from "../web/js/appearance.js";

const defaults = { accent: null, density: "comfortable", scale: 100, rig: "frame", motion: "system" };
const baseline = () => ({ design: "graphite", appearance: { ...defaults }, looks: [] });

test("bad stored preferences fall back without retaining foreign settings", () => {
  assert.deepEqual(normalizeAppearance({ accent: "url(evil)", density: "huge", scale: true,
    rig: "unknown", motion: "always", fan_failsafe_duty: 0 }), defaults);
  assert.deepEqual(normalizeAppearance(null), defaults);
  assert.deepEqual(normalizeAppearance({ accent: "#Ff00AA", scale: 120, motion: "reduced" }),
    { ...defaults, accent: "#ff00aa", scale: 120, motion: "reduced" });
});

test("recipes roundtrip only appearance and reject damaged or future files", () => {
  const recipe = { name: "My evening", design: "paper", appearance: { ...defaults, accent: "#20aa77" }, fan_failsafe_duty: 0 };
  const encoded = encodeLookRecipe(recipe);
  const decoded = parseLookRecipe(encoded);
  assert.deepEqual(decoded, { name: recipe.name, design: "paper", appearance: recipe.appearance });
  assert.ok(!encoded.includes("fan_failsafe_duty"));
  for (const patch of [{ version: 2 }, { design: "unknown" }, { appearance: { scale: 400 } },
    { appearance: { accent: "bad" } }, { appearance: [] }, { name: " " }]) {
    assert.throws(() => parseLookRecipe(JSON.stringify({ ...JSON.parse(encoded), ...patch })));
  }
  assert.throws(() => parseLookRecipe("{"));
  assert.throws(() => parseLookRecipe(" ".repeat(65537)));
});

test("saved looks are bounded, unique and detached from caller data", () => {
  const input = Array.from({ length: 14 }, (_, i) => ({ id: `look-${i}`, name: `Look ${i}`,
    design: "graphite", appearance: { accent: "#abcdef" } }));
  const result = normalizeLooks([input[0], input[0], { id: "BAD!", name: "Bad" }, ...input.slice(1)]);
  assert.equal(result.length, 12);
  assert.equal(new Set(result.map((l) => l.id)).size, 12);
  input[0].appearance.accent = "#000000";
  assert.equal(result[0].appearance.accent, "#abcdef");
});

test("saved-look IDs cannot be coerced from missing values or numbers", () => {
  const recipe = { name: "Valid name", design: "graphite", appearance: defaults };
  assert.deepEqual(normalizeLooks([{ ...recipe }, { ...recipe, id: 123 }, { ...recipe, id: "with_underscore" },
    { ...recipe, id: "control-name", name: "A\u0001B" }]), []);
});

test("cancel restores the exact original design and appearance without writing", () => {
  const applied = [], writes = [];
  const tx = createAppearanceTransaction(baseline(), { apply: (v) => applied.push(v), save: (v) => writes.push(v) });
  tx.preview({ design: "paper", appearance: { accent: "#00aa88", scale: 110 } });
  tx.cancel();
  tx.cancel();
  assert.deepEqual(applied.at(-1), baseline());
  assert.equal(writes.length, 0);
  assert.equal(tx.closed, true);
});

test("a failed Keep retains draft, while duplicate Keep shares one write", async () => {
  const requests = [];
  const tx = createAppearanceTransaction(baseline(), { apply() {}, save: (v) => new Promise((resolve, reject) => requests.push({ v, resolve, reject })) });
  tx.preview({ appearance: { ...defaults, density: "compact" } });
  const first = tx.keep();
  const duplicate = tx.keep();
  assert.equal(requests.length, 1);
  assert.equal(tx.saving, true);
  requests[0].reject(new Error("disk full"));
  await assert.rejects(first, /disk full/);
  await assert.rejects(duplicate, /disk full/);
  assert.equal(tx.closed, false);
  assert.equal(tx.draft.appearance.density, "compact");
  const retry = tx.keep();
  requests[1].resolve(requests[1].v);
  await retry;
  assert.equal(tx.closed, true);
});

test("queued writes cannot finish out of order and recover after failure", async () => {
  const requests = [];
  const write = serializeWrites((patch) => new Promise((resolve, reject) => requests.push({ patch, resolve, reject })));
  const patch = { design: "paper" };
  const first = write(patch);
  patch.design = "amber";
  const second = write({ design: "neon" });
  await Promise.resolve();
  assert.equal(requests.length, 1);
  assert.equal(requests[0].patch.design, "paper");
  requests[0].reject(new Error("failed"));
  await assert.rejects(first);
  await Promise.resolve();
  assert.equal(requests.length, 2);
  requests[1].resolve("neon");
  assert.equal(await second, "neon");
});

test("extreme accents retain readable labels and primary-button text", () => {
  for (const background of ["#1b1e21", "#000000", "#141414", "#f5f2eb", "#ded9cf"]) {
    for (const accent of ["#000000", "#ffffff", "#ffff00", "#0000ff", "#ff0000", "#dfb878"]) {
      const palette = accentPalette(accent, [background]);
      assert.ok(contrastRatio(palette.accent, background) >= 4.5, `${accent} on ${background}`);
      assert.ok(contrastRatio(palette.ink, palette.accent) >= 4.5);
      assert.ok(contrastRatio(palette.ink, palette.highlight) >= 4.5);
    }
  }
});
