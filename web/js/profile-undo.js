// Applying a profile, and the Undo that puts back what ran before it.
import { S, emit, fail, get, post, toast } from "./core.js";
import { applyLightingScene, waitForLightingScene } from "./scene-commands.js";

// The curve a profile switches the fans to, as the controller names it (None leaves the fans alone).
const profileCurve = (preset) => preset?.fan_points?.length ? `profile:${preset.id}` : preset?.fan_curve || null;

// Apply a profile and return { reply, undo }. undo() puts back what ran before: the previous profile, or the previous
// lighting and fan curve when no profile was active. A FanControl profile changes another program, so it gets none.
export async function applyProfile(preset, { reviewed = false, reviewedCurvePoints } = {}) {
  const undo = profileUndoPoint(preset);
  const body = reviewed ? { reviewed: preset, reviewed_curve_points: reviewedCurvePoints } : {};
  const reply = await post(`/api/presets/${encodeURIComponent(preset.id)}/apply`, body);
  if (reply.scene_status?.status === "pending")
    reply.scene_status = (await waitForLightingScene(reply.scene_status.desired_revision, reply.scene_status.session)).scene_status;
  return { reply, undo: undo() };
}

// Remember what is running now, before a profile replaces it. Call the result after the profile applied.
export function profileUndoPoint(preset) {
  const before = { preset: S.state?.active_preset ?? null, curve: S.state?.fans?.base_curve ?? null,
    scene: S.state?.scene ? structuredClone(S.state.scene) : null };
  return () => profileUndo(preset, before);
}

function profileUndo(preset, before) {
  if (preset.fan_profile || before.preset === preset.id) return null;
  const previous = before.preset ? S.presets.find((p) => p.id === before.preset) : null;
  if (before.preset && (!previous || previous.fan_profile)) return null;
  const namedCurve = (name) => !!name && !!S.curves?.curves && name in S.curves.curves;
  return async () => {
    if (previous) {
      // Restore the fans first: applying a profile without its own curve keeps whatever curve is running.
      if (!profileCurve(previous) && namedCurve(before.curve))
        await post("/api/fan_curve", { name: before.curve });
      const reply = await post(`/api/presets/${encodeURIComponent(previous.id)}/apply`);
      if (reply.scene_status?.status === "pending") await waitForLightingScene(reply.scene_status.desired_revision, reply.scene_status.session);
      return previous.name;
    }
    // No profile was running: put the earlier look back exactly, per-area effects included (null clears the
    // profile's), checked against the scene the profile left so a change made since is never overwritten.
    if (before.scene) {
      const now = (await get("/api/state")).scene_status;
      const { effect, params, brightness, on, areas } = before.scene;
      await applyLightingScene({ effect, params, brightness, on, areas: areas ?? null, expected_effect: preset.effect,
        expected_revision: now?.desired_revision, expected_session: now?.session }, { wait: true });
    }
    if (namedCurve(before.curve)) await post("/api/fan_curve", { name: before.curve });
    return null;
  };
}

// The toast after a profile is applied, with Undo when there is something to go back to.
export function profileToast(preset, { reply, undo }) {
  const failed = (reply.steps || []).filter((s) => !s.ok);
  if (failed.length) return toast(`${preset.name} is on, but ${failed[0].step} didn't apply: ${failed[0].error}`, true);
  const message = reply.scene_status?.overridden ? `${preset.name} saved. An automation controls the lights.` : `${preset.name} is on`;
  if (!undo || reply.scene_status?.overridden) return toast(message);
  toast(message, { action: undoAction(undo) });
}

// A toast action that runs a profile undo and says what came back.
export function undoAction(undo) {
  return { label: "Undo", onClick: async () => {
    try {
      const name = await undo();
      emit("refresh");
      toast(name ? `${name} is back on` : "Previous lighting and fans restored");
    } catch (err) { fail(err); emit("refresh"); }
  } };
}
