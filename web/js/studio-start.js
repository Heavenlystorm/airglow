// Where a new Studio effect can start: layer templates, code templates, and "Remix" for built-in effects (their
// layer equivalents, using the settings you have them on now). Templates carry stable layer ids and an effect-wide
// palette, so the first thing a new effect can do is look different with one control.

export const LAYER_TEMPLATES = [
  { id: "blank", name: "Blank", description: "One colour to build on.",
    master: { palette: ["#0b2a4a", "#2f5bff"] },
    layers: [{ id: "base", type: "solid", params: { color: "#0b2a4a" } }] },
  { id: "aurora", name: "Aurora", description: "Slow northern lights with a few stars.",
    master: { palette: ["#00ffa3", "#2f5bff", "#8a3dff"] },
    layers: [
      { id: "sky", type: "gradient", palette: true, params: { colors: ["#00ffa3", "#2f5bff", "#8a3dff"], speed: 14, angle: 20, scale: 40 } },
      { id: "drift", type: "plasma", palette: true, opacity: 45, blend: "screen", params: { colors: ["#000000", "#00e0c8", "#000000"], speed: 20, scale: 60 } },
      { id: "stars", type: "sparkles", opacity: 70, blend: "add", params: { color: "#e8fff6", density: 10, speed: 30 } }] },
  { id: "ember", name: "Ember", description: "Glowing coals that flare now and then.",
    master: { palette: ["#1a0400", "#ff3a00", "#ffb21a"] },
    layers: [
      { id: "coals", type: "plasma", palette: true, params: { colors: ["#1a0400", "#ff3a00", "#ffb21a"], speed: 35, scale: 55 } },
      { id: "sparks", type: "sparkles", opacity: 80, blend: "add", params: { color: "#ffcf7a", density: 18, speed: 60 } }] },
  { id: "ocean", name: "Ocean", description: "Deep blue with white crests rolling across.",
    master: { palette: ["#001a3a", "#0060c8", "#00b4ff"] },
    layers: [
      { id: "deep", type: "gradient", palette: true, params: { colors: ["#001a3a", "#0060c8", "#00b4ff"], speed: 18, angle: 90 } },
      { id: "crests", type: "wave", opacity: 55, blend: "add", params: { color: "#d8f4ff", speed: 30, width: 60, sharpness: 70, angle: 10 } }] },
  { id: "police", name: "Lights and sirens", description: "Red and blue bars chasing each other.",
    master: { palette: ["#ff1040", "#2f5bff"] },
    layers: [
      { id: "dark", type: "solid", params: { color: "#000000" } },
      { id: "red", type: "scanner", blend: "add", params: { color: "#ff1040", speed: 55, width: 30, angle: 0 } },
      { id: "blue", type: "scanner", blend: "add", params: { color: "#2f5bff", speed: 55, width: 30, angle: 180 } }] },
  { id: "party", name: "Party", description: "Music spectrum with a white flash on every beat.",
    master: { palette: ["#8a3dff", "#ffffff"] },
    layers: [
      { id: "bars", type: "spectrum", params: { sensitivity: 65 } },
      { id: "flash", type: "beat", opacity: 60, blend: "add", params: { color: "#ffffff", sensitivity: 55, decay: 30 } }] },
  { id: "playable", name: "Two knobs", description: "A comet around every fan with a palette and a speed control.",
    master: { palette: ["#00e0c8", "#8a3dff", "#ff2fb4"] },
    expose: { palette: "Your colours", speed: "Pace" },
    layers: [
      { id: "trail", type: "comet", palette: true, react: { what: "speed", amount: 30 },
        expose: { tail: "Tail length" }, params: { color: "#00e0c8", background: "#05070d", speed: 45, tail: 50 } },
      { id: "glow", type: "bass", palette: true, opacity: 40, blend: "add", params: { color: "#8a3dff", sensitivity: 60, size: 60 } }] },
];

export const CODE_TEMPLATES = [
  { id: "canvas", name: "Canvas", description: "Draw with the 2D canvas; every LED takes the colour under it.", code: `// Draw on a 320 x 200 canvas; every LED takes the colour under it.
// ctx.params holds your controls, frame.time is in seconds.
airglow.effect({
  render(ctx, frame) {
    const g = ctx.canvas.getContext("2d");
    const t = frame.time * ctx.params.speed;
    g.fillStyle = "#050510";
    g.fillRect(0, 0, ctx.width, ctx.height);
    for (let i = 0; i < 4; i++) {
      const x = (Math.sin(t + i * 1.7) * 0.5 + 0.5) * ctx.width;
      const y = (Math.cos(t * 0.8 + i) * 0.5 + 0.5) * ctx.height;
      const r = g.createRadialGradient(x, y, 0, x, y, 90);
      r.addColorStop(0, ctx.params.color);
      r.addColorStop(1, "transparent");
      g.fillStyle = r;
      g.fillRect(0, 0, ctx.width, ctx.height);
    }
  },
});`, params: [
    { id: "speed", label: "Speed", type: "number", min: 0.1, max: 3, step: 0.1, default: 1 },
    { id: "color", label: "Colour", type: "color", default: "#ff2fb4" }] },
  { id: "point", name: "Per LED", description: "Answer one question: what colour is this LED right now?", code: `// Called for every LED: led.x and led.y go from 0 to 1 across the case, led.local is where it sits on its own
// fan or strip, led.area is where it is ("front", "top", "gpu", ...). Return [red, green, blue], each 0 to 255.
let speed = 1;
airglow.effect({
  init(ctx) { speed = ctx.params.speed; },
  onParamChange(id, value) { if (id === "speed") speed = value; },
  point(led, frame) {
    const wave = Math.sin((led.x * 4 - frame.time * speed) * Math.PI) * 0.5 + 0.5;
    if (led.area === "gpu") return [255 * wave, 40, 255];
    return [40, 120 + 135 * wave, 255 * (1 - wave)];
  },
});`, params: [
    { id: "speed", label: "Speed", type: "number", min: 0.1, max: 4, step: 0.1, default: 1 }] },
  { id: "controls", name: "Controls", description: "Declare your knobs in code with airglow.params.", code: `// Controls live next to the code that reads them. Airglow writes them into the file when you save.
airglow.params({
  speed: { label: "Speed", type: "number", min: 0.1, max: 3, step: 0.1, default: 1 },
  color: { label: "Colour", type: "color", default: "#2f5bff" },
  mode: { label: "Mode", type: "select", options: ["bars", "dots"], default: "bars" },
});

airglow.effect({
  render(ctx, frame) {
    const g = ctx.canvas.getContext("2d");
    g.fillStyle = "#05060a";
    g.fillRect(0, 0, ctx.width, ctx.height);
    g.fillStyle = ctx.params.color;
    for (let i = 0; i < 12; i++) {
      const v = ctx.params.mode === "bars" ? 20 + 80 * Math.abs(Math.sin(i + frame.time * ctx.params.speed)) : 30;
      const x = 8 + i * 26;
      g.fillRect(x, 100 - v / 2, 18, v);
    }
  },
});`, params: [
    { id: "speed", label: "Speed", type: "number", min: 0.1, max: 3, step: 0.1, default: 1 },
    { id: "color", label: "Colour", type: "color", default: "#2f5bff" },
    { id: "mode", label: "Mode", type: "select", options: ["bars", "dots"], default: "bars" }] },
  { id: "webgl", name: "Shader", description: "A WebGL fragment shader, for the people who speak GLSL.", code: `// A fragment shader on the 320 x 200 canvas. u_time in seconds, u_res the canvas size, u_bass 0..1.
const FRAG = \`#version 300 es
precision highp float;
uniform float u_time; uniform vec2 u_res; uniform float u_bass;
out vec4 o;
void main() {
  vec2 p = gl_FragCoord.xy / u_res;
  float v = sin(p.x * 9.0 + u_time) + sin(p.y * 7.0 - u_time * 1.3) + sin((p.x + p.y) * 6.0 + u_time * 0.7);
  o = vec4(0.5 + 0.5 * sin(v + vec3(0.0, 2.0, 4.0)) * (0.6 + 0.4 * u_bass), 1.0);
}\`;
let gl, prog, loc;
airglow.effect({
  init(ctx) {
    gl = ctx.canvas.getContext("webgl2");
    const sh = (type, src) => { const s = gl.createShader(type); gl.shaderSource(s, src); gl.compileShader(s);
      if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s)); return s; };
    prog = gl.createProgram();
    gl.attachShader(prog, sh(gl.VERTEX_SHADER, "#version 300 es\\nin vec2 a; void main() { gl_Position = vec4(a, 0, 1); }"));
    gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FRAG));
    gl.linkProgram(prog);
    gl.useProgram(prog);
    gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
    gl.enableVertexAttribArray(0);
    gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0);
    loc = { t: gl.getUniformLocation(prog, "u_time"), r: gl.getUniformLocation(prog, "u_res"), b: gl.getUniformLocation(prog, "u_bass") };
  },
  render(ctx, frame) {
    gl.uniform1f(loc.t, frame.time);
    gl.uniform2f(loc.r, ctx.width, ctx.height);
    gl.uniform1f(loc.b, frame.audio ? frame.audio.bass : 0);
    gl.drawArrays(gl.TRIANGLES, 0, 3);
  },
});`, params: [] },
];

// Built-in effects that have a layer equivalent: their current settings become the first layers of a remix
export const REMIX = {
  solid: (p) => [{ type: "solid", params: { color: p.color } }],
  breathing: (p) => [{ type: "pulse", params: { color: p.color, speed: p.speed, floor: 6 } }],
  cycle: (p) => [{ type: "cycle", params: { speed: p.speed, saturation: p.saturation } }],
  rainbow: (p) => [{ type: "rainbow", params: { speed: p.speed, size: p.size, angle: p.angle, saturation: p.saturation } }],
  gradient: (p) => [{ type: "gradient", params: { colors: p.colors, speed: p.speed, angle: p.angle } }],
  spectrum: (p) => [{ type: "spectrum", params: { sensitivity: p.sensitivity, hue: p.hue, angle: p.angle } }],
  pulse: (p) => [{ type: "solid", opacity: p.floor, params: { color: p.color } },
    { type: "beat", params: { color: p.color, sensitivity: p.sensitivity } }],
};

// ------------------------------------------------------------ Surprise me
// A valid starting point in one click: a colour, a motion or two, sometimes music. Every layer type and value comes
// from the catalogue, so the result always passes the checks.
const PALETTES = [
  ["#00ffa3", "#2f5bff", "#8a3dff"], ["#ff2fb4", "#ffb21a", "#ff5a1f"], ["#00e0c8", "#00b4ff", "#d8f4ff"],
  ["#ff1040", "#8a3dff", "#2f5bff"], ["#c8ff2a", "#22e06b", "#00e0c8"], ["#ffcf7a", "#ff3a00", "#1a0400"],
  ["#e8fff6", "#8a3dff", "#2f5bff"], ["#ffb21a", "#ff5a1f", "#ff2fb4"],
];
const pick = (list) => list[Math.floor(Math.random() * list.length)];
const rnd = (lo, hi) => lo + Math.random() * (hi - lo);

export function surprise() {
  const palette = pick(PALETTES).slice();
  const base = pick(["gradient", "plasma", "rainbow", "cycle"]);
  const layers = [];
  const first = { id: "a", type: base, params: {} };
  if (base === "gradient") first.params = { colors: palette, style: pick(["linear", "radial"]), angle: Math.round(rnd(0, 359)), speed: Math.round(rnd(8, 35)), scale: Math.round(rnd(35, 80)) };
  else if (base === "plasma") first.params = { colors: palette, speed: Math.round(rnd(15, 45)), scale: Math.round(rnd(35, 75)) };
  else if (base === "rainbow") first.params = { speed: Math.round(rnd(20, 60)), size: Math.round(rnd(25, 70)), angle: Math.round(rnd(0, 359)), saturation: Math.round(rnd(60, 100)) };
  else first.params = { speed: Math.round(rnd(15, 45)), saturation: Math.round(rnd(60, 100)) };
  if (base === "gradient" || base === "plasma") first.palette = true;
  layers.push(first);
  const motion = pick(["wave", "scanner", "sparkles", "comet", "chase"]);
  const second = { id: "b", type: motion, opacity: Math.round(rnd(35, 75)), blend: pick(["add", "screen"]), params: {} };
  if (motion === "wave") second.params = { color: palette[0], speed: Math.round(rnd(15, 45)), width: Math.round(rnd(30, 70)), sharpness: Math.round(rnd(40, 90)), angle: Math.round(rnd(0, 359)) };
  else if (motion === "scanner") second.params = { color: palette[1] || palette[0], speed: Math.round(rnd(20, 55)), width: Math.round(rnd(10, 40)), angle: Math.round(rnd(0, 359)), bounce: true };
  else if (motion === "sparkles") second.params = { color: palette[0], density: Math.round(rnd(8, 35)), speed: Math.round(rnd(25, 70)), rainbow: Math.random() < 0.3 };
  else if (motion === "comet") second.params = { color: palette[0], background: "#05070d", speed: Math.round(rnd(25, 65)), tail: Math.round(rnd(30, 80)), reverse: Math.random() < 0.5 };
  else second.params = { color: palette[0], speed: Math.round(rnd(25, 65)), count: Math.round(rnd(1, 4)), width: Math.round(rnd(30, 80)), reverse: Math.random() < 0.5 };
  layers.push(second);
  if (Math.random() < 0.35) {
    const music = Math.random() < 0.5 ? "beat" : "spectrum";
    layers.push(music === "beat"
      ? { id: "c", type: "beat", opacity: 55, blend: "add", params: { color: palette[2] || palette[0], sensitivity: Math.round(rnd(45, 80)), decay: 45 } }
      : { id: "c", type: "spectrum", opacity: 75, blend: "screen", params: { sensitivity: Math.round(rnd(45, 80)), hue: 0, angle: Math.round(rnd(0, 359)) } });
  }
  return { layers, master: { palette, speed: 100, brightness: 100, react: 50 },
           expose: Math.random() < 0.5 ? { palette: "Your colours" } : { palette: "Your colours", speed: "Pace" } };
}
