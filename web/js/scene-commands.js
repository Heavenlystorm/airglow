// One ordered path for manual scene edits. Parameter editors bind their effect at input time.
import { S, get, post, emit, settleSceneMutation } from "./core.js";

let tail = Promise.resolve();
export function adoptSceneState(reply) {
  const status = reply.scene_status;
  const current = S.state?.scene_status;
  const sameSession = !status?.session || !current?.session || status.session === current.session;
  const oldRenderer = (status?.renderer_revision ?? 0) <= (current?.renderer_revision ?? 0);
  const staleRenderer = status?.renderer_revision != null && current?.renderer_revision != null &&
    status.desired_revision === current.desired_revision && status.renderer_revision < current.renderer_revision;
  if (sameSession && status && (status.desired_revision < (current?.desired_revision ?? -1) ||
      staleRenderer ||
      (status.status === "pending" && status.desired_revision === current?.active_revision && oldRenderer))) {
    return { ...reply, scene: S.state.scene, scene_status: current,
      effective: S.state.effective, active_preset: S.state.active_preset };
  }
  return reply;
}
function receive(incoming) {
  const reply = adoptSceneState(incoming), status = reply.scene_status;
  if (reply.scene) S.state.scene = reply.scene;
  if (status) S.state.scene_status = status;
  settleSceneMutation(status);
  emit("scene:status", status);
  return reply;
}

function checkedReply(incoming, session, allowServerRestart = false) {
  const current = S.state?.scene_status;
  const serverSession = incoming.scene_status?.session;
  if (session && ((current?.session && current.session !== session && serverSession !== current.session) ||
      (serverSession && serverSession !== session && !allowServerRestart)))
    throw new Error("Airglow restarted. Refresh before changing the lights again.");
  const reply = receive(incoming);
  if (incoming.scene_status && reply.scene_status?.desired_revision !== incoming.scene_status.desired_revision)
    throw new Error("A newer lighting change replaced this one");
  if (reply.scene_status?.status === "failed") throw new Error(reply.scene_status.error || "The lighting scene could not start");
  return reply;
}

export async function waitForLightingScene(revision, session = S.state?.scene_status?.session) {
  const deadline = Date.now() + 20000;
  while (Date.now() < deadline) {
    const reply = checkedReply(await get("/api/state"), session);
    const status = reply.scene_status;
    if (!status) return reply;
    if (status.desired_revision !== revision) throw new Error("A newer lighting change replaced this one");
    if (status.status === "failed") throw new Error(status.error || "The lighting scene could not start");
    if (status.status === "ready") return reply;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("The lighting scene is still starting. Check its status before trying again.");
}

export function applyLightingScene(patch, options = {}) {
  const body = structuredClone(patch);
  const pureOff = body.on === false && !["effect", "params", "brightness"].some(key => key in body);
  if (body.params && body.expected_effect == null) body.expected_effect = S.state?.scene?.effect;
  const run = async () => {
    const revision = S.state?.scene_status?.desired_revision;
    if (body.expected_revision == null && revision != null) body.expected_revision = revision;
    if (body.expected_session == null && S.state?.scene_status?.session) body.expected_session = S.state.scene_status.session;
    const incoming = await post("/api/scene", body);
    // A later accepted response or SSE snapshot always wins.
    return checkedReply(incoming, body.expected_session, pureOff);
  };
  const result = tail.then(run);
  tail = result.then(() => {}, () => {});
  // Readiness polling never blocks a later selection or Off command.
  return options.wait ? result.then((reply) => reply.scene_status?.status === "pending"
    ? waitForLightingScene(reply.scene_status.desired_revision, reply.scene_status.session) : reply) : result;
}

export function undoLightingScene(options = {}) {
  const body = { expected_revision: options.expected_revision ?? S.state?.scene_status?.desired_revision,
    expected_session: options.expected_session ?? S.state?.scene_status?.session };
  const result = tail.then(async () => {
    return checkedReply(await post("/api/scene/undo", body), body.expected_session);
  });
  tail = result.then(() => {}, () => {});
  return result.then((reply) => reply.scene_status?.status === "pending"
    ? waitForLightingScene(reply.scene_status.desired_revision, reply.scene_status.session) : reply);
}

