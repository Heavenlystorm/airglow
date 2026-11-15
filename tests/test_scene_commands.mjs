import test from "node:test";
import assert from "node:assert/strict";
import vm from "node:vm";
import fs from "node:fs";

async function fixture(post, get = async () => ({})) {
  const S = { state: { scene: { effect: "solid" }, scene_status: { desired_revision: 0, active_revision: 0, status: "ready" } } };
  const context = vm.createContext({ structuredClone, Date, Promise, Error, setTimeout });
  const core = new vm.SyntheticModule(["S", "get", "post", "emit", "settleSceneMutation"], function () {
    this.setExport("S", S); this.setExport("get", get); this.setExport("post", post); this.setExport("emit", () => {});
    this.setExport("settleSceneMutation", () => {});
  }, { context });
  const mod = new vm.SourceTextModule(fs.readFileSync(new URL("../web/js/scene-commands.js", import.meta.url), "utf8"), { context });
  await mod.link(() => core); await mod.evaluate();
  return { S, ...Object.fromEntries(Object.keys(mod.namespace).map(k => [k, mod.namespace[k]])) };
}
function reply(revision, effect, status = "ready") {
  return { scene: { effect }, scene_status: { desired_revision: revision, active_revision: status === "ready" ? revision : revision - 1, status } };
}

test("queued parameters remain bound to their original effect", async () => {
  const calls = [];
  const f = await fixture(async (_, body) => { calls.push(body); return reply(calls.length, body.effect || "comet"); });
  const select = f.applyLightingScene({ effect: "comet" });
  const stale = f.applyLightingScene({ params: { color: "#123456" } });
  await Promise.all([select, stale]);
  assert.equal(calls[1].expected_effect, "solid");
  assert.equal(calls[1].expected_revision, 1);
});

test("an old response cannot replace a newer state snapshot", async () => {
  let resolve;
  const f = await fixture(() => new Promise(r => { resolve = r; }));
  const pending = f.applyLightingScene({ effect: "comet" });
  await Promise.resolve();
  f.S.state.scene = { effect: "wave" }; f.S.state.scene_status = reply(2, "wave").scene_status;
  resolve(reply(1, "comet")); await assert.rejects(pending, /newer lighting change/);
  assert.equal(f.S.state.scene.effect, "wave");
});

test("waiting for readiness never queues Off behind a slow renderer", async () => {
  let revision = 0, polls = 0;
  const f = await fixture(async (_, body) => reply(++revision, "comet", body.on === false ? "ready" : "pending"),
    async () => { polls++; return reply(revision, "comet", revision === 1 ? "pending" : "ready"); });
  const starting = f.applyLightingScene({ effect: "comet" }, { wait: true });
  const observed = starting.catch(e => e);
  await new Promise(r => setTimeout(r, 5));
  const off = await f.applyLightingScene({ on: false });
  assert.equal(off.scene_status.desired_revision, 2);
  assert.match((await observed).message, /newer lighting change/);
  assert.ok(polls > 0);
});

test("a superseded ready Apply cannot report success to its audition", async () => {
  let resolve;
  const f = await fixture(() => new Promise(r => { resolve = r; }));
  const pending = f.applyLightingScene({ effect: "comet" }, { wait: true });
  await Promise.resolve();
  f.S.state.scene = { effect: "rainbow" }; f.S.state.scene_status = reply(2, "rainbow").scene_status;
  resolve(reply(1, "comet"));
  await assert.rejects(pending, /newer lighting change/);
});

test("an old process reply cannot restore state after a restart", async () => {
  let resolve;
  const f = await fixture(() => new Promise(r => { resolve = r; }));
  f.S.state.scene_status.session = "before-restart";
  const pending = f.applyLightingScene({ effect: "comet" }, { wait: true });
  await Promise.resolve();
  f.S.state.scene = { effect: "rainbow" };
  f.S.state.scene_status = { ...reply(0, "rainbow").scene_status, session: "after-restart" };
  const old = reply(1, "comet"); old.scene_status.session = "before-restart";
  resolve(old);
  await assert.rejects(pending, /restarted/);
  assert.equal(f.S.state.scene.effect, "rainbow");
  assert.equal(f.S.state.scene_status.session, "after-restart");
});

test("a stale ready poll cannot fulfill a superseded scene wait", async () => {
  let resolve;
  const f = await fixture(async () => ({}), () => new Promise(r => { resolve = r; }));
  const pending = f.waitForLightingScene(1);
  f.S.state.scene = { effect: "rainbow" }; f.S.state.scene_status = reply(2, "rainbow").scene_status;
  resolve(reply(1, "comet"));
  await assert.rejects(pending, /newer lighting change/);
  assert.equal(f.S.state.scene.effect, "rainbow");
});

test("pure Off accepts a restarted server's safety acknowledgement", async () => {
  const off = { scene: { effect: "solid", on: false }, scene_status: { ...reply(1, "solid").scene_status, session: "new" } };
  const f = await fixture(async () => off);
  f.S.state.scene_status.session = "old";
  const result = await f.applyLightingScene({ on: false }, { wait: true });
  assert.equal(result.scene.on, false);
  assert.equal(f.S.state.scene_status.session, "new");
});

test("an old renderer's ready reply cannot overwrite a newer failure at the same scene revision", async () => {
  let resolve;
  const f = await fixture(() => new Promise(r => { resolve = r; }));
  const pending = f.applyLightingScene({ effect: "comet" }, { wait: true });
  await Promise.resolve();
  f.S.state.scene_status = { ...reply(1, "comet", "failed").scene_status, renderer_revision: 5, error: "Renderer capacity changed" };
  const old = reply(1, "comet"); old.scene_status.renderer_revision = 4;
  resolve(old);
  await assert.rejects(pending, /Renderer capacity changed/);
  assert.equal(f.S.state.scene_status.renderer_revision, 5);
  assert.equal(f.S.state.scene_status.status, "failed");
});
