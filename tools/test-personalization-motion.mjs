import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import vm from "node:vm";

test("personal reduced-motion setting suppresses transitions and cannot override the OS preference", async () => {
  const mq = { matches: false };
  const root = { dataset: { motion: "system" } };
  const context = vm.createContext({ matchMedia: () => mq, document: { documentElement: root } });
  const mod = new vm.SourceTextModule(readFileSync(new URL("../web/js/motion.js", import.meta.url), "utf8"), { context });
  await mod.link(() => { throw Error("Unexpected dependency"); });
  await mod.evaluate();
  let transitions = 0;
  const el = { animate() { transitions++; } };
  mod.namespace.enter(el);
  assert.equal(transitions, 1);
  root.dataset.motion = "reduced";
  mod.namespace.enter(el);
  assert.equal(transitions, 1, "explicit Reduce motion must suppress the entrance");
  root.dataset.motion = "system";
  mq.matches = true;
  mod.namespace.enter(el);
  assert.equal(transitions, 1, "system Reduce motion remains respected");
  mq.matches = false;
  mod.namespace.enter(el);
  assert.equal(transitions, 2);
});

test("scaled interface keeps tab markers and FLIP motion in local CSS coordinates", async () => {
  const root = { dataset: { motion: "system", scale: "120" } };
  const context = vm.createContext({ matchMedia: () => ({ matches: false }), document: { documentElement: root } });
  const mod = new vm.SourceTextModule(readFileSync(new URL("../web/js/motion.js", import.meta.url), "utf8"), { context });
  await mod.link(() => { throw Error("Unexpected dependency"); });
  await mod.evaluate();
  for (const scale of [90, 100, 120]) {
    root.dataset.scale = String(scale);
    const zoom = scale / 100;
    const marker = { parentElement: { getBoundingClientRect: () => ({ left: 200 * zoom }) }, style: {} };
    mod.namespace.moveMarker(marker, { getBoundingClientRect: () => ({ left: 300 * zoom, width: 80 * zoom }) }, true);
    assert.equal(marker.style.transform, "translateX(112px)");
    assert.equal(marker.style.width, "56px");
    let moved = false, animation;
    const el = { getBoundingClientRect: () => ({ left: (moved ? 100 : 0) * zoom, top: (moved ? 40 : 0) * zoom, width: 100, height: 80 }), animate: frames => { animation = frames; } };
    mod.namespace.flip({ children: [el] }, () => { moved = true; });
    assert.equal(animation[0].transform, "translate(-100px, -40px) scale(1, 1)");
  }
});
