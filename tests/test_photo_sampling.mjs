import assert from "node:assert/strict";
import test from "node:test";
import { extractPhotoStops, photoHistogram, pixelAt } from "../web/js/photo-sampling.js";

function image(width, height, colourAt) {
  const data = new Uint8ClampedArray(width * height * 4);
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const [r, g, b, a = 255] = colourAt(x, y);
    data.set([r, g, b, a], (y * width + x) * 4);
  }
  return { width, height, data };
}

test("four source colours keep their colours and marker positions across extraction counts", () => {
  const values = [[31, 18, 53], [103, 56, 160], [255, 141, 96], [189, 237, 212]];
  const im = image(16, 4, x => values[Math.floor(x / 4)]);
  const bins = photoHistogram(im);
  for (const mode of ["balanced", "highlights", "vivid"]) {
    for (const n of [2, 3, 4]) {
      const palette = extractPhotoStops(bins, n, mode);
      assert.equal(new Set(palette.map(p => p.color)).size, n);
      for (const p of palette) assert.deepEqual(pixelAt(im, p.x, p.y), p);
      assert.deepEqual(extractPhotoStops(bins, n, mode), palette);
    }
    assert.deepEqual(extractPhotoStops(bins, 4, mode).map(p => p.color), ["#1f1235", "#6738a0", "#ff8d60", "#bdedd4"]);
  }
});

test("transparent pixels stay unselected and flat images repeat their own hue", () => {
  const im = image(8, 8, (x, y) => x === y ? [0, 110, 66, 230] : [255, 255, 255, 0]);
  assert.equal(pixelAt(im, -2, 100), null);
  const four = extractPhotoStops(photoHistogram(im), 4);
  assert.deepEqual(four.map(p => p.color), Array(4).fill("#006e42"));
  for (const stop of four) assert.deepEqual(pixelAt(im, stop.x, stop.y), stop);
  assert.deepEqual(extractPhotoStops(photoHistogram(image(10, 1, () => [0, 0, 0, 2])), 2), []);
});

test("sparse transparent artwork can retain multiple isolated colours between sampling points", () => {
  const im = image(300, 302, (x,y) => x === 0 && y === 0 ? [255, 0, 0] :
    x === 299 && y === 301 ? [0, 255, 0] : [0, 0, 0, 0]);
  const stops = extractPhotoStops(photoHistogram(im), 2);
  assert.deepEqual(stops, [{ color: "#ff0000", x: 0, y: 0 }, { color: "#00ff00", x: 299, y: 301 }]);
});

test("emphasis changes the actual palette of a scene with a small bright and a small vivid accent", () => {
  const im = image(100, 4, x => x < 60 ? [65, 55, 70] : x < 89 ? [135, 125, 132] :
    x < 96 ? [255, 251, 220] : [238, 19, 146]);
  const bins = photoHistogram(im);
  const balanced = extractPhotoStops(bins, 2);
  const highlights = extractPhotoStops(bins, 2, "highlights");
  const vivid = extractPhotoStops(bins, 2, "vivid");
  assert.notDeepEqual(highlights, balanced);
  assert.notDeepEqual(vivid, balanced);
  assert.ok(highlights.some(p => p.color === "#fffbdc"));
  assert.ok(vivid.some(p => p.color === "#ee1392"));
});
