// Airglow saved-effects blend v1. Two frozen endpoints; the normal worker budget covers both.
(() => {
  const W = 320, H = 200;
  function endpoint(factory, params, credit) {
    let def;
    factory({ effect(value) { def = value; }, params() {} });
    if (!def || (typeof def.point !== "function" && typeof def.render !== "function"))
      throw new Error(`Cannot blend ${credit.name}: it must register a point or render effect with airglow.effect().`);
    let ctx, canvas, sampler, direct, drawing, image;
    return {
      init(parent) {
        canvas = new OffscreenCanvas(W, H);
        const get = canvas.getContext.bind(canvas);
        try { canvas.getContext = (type, ...args) => { drawing = type; return get(type, ...args); }; } catch {}
        ctx = { ...parent, canvas, width: W, height: H, params: structuredClone(params) };
        def.init?.(ctx);
      },
      frame(frame, rig) {
        ctx.rig = rig;
        if (def.render) {
          def.render(ctx, frame);
          if (direct === undefined) direct = drawing === "2d" ? canvas.getContext("2d") : null;
          if (direct) image = direct.getImageData(0, 0, W, H).data;
          else {
            sampler ||= new OffscreenCanvas(W, H).getContext("2d", { willReadFrequently: true });
            sampler.clearRect(0, 0, W, H);
            sampler.drawImage(canvas, 0, 0);
            image = sampler.getImageData(0, 0, W, H).data;
          }
        } else def.frame?.(frame);
      },
      point(led, frame) {
        if (!def.render) return def.point(led, frame) || [0, 0, 0];
        const cx = Math.round(led.px), cy = Math.round(led.py);
        let r = 0, g = 0, b = 0, n = 0;
        // Match worker.js: each endpoint is sampled and alpha-composited over black before mixing.
        for (let dy = -1; dy <= 1; dy++) for (let dx = -1; dx <= 1; dx++) {
          const x = cx + dx, y = cy + dy;
          if (x < 0 || y < 0 || x >= W || y >= H) continue;
          const offset = (y * W + x) * 4, alpha = image[offset + 3] / 255;
          r += image[offset] * alpha; g += image[offset + 1] * alpha; b += image[offset + 2] * alpha; n++;
        }
        return n ? [r / n, g / n, b / n] : [0, 0, 0];
      },
      dispose() { def.dispose?.(ctx); image = sampler = canvas = ctx = null; },
    };
  }
  const ends = [/*__AIRGLOW_BLEND_ENDPOINTS__*/];
  let context, amount = .5;
  airglow.effect({
    init(ctx) { context = ctx; for (const end of ends) end.init(ctx); },
    frame(frame) {
      const value = Number(context.params.blend);
      amount = Number.isFinite(value) ? Math.max(0, Math.min(1, value / 100)) : .5;
      for (const end of ends) end.frame(frame, context.rig);
    },
    point(led, frame) {
      const a = ends[0].point(led, frame), b = ends[1].point(led, frame);
      // Preserve each endpoint's Uint8 LED output, including the exact A and B positions.
      return [0, 1, 2].map(i => Math.round((a[i] & 255) * (1 - amount) + (b[i] & 255) * amount));
    },
    dispose() { for (const end of ends) end.dispose(); },
  });
})();
