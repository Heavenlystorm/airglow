import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";

const path = new URL("../web/js/overview-instruments.js", import.meta.url);
const I = existsSync(path) ? await import(`data:text/javascript;base64,${Buffer.from(readFileSync(path, "utf8")).toString("base64")}`) : {};
const curves = { curves: { silent: [[40, 25], [60, 50], [90, 100]], balanced: [[40, 30], [60, 60], [90, 100]] }, safety_temp: 90 };
const svg = (html) => html.match(/<svg\b[\s\S]*?<\/svg>/)?.[0];

test("saved solid and gradient artwork retains the profile colors and distinguishes their spatial pattern", () => {
  assert.equal(typeof I.profileArtwork, "function", "profile artwork is implemented");
  const solid = I.profileArtwork({ name: "Ember", effect: "solid", params: { color: "#fa6412" }, brightness: 35 });
  const gradient = I.profileArtwork({ name: "Water", effect: "gradient", params: { colors: ["#00b4ff", "#2f5bff"], angle: 0 }, brightness: 75 });
  assert.match(solid, /fill="#fa6412"/);
  assert.match(gradient, /#00b4ff/);
  assert.match(gradient, /#2f5bff/);
  assert.notEqual(svg(solid), svg(gradient));
  assert.match(solid, /35%/);
  assert.match(solid, /<g opacity="0.35">/);
  assert.match(gradient, /Saved pattern/);
});

test("rainbow artwork honors saved saturation and a zero-brightness profile is visibly off", () => {
  const white = I.profileArtwork({ effect: "rainbow", params: { saturation: 0 }, brightness: 100 });
  assert.ok([...white.matchAll(/<rect x="\d+"[^>]*fill="([^"]+)"/g)].every((m) => m[1] === "#ffffff"));
  const dark = I.profileArtwork({ effect: "solid", params: { color: "#ff0000" }, brightness: 0 });
  assert.match(dark, /Lights off/);
  assert.match(dark, /<g opacity="0">/);
});

test("same-color breathing and solid thumbnails have distinct geometry while gradients honor direction", () => {
  const profile = { name: "Blue", effect: "solid", params: { color: "#0066ff" }, brightness: 100 };
  assert.notEqual(svg(I.profileArtwork(profile)), svg(I.profileArtwork({ ...profile, effect: "breathing" })));
  const gradient = { effect: "gradient", params: { colors: ["#ff0000", "#0000ff"], angle: 0 } };
  assert.notEqual(svg(I.profileArtwork(gradient)), svg(I.profileArtwork({ ...gradient, params: { ...gradient.params, angle: 90 } })));
});

test("reactive and unsupported effects show saved palettes or unavailable artwork without inventing live input", () => {
  const script = I.profileArtwork({ name: "Custom", effect: "studio:custom", params: { colors: ["#123456", "#abcdef"] } });
  assert.match(script, /Saved palette/);
  assert.match(script, /#123456/);
  assert.match(script, /#abcdef/);
  const screen = I.profileArtwork({ effect: "screen", params: {} });
  assert.match(screen, /Screen reactive/);
  assert.doesNotMatch(screen, /#(?:00b4ff|fa6412)/);
  assert.match(I.profileArtwork(null), /Preview unavailable/);
});

test("profile area overrides retain their saved colors and clearly label the palette summary", () => {
  const html = I.profileArtwork({ effect: "solid", params: { color: "#0000ff" }, areas: { front: { effect: "solid", params: { color: "#ff0000" } } } });
  assert.match(html, /#0000ff/);
  assert.match(html, /#ff0000/);
  assert.match(html, /Saved area palette/);
});

test("artwork escapes text, rejects injected colors, and does not duplicate SVG IDs", () => {
  const html = I.profileArtwork({ name: '<img src=x onerror="alert(1)">', effect: 'studio:" onload="alert(1)', params: { color: '#000000" onload="alert(1)', colors: ["#123456", "url(https://bad)"] } });
  assert.doesNotMatch(html, /<img|\sonload=["']|\sonerror=["']|url\(|\sid=/);
  assert.match(html, /&lt;img/);
  assert.doesNotMatch(I.profileArtwork({ effect: "gradient", params: { colors: "#ff0000" } }), /undefined|NaN/);
});

test("fan graph follows the active curve rather than the base curve and plots requested duty without modifying inputs", () => {
  const fans = { curve: "silent", base_curve: "balanced", temp: 60, duty: 42, channels: [], ports: [] };
  const before = structuredClone({ fans, curves });
  const html = I.fanInstrument(fans, curves, { stale: false });
  assert.match(html, /60°C/);
  assert.match(html, /42% requested/);
  assert.match(html, /class="fan-current"/);
  assert.match(html, /class="fan-curve-line"[^>]*points="32,76\.5 100,76\.5 168,57 270,18 304,18"/);
  assert.deepEqual({ fans, curves }, before);
});

test("zero control temperature and requested duty remain visible instead of becoming missing", () => {
  const html = I.fanInstrument({ curve: "silent", temp: 0, duty: 0 }, curves);
  assert.match(html, /0°C/);
  assert.match(html, /0% requested/);
  assert.match(html, /class="fan-current"[^>]*left:10%;top:77\.41935483870968%/);
});

test("stale fan data retains the saved curve but removes the operating point and old readings", () => {
  const html = I.fanInstrument({ curve: "silent", temp: 63, duty: 47 }, curves, { stale: true });
  assert.match(html, /fan-curve-line/);
  assert.match(html, /Waiting for current readings/);
  assert.doesNotMatch(html, /fan-current|63°C|47%/);
});

test("missing temperature, invalid duty, and an unavailable profile curve never fabricate a point or curve", () => {
  const missing = I.fanInstrument({ curve: "silent", temp: null, duty: 50 }, curves);
  assert.match(missing, /Temperature unavailable/);
  assert.match(missing, /50% requested/);
  assert.doesNotMatch(missing, /fan-current/);
  const invalid = I.fanInstrument({ curve: "silent", temp: 40, duty: "50" }, curves);
  assert.doesNotMatch(invalid, /fan-current/);
  assert.match(invalid, /Request unavailable/);
  const unavailable = I.fanInstrument({ curve: "profile:mine", temp: 55, duty: 25 }, curves);
  assert.match(unavailable, /Curve unavailable/);
  assert.doesNotMatch(unavailable, /fan-curve-line/);
  assert.doesNotMatch(I.fanInstrument(null, null), /undefined|NaN|fan-current/);
});

test("malformed curves are omitted atomically and labels cannot inject markup", () => {
  const name = '<img src=x onerror="bad">';
  const html = I.fanInstrument({ curve: name, temp: Infinity, duty: -1 }, { curves: { [name]: [[40, 30], [60, "bad"], [90, 100]] } });
  assert.match(html, /Curve unavailable/);
  assert.doesNotMatch(html, /<img|\sonerror=|fan-curve-line|fan-current|NaN|Infinity/);
});

test("Fahrenheit labels convert temperatures without changing the curve or operating point", () => {
  const fans = { curve: "silent", temp: 60, duty: 50 };
  const c = I.fanInstrument(fans, curves);
  const f = I.fanInstrument(fans, curves, { tempUnit: "F" });
  assert.match(f, /140°F/);
  assert.equal(c.match(/class="fan-current"[^>]+/)[0], f.match(/class="fan-current"[^>]+/)[0]);
});
