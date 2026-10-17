// The Studio's layer renderer. Runs inside the effect worker (and is copied into effects made with "View code").
// A builder effect is a stack of layers; each layer gives every LED a colour and a strength, and the stack is
// blended bottom to top. Positions are 0..1 across the case on both axes, like the built-in effects see them, so a
// remixed built-in looks the same.
const AirglowLayers = (() => {
  const TAU = Math.PI * 2;
  const clamp = (v, lo = 0, hi = 1) => (v < lo ? lo : v > hi ? hi : v);
  const hz = (speed, lo, hi) => lo * Math.pow(hi / lo, speed / 100);          // same curve as the built-in effects
  const rgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  function hsv(h, s, v) {
    h = ((h % 1) + 1) % 1;
    const i = Math.floor(h * 6), f = h * 6 - i, p = v * (1 - s), q = v * (1 - f * s), t = v * (1 - (1 - f) * s);
    return [[v, t, p], [q, v, p], [p, v, t], [p, q, v], [t, p, v], [v, p, q]][i % 6];
  }
  // position along a direction, normalised so it runs 0..1 across the case whatever the angle
  function axis(angle) {
    const a = angle * Math.PI / 180, c = Math.cos(a), s = Math.sin(a);
    const vals = [0, c, s, c + s], lo = Math.min(...vals), span = Math.max(...vals) - lo || 1;
    return { c, s, along: (x, y) => x * c + y * s, norm: (x, y) => (x * c + y * s - lo) / span };
  }
  function palette(hexes) {
    const stops = hexes.map(rgb);
    return (s) => {                                       // s 0..1 -> colour, stops spread evenly
      const x = clamp(s) * (stops.length - 1), i = Math.min(Math.floor(x), stops.length - 2), f = x - i;
      const a = stops[i], b = stops[i + 1];
      return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f];
    };
  }
  const mirror = (s) => 1 - Math.abs(2 * (((s % 1) + 1) % 1) - 1);
  function hash(i, j) {                                   // 0..1, stable per (i, j)
    let h = (i * 374761393 + j * 668265263) | 0;
    h = Math.imul(h ^ (h >>> 13), 1274126177);
    return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
  }

  // Each type: make(params) -> { frame(f)?: once per frame, at(x, y, i, f) -> [r, g, b, strength] }
  const TYPES = {
    solid(p) {
      const c = rgb(p.color);
      return { at: () => [c[0], c[1], c[2], 1] };
    },
    gradient(p) {
      const pal = palette(p.colors), ax = axis(p.angle), k = 0.8 * 50 / p.scale;
      return {
        at(x, y, i, f) {
          const base = p.style === "radial" ? Math.hypot((x - 0.5) * 1.6, y - 0.5) * 1.2 : ax.along(x, y);
          const c = pal(mirror(base * k + hz(p.speed, 0.005, 0.2) * f.time));
          return [c[0], c[1], c[2], 1];
        },
      };
    },
    rainbow(p) {
      const ax = axis(p.angle), scale = 0.4 + 2.6 * (1 - p.size / 100);
      return {
        at(x, y, i, f) {
          const c = hsv(ax.along(x, y) * scale - hz(p.speed, 0.02, 0.8) * f.time, p.saturation / 100, 1);
          return [c[0], c[1], c[2], 1];
        },
      };
    },
    cycle(p) {
      let c = [0, 0, 0];
      return {
        frame(f) { c = hsv(hz(p.speed, 0.005, 0.25) * f.time, p.saturation / 100, 1); },
        at: () => [c[0], c[1], c[2], 1],
      };
    },
    plasma(p) {
      const pal = palette(p.colors), k = 2 + 16 * (1 - p.scale / 100);
      return {
        at(x, y, i, f) {
          const t = f.time * hz(p.speed, 0.1, 3);
          const v = Math.sin(x * k * 1.6 + t) + Math.sin(y * k * 1.3 - t * 0.7)
            + Math.sin((x * 1.6 + y) * k * 0.7 + t * 1.1) + Math.sin(Math.hypot(x - 0.5, y - 0.5) * k * 1.5 - t);
          const c = pal(mirror(v / 8 + 0.5));
          return [c[0], c[1], c[2], 1];
        },
      };
    },
    wave(p) {
      const c = rgb(p.color), ax = axis(p.angle), freq = 1 + 9 * (1 - p.width / 100), pow = 1 + p.sharpness / 8;
      return {
        at(x, y, i, f) {
          const s = 0.5 + 0.5 * Math.cos(TAU * (ax.along(x, y) * freq - hz(p.speed, 0.05, 2) * f.time));
          return [c[0], c[1], c[2], Math.pow(s, pow)];
        },
      };
    },
    scanner(p) {
      const c = rgb(p.color), ax = axis(p.angle), w = Math.max(0.01, p.width / 100 * 0.5);
      let pos = 0;
      return {
        frame(f) {
          const u = hz(p.speed, 0.05, 1.5) * f.time;
          pos = p.bounce ? mirror(u / 2) * (1 + 2 * w) - w : ((u % 1) * (1 + 2 * w)) - w;
        },
        at(x, y) {
          const k = clamp(1 - Math.abs(ax.norm(x, y) - pos) / w);
          return [c[0], c[1], c[2], k * k];
        },
      };
    },
    sparkles(p) {
      const c = rgb(p.color), rate = 0.3 + 3 * p.speed / 100, chance = 0.02 + 0.4 * p.density / 100;
      return {
        at(x, y, i, f) {
          const u = f.time * rate + hash(i, 7), slot = Math.floor(u);
          if (hash(i, slot) > chance) return [0, 0, 0, 0];
          const life = u - slot, k = (1 - life) * (1 - life);
          const col = p.rainbow ? hsv(hash(i, slot + 99), 1, 1) : c;
          return [col[0], col[1], col[2], k];
        },
      };
    },
    pulse(p) {
      const c = rgb(p.color), floor = p.floor / 100;
      let a = 0;
      return {
        frame(f) { const k = 0.5 - 0.5 * Math.cos(TAU * hz(p.speed, 0.05, 0.6) * f.time); a = floor + (1 - floor) * k * k; },
        at: () => [c[0], c[1], c[2], a],
      };
    },
    beat(p) {
      const c = rgb(p.color), gain = 0.4 + 1.6 * p.sensitivity / 100, keep = 0.75 + 0.24 * p.decay / 100;
      let env = 0;
      return {
        frame(f) {
          const a = f.audio;
          const hit = a ? Math.max(a.beat ? 1 : 0, clamp(a.bass * gain)) : 0;
          env = Math.max(hit, env * Math.pow(keep, f.dt * 30));
        },
        at: () => [c[0], c[1], c[2], env],
      };
    },
    spectrum(p) {
      const ax = axis(p.angle), gain = 0.4 + 1.6 * p.sensitivity / 100;
      return {
        at(x, y, i, f) {
          const bands = f.audio?.bands;
          if (!bands) return [0, 0, 0, 0];
          const s = Math.min(ax.norm(x, y), 0.999999);
          const level = clamp(bands[Math.floor(s * bands.length)] * gain);
          const c = hsv(p.hue / 360 + 0.66 - s * 0.66, 1, 1);
          return [c[0], c[1], c[2], Math.pow(level, 1.4)];
        },
      };
    },
    bass(p) {
      const c = rgb(p.color), gain = 0.4 + 1.6 * p.sensitivity / 100;
      let r = 0;
      return {
        frame(f) { r = 0.05 + (p.size / 100) * 0.9 * clamp((f.audio?.bass || 0) * gain); },
        at(x, y) {
          const k = clamp(1 - Math.hypot((x - 0.5) * 1.6, y - 0.5) / r);
          return [c[0], c[1], c[2], Math.pow(k, 1.5)];
        },
      };
    },
  };

  const BLEND = {
    normal: (b, l, a) => b * (1 - a) + l * a,
    add: (b, l, a) => b + l * a,
    screen: (b, l, a) => 1 - (1 - b) * (1 - l * a),
    multiply: (b, l, a) => b * (1 - a + a * l),
  };

  // layers: the effect's layer list; params: the effect's controls (l<index>_<setting>) that override layer values;
  // defaults: layer type -> {setting: default}, so a half-edited layer in the Studio still draws.
  function effect(layers, defaults = {}) {
    let built = [];
    function build(params) {
      built = layers.map((layer, idx) => {
        if (layer.enabled === false || !TYPES[layer.type]) return null;
        const p = { ...(defaults[layer.type] || {}), ...(layer.params || {}) };
        for (const key of Object.keys(layer.expose || {})) {
          const v = params[`l${idx}_${key}`];
          if (v !== undefined) p[key] = v;
        }
        return { r: TYPES[layer.type](p), a: (layer.opacity ?? 100) / 100, blend: BLEND[layer.blend] || BLEND.normal,
                 mask: layer.mask && layer.mask !== "all" ? layer.mask : null };
      }).filter(Boolean);
    }
    return {
      init(ctx) { build(ctx.params); },
      onParamChange(id, value, ctx) { build(ctx.params); },
      frame(f) { for (const l of built) l.r.frame?.(f); },
      point(pt, f) {
        let r = 0, g = 0, b = 0;
        for (const l of built) {
          if (l.mask && pt.area !== l.mask) continue;
          const c = l.r.at(pt.x, pt.y, pt.index, f), a = c[3] * l.a;
          if (a <= 0) continue;
          r = l.blend(r, c[0], a); g = l.blend(g, c[1], a); b = l.blend(b, c[2], a);
        }
        return [clamp(r) * 255, clamp(g) * 255, clamp(b) * 255];
      },
    };
  }
  return { effect, TYPES, hsv, rgb, hz };
})();
