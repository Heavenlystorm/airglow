// The Studio's layer renderer. Comes with every effect in its worker: builder effects are drawn by it, and code
// effects (including those made with "View code") can use it as AirglowLayers.
//
// A builder effect is a stack of layers; each layer gives every LED a colour and a strength, and the stack is
// blended bottom to top. Positions are 0..1 across the case on both axes, like the built-in effects see them, so a
// remixed built-in looks the same.
//
// Since effect format 2 the stack also has effect-wide settings (palette, speed, brightness, music reactivity) and a
// layer may take its colours from the palette, react to the music, and keep a stable id so its controls survive a
// reorder. A v1 file (a plain list of layers) still draws exactly as it did.
//
// A property of the global object rather than a declaration: an effect that declares its own AirglowLayers (as files
// from early "View code" did) shadows it instead of failing with "already declared".
self.AirglowLayers = (() => {
  const TAU = Math.PI * 2;
  const clamp = (v, lo = 0, hi = 1) => (v < lo ? lo : v > hi ? hi : v);
  const hz = (speed, lo, hi) => lo * Math.pow(hi / lo, speed / 100);          // same curve as the built-in effects
  const HEX = /^#[0-9a-f]{6}$/i;
  const rgb = (hex) => (HEX.test(String(hex)) ? [1, 3, 5].map((i) => parseInt(String(hex).slice(i, i + 2), 16) / 255) : [1, 1, 1]);
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
    const stops = (Array.isArray(hexes) && hexes.length ? hexes : ["#ff2fb4", "#2f5bff"]).map(rgb);
    if (stops.length === 1) stops.push(stops[0]);          // one colour still draws (a half-edited layer)
    return (s) => {                                       // s 0..1 -> colour, stops spread evenly
      const x = clamp(s) * (stops.length - 1), i = Math.min(Math.floor(x), stops.length - 2), f = x - i;
      const a = stops[i], b = stops[Math.max(i + 1, 0)];
      return [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f];
    };
  }
  const mirror = (s) => 1 - Math.abs(2 * (((s % 1) + 1) % 1) - 1);
  function hash(i, j) {                                   // 0..1, stable per (i, j)
    let h = (i * 374761393 + j * 668265263) | 0;
    h = Math.imul(h ^ (h >>> 13), 1274126177);
    return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
  }
  // a LED's place within its own fan or strip: the real thing when the rig sends it, its x otherwise
  const localOf = (pt, x) => (pt && pt.local != null ? pt.local : x);

  // Each type: make(params) -> { frame(f)?: once per frame, at(x, y, i, f, pt) -> [r, g, b, strength] }
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
    comet(p) {
      const c = rgb(p.color), bg = rgb(p.background), tail = 0.04 + 0.9 * p.tail / 100;
      return {
        at(x, y, i, f, pt) {
          const head = (hz(p.speed, 0.08, 2.0) * f.time) % 1;
          const d = p.reverse ? (localOf(pt, x) - head + 1) % 1 : (head - localOf(pt, x) + 1) % 1;
          const k = Math.pow(clamp(1 - d / tail), 2);
          return [bg[0] + (c[0] - bg[0]) * k, bg[1] + (c[1] - bg[1]) * k, bg[2] + (c[2] - bg[2]) * k, 1];
        },
      };
    },
    chase(p) {
      const c = rgb(p.color), bands = Math.max(1, Math.round(p.count));
      const w = Math.max(0.02, p.width / 100);
      return {
        at(x, y, i, f, pt) {
          const u = (localOf(pt, x) * bands + (p.reverse ? -1 : 1) * hz(p.speed, 0.05, 1.5) * f.time) % 1;
          const k = Math.pow(clamp(1 - Math.abs(2 * u - 1) / w), 1.5);
          return [c[0], c[1], c[2], k];
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

  const MASTER = { palette: ["#ff2fb4", "#2f5bff"], speed: 100, brightness: 100, react: 50 };
  const num = (v, lo, hi, fallback) => (typeof v === "number" && isFinite(v) ? clamp(v, lo, hi) : fallback);

  function config(raw) {
    if (Array.isArray(raw)) return { layers: raw, master: {}, expose: {} };
    if (!raw || !Array.isArray(raw.layers)) return { layers: [], master: {}, expose: {} };
    return raw;
  }

  // The music as a layer sees it, with the effect's reactivity applied: 50 is the analysed level, 0 quiet, 100 loud.
  function scaledAudio(audio, gain) {
    if (!audio || gain === 1) return audio;
    const src = audio.bands, bands = new Float32Array(src ? src.length : 64);
    for (let i = 0; i < bands.length; i++) bands[i] = clamp((src ? src[i] : 0) * gain, 0, 1.5);
    return { level: clamp((audio.level || 0) * gain), bass: clamp((audio.bass || 0) * gain), beat: !!audio.beat, bands };
  }

  // layers: the effect's layer list (or the whole {layers, master, expose} block); params: the effect's controls,
  // which override layer settings and the effect-wide ones (m_palette, m_speed, m_brightness, m_react).
  // defaults: layer type -> {setting: default}, so a half-edited layer in the Studio still draws.
  function effect(raw, defaults = {}) {
    const cfg = config(raw);
    let built = [], master = { ...MASTER }, shared = null, env = 0, brightness = 1, reactGain = 1;

    function build(params) {
      const m = cfg.master || {};
      const pal = (Array.isArray(params.m_palette) ? params.m_palette
        : (Array.isArray(m.palette) ? m.palette : MASTER.palette)).filter((h) => HEX.test(String(h))).map((h) => String(h).toLowerCase());
      master = {
        palette: (pal.length >= 2 ? pal : MASTER.palette).slice(0, 4),
        speed: num(params.m_speed, 0, 200, num(m.speed, 0, 200, MASTER.speed)),
        brightness: num(params.m_brightness, 0, 100, num(m.brightness, 0, 100, MASTER.brightness)),
        react: num(params.m_react, 0, 100, num(m.react, 0, 100, MASTER.react)),
      };
      brightness = master.brightness / 100;
      reactGain = 0.4 + 1.2 * master.react / 100;
      built = cfg.layers.map((layer, idx) => {
        if (layer.enabled === false || !TYPES[layer.type]) return null;
        const p = { ...(defaults[layer.type] || {}), ...(layer.params || {}) };
        const key = layer.id != null ? layer.id : idx;            // a stable id keeps a control on its own layer
        for (const name of Object.keys(layer.expose || {})) {
          const v = params[`l${key}_${name}`];
          if (v !== undefined) p[name] = v;
        }
        if (layer.palette) {
          if (Array.isArray(p.colors)) p.colors = master.palette.slice();
          else if (p.color) p.color = master.palette[0];
        }
        const react = layer.react || {};
        const amount = clamp((react.amount || 0) / 100);
        return {
          r: TYPES[layer.type](p), a: (layer.opacity ?? 100) / 100,
          blend: BLEND[layer.blend] || BLEND.normal,
          mask: layer.mask && layer.mask !== "all" ? layer.mask : null,
          react: { what: react.what === "speed" ? "speed" : "strength", amount },
          frame: null, aNow: 0,
        };
      }).filter(Boolean);
    }

    // One frame for the whole stack, plus a slightly different one for each layer that follows the music's speed.
    function prepare(f) {
      const t = f.time * master.speed / 100, dt = f.dt * master.speed / 100;
      const audio = scaledAudio(f.audio, reactGain);
      const base = { time: t, dt, audio };
      env = audio ? clamp(Math.max(audio.beat ? 1 : 0, audio.bass || 0)) : 0;
      shared = base;
      for (const l of built) {
        const mod = l.react.amount;
        let frame = base;
        if (l.react.what === "speed" && mod > 0) {
          const k = 1 + mod * env;
          frame = { time: t * k, dt: dt * k, audio };
        }
        l.frame = frame;
        l.r.frame?.(frame);
        l.aNow = l.react.what === "strength" && mod > 0 ? Math.min(1, l.a * (1 + mod * env)) : l.a;
      }
    }

    return {
      init(ctx) { build(ctx.params); },
      onParamChange(id, value, ctx) { build(ctx.params); },
      frame(f) { prepare(f); },
      point(pt, f) {
        const frame = shared || f;
        let r = 0, g = 0, b = 0;
        for (const l of built) {
          if (l.mask && pt.area !== l.mask) continue;
          const c = l.r.at(pt.x, pt.y, pt.index, l.frame || frame, pt), a = c[3] * (l.aNow || l.a);
          if (a <= 0) continue;
          r = l.blend(r, c[0], a); g = l.blend(g, c[1], a); b = l.blend(b, c[2], a);
        }
        return [clamp(r) * 255 * brightness, clamp(g) * 255 * brightness, clamp(b) * 255 * brightness];
      },
    };
  }
  return { effect, TYPES, hsv, rgb, hz, MASTER };
})();
