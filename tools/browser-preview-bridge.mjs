// The demo deliberately disables native WebView2 hosts. Browser tests substitute only that transport:
// effects still run in the production EffectRunner sandbox/worker, on a separate owned page.
export async function installBrowserPreviewBridge(page, base) {
  if (base !== "http://127.0.0.1:8790") throw new Error("Browser preview bridge only supports the owned demo fixture");
  const settings = await (await page.request.get(`${base}/api/settings`)).json();
  if (settings.settings?.lhm_url !== "http://127.0.0.1:8791") throw new Error("Browser preview bridge requires demo fixture settings");
  const hostContext = await page.context().browser().newContext();
  const host = await hostContext.newPage();
  const hostErrors = [];
  host.on("pageerror", error => hostErrors.push(error.message));
  await host.route(`${base}/__browser-preview-bridge__`, route => route.fulfill({ contentType: "text/html",
    body: "<!doctype html><title>Owned browser preview bridge</title>" }));
  await host.goto(`${base}/__browser-preview-bridge__`);
  await host.evaluate(async () => {
    const { EffectRunner } = await import("/js/fxrun.js");
    const entries = new Map(), stats = { loads: 0, frames: 0, stops: 0 };
    let sequence = 0;
    function failure(entry, problem) {
      entry.problem = problem;
      for (const wake of entry.waiting.splice(0)) wake();
    }
    function wait(entry, predicate, timeout = 6000) {
      if (predicate()) return Promise.resolve();
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          entry.waiting = entry.waiting.filter(fn => fn !== wake);
          reject(new Error("Browser preview worker did not answer"));
        }, timeout);
        function wake() {
          if (!predicate()) { entry.waiting.push(wake); return; }
          clearTimeout(timer); resolve();
        }
        entry.waiting.push(wake);
      });
    }
    globalThis.browserPreviewBridge = {
      stats,
      async request(action, body) {
        if (action === "load") {
          const id = `browser-preview-${++sequence}`;
          const entry = { ready: false, frame: 0, colors: null, ms: 0, backdrop: null, problem: null, logs: [], waiting: [] };
          const runner = new EffectRunner({
            onReady() { entry.ready = true; for (const wake of entry.waiting.splice(0)) wake(); },
            onFrame(colors, ms) {
              entry.colors = btoa(String.fromCharCode(...colors)); entry.ms = ms; entry.frame++; stats.frames++;
              // EffectRunner delivers the optional backdrop immediately after the colors.
              queueMicrotask(() => { for (const wake of entry.waiting.splice(0)) wake(); });
            },
            onBackdrop(bitmap) {
              const canvas = document.createElement("canvas"); canvas.width = bitmap.width; canvas.height = bitmap.height;
              canvas.getContext("2d").drawImage(bitmap, 0, 0); bitmap.close();
              entry.backdrop = canvas.toDataURL("image/jpeg", .8).split(",")[1];
            },
            onLog(text) { entry.logs.push(text); entry.logs = entry.logs.slice(-40); },
            onError(error) { failure(entry, { reason: "error", message: error.message, line: error.line }); },
            onStopped(reason) { if (!entry.problem) failure(entry, { reason, message: `Preview stopped: ${reason}` }); },
          });
          entry.runner = runner; entries.set(id, entry); stats.loads++;
          try {
            await runner.load(body.effect, body.points);
            await wait(entry, () => entry.ready || entry.problem);
            if (entry.problem) throw new Error(entry.problem.message);
            return { id };
          } catch (error) { runner.destroy(); entries.delete(id); throw error; }
        }
        const entry = entries.get(body.id);
        if (action === "stop") {
          if (entry) { entry.runner.destroy(); entries.delete(body.id); failure(entry, { reason: "stopped", message: "Preview ended" }); }
          stats.stops++; return { ok: true };
        }
        if (action !== "tick") throw new Error(`Unsupported browser preview action: ${action}`);
        if (!entry) return { problem: { reason: "stopped", message: "Preview ended" } };
        if (entry.problem) return { problem: entry.problem };
        const updates = body.updates || {};
        if (updates.params) entry.runner.setParams(updates.params);
        if (updates.points) entry.runner.setPoints(updates.points);
        if (updates.layers) await entry.runner.setLayers(updates.layers);
        const previous = entry.frame;
        entry.runner.tick(body.time, body.dt, body.audio, body.backdrop);
        await wait(entry, () => entry.frame > previous || entry.problem);
        if (entry.problem) return { problem: entry.problem };
        const logs = entry.logs.splice(0);
        return { colors: entry.colors, ms: entry.ms, logs, ...(body.backdrop && entry.backdrop ? { backdrop: entry.backdrop } : {}) };
      },
      close() { for (const entry of entries.values()) { entry.runner.destroy(); failure(entry, { reason: "stopped", message: "Bridge closed" }); } entries.clear(); },
    };
  });
  let closing = false;
  const pattern = `${base}/api/studio/preview/*`;
  const routePreview = async route => {
    try {
      if (closing) return await route.abort();
      const action = new URL(route.request().url()).pathname.split("/").at(-1), body = route.request().postDataJSON();
      const result = await host.evaluate(({ action, body }) => browserPreviewBridge.request(action, body), { action, body });
      await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(result) });
    } catch (error) {
      if (!closing) await route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: error.message }) }).catch(() => {});
    }
  };
  await page.route(pattern, routePreview);
  return {
    async stats() { return host.evaluate(() => browserPreviewBridge.stats); },
    async close() {
      closing = true;
      if (!host.isClosed()) await host.evaluate(() => browserPreviewBridge.close()).catch(() => {});
      if (!page.isClosed()) await page.unroute(pattern, routePreview);
      await hostContext.close();
      if (hostErrors.length) throw new Error(`Preview bridge page errors: ${hostErrors.join("; ")}`);
    },
  };
}
