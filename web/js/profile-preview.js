import { PreviewRunner } from "./fxpreview.js";
import { ledPoints, sampleZones, testAudio, zoneFrames } from "./fxrun.js";
import { litZones, paintRig } from "./stage.js";

// A single composed scene can use every Studio slot. Keep the next portrait, including a
// replacement sheet's first one, behind the previous lease's acknowledged release.
let previous = Promise.resolve();

export function mountProfileFrames(host, profiles, seconds) {
  const connected = litZones(), sample = connected.length === 0;
  const zones = sample ? sampleZones() : connected;
  const audio = testAudio(seconds);
  const jobs = [], panels = host.querySelectorAll(".pl-portrait");
  let stopped = false, finishCurrent, geometry;
  host.querySelector("[data-rig-source]").textContent = `${sample ? "Sample rig" : "Your rig"} · simulated inputs`;
  const resize = new ResizeObserver(() => {
    if (stopped) return;
    for (const job of jobs) {
      if (job.frames) paintRig(job.canvas, job.frames, { zones, pad: 15 });
    }
  });

  async function portrait(job) {
    if (stopped) return;
    geometry ||= ledPoints(zones);
    const { points, slices } = geometry;
    let finish;
    const finished = new Promise(resolve => { finish = resolve; });
    finishCurrent = finish;
    const fail = message => {
      if (!stopped) {
        job.status.textContent = message;
        job.panel.dataset.frame = "error";
      }
      finish();
    };
    const runner = new PreviewRunner({
      onReady: () => runner.tick(seconds, 1 / 30, audio),
      onFrame: colors => {
        if (stopped) return;
        job.frames = zoneFrames(colors, slices);
        paintRig(job.canvas, job.frames, { zones, pad: 15 });
        job.panel.dataset.frame = "ready";
        job.status.textContent = "";
        finish();
      },
      onError: e => fail(e.message || "Preview unavailable"),
    });
    const deadline = setTimeout(() => fail("Preview timed out. Try again."), 17000);
    try {
      runner.load(job.effect, points).catch(error => fail(error.message || "Preview unavailable"));
      await finished;
    } finally {
      clearTimeout(deadline);
      finishCurrent = null;
      await runner.destroy();
    }
  }

  for (const [i, profile] of profiles.entries()) {
    const panel = panels[i], canvas = panel.querySelector("canvas"), status = panel.querySelector("output");
    const config = zones.map(z => {
      const light = z.config.override || profile.areas?.[z.spot] || profile;
      return { key: z.key, count: z.positions.length, id: light.effect, params: light.params || {},
        ...(sample ? {} : { layout: z.config.layout }) };
    });
    const job = { panel, canvas, status, frames: null,
      effect: { kind: "scene", id: profile.effect, brightness: profile.brightness, zones: config } };
    jobs.push(job);
    resize.observe(canvas);
    previous = previous.then(() => portrait(job)).catch(error => {
      if (!stopped) {
        status.textContent = error.message || "Preview unavailable";
        panel.dataset.frame = "error";
      }
    });
  }
  return () => {
    stopped = true;
    resize.disconnect();
    finishCurrent?.();
  };
}
