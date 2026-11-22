// All coordinates refer to the displayed, orientation-corrected bitmap. Transparent pixels never become swatches.
const linear = Array.from({ length: 256 }, (_, i) => {
  const s = i / 255;
  return s <= .04045 ? s / 12.92 : ((s + .055) / 1.055) ** 2.4;
});

function oklab(r, g, b) {
  const l = Math.cbrt(.4122214708 * r + .5363325363 * g + .0514459929 * b);
  const m = Math.cbrt(.2119034982 * r + .6806995451 * g + .1073969566 * b);
  const s = Math.cbrt(.0883024619 * r + .2817188376 * g + .6299787005 * b);
  return [.2104542553 * l + .793617785 * m - .0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + .4505937099 * s,
    .0259040371 * l + .7827717662 * m - .808675766 * s];
}
const distance = (a, b) => (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 + (a[2] - b[2]) ** 2;
const hex = (r, g, b) => "#" + [r, g, b].map(c => c.toString(16).padStart(2, "0")).join("");

export function pixelAt(image, x, y) {
  x = Math.min(image.width - 1, Math.max(0, Math.round(x)));
  y = Math.min(image.height - 1, Math.max(0, Math.round(y)));
  const i = (y * image.width + x) * 4;
  const data = image.data;
  return data[i + 3] < 24 ? null : { color: hex(data[i], data[i + 1], data[i + 2]), x, y };
}

export function photoHistogram(image) {
  const { width, height, data } = image, bins = new Map();
  // Bound initial evaluation of large photos. An offset grid covers both wide panoramas and tall portraits.
  const stride = Math.max(1, Math.ceil(Math.sqrt(width * height / 18000)));
  const sx = Math.min(width - 1, stride >> 1), sy = Math.min(height - 1, stride >> 1);
  function addPixel(x, y) {
    const i = (y * width + x) * 4, alpha = data[i + 3];
    if (alpha < 24) return;
    const r = data[i], g = data[i + 1], b = data[i + 2];
    const key = ((r >> 3) << 10) | ((g >> 3) << 5) | (b >> 3);
    const bin = bins.get(key);
    // Among similar pixels, mark the point nearest the image centre so flat colours remain easy to locate.
    const radius = ((x + .5) / width - .5) ** 2 + ((y + .5) / height - .5) ** 2;
    if (bin) {
      bin.mass += alpha / 255;
      if (radius >= bin.radius) return;
    }
    bins.set(key, { x, y, radius, color: hex(r, g, b), mass: bin?.mass || alpha / 255, lab: oklab(linear[r], linear[g], linear[b]) });
  }
  for (let y = sy; y < height; y += stride) for (let x = sx; x < width; x += stride) addPixel(x, y);
  // A tiny opaque mark on a transparent image can fall between grid samples.
  if (!bins.size && stride > 1) {
    for (let i = 3; i < data.length; i += 4) if (data[i] >= 24) {
      const p = (i - 3) / 4;
      addPixel(p % width, Math.floor(p / width));
    }
  }
  return [...bins.values()];
}

// Fit groups in perceptual colour space, then pick an actual source pixel nearest each centre.
// Switching the emphasis changes how much influence bright or saturated parts of the photo have.
export function extractPhotoStops(bins, count, emphasis = "balanced") {
  if (!bins.length || !Number.isInteger(count) || count < 2 || count > 4) return [];
  const weights = bins.map(({ mass, lab: [l, a, b] }) => {
    if (emphasis === "highlights") return mass * (.045 + 5 * l ** 4);
    if (emphasis === "vivid") return mass * (.07 + Math.min(1, Math.hypot(a, b) * 5) ** 2 * 3);
    return mass;
  });
  const centers = [];
  const nearest = bins.map(() => Infinity);
  let next = weights.reduce((best, weight, i) => weight > weights[best] ? i : best, 0);
  for (let c = 0; c < Math.min(count, bins.length); c++) {
    centers.push(bins[next].lab);
    let best = -1;
    for (let i = 0; i < bins.length; i++) {
      nearest[i] = Math.min(nearest[i], distance(bins[i].lab, centers[c]));
      const score = nearest[i] * Math.sqrt(weights[i]);
      if (score > best) { best = score; next = i; }
    }
    if (best <= 0) break;
  }
  const assignments = new Uint8Array(bins.length);
  for (let pass = 0; pass < 8; pass++) {
    const sums = centers.map(() => [0, 0, 0, 0]);
    let moved = false;
    for (let i = 0; i < bins.length; i++) {
      let group = 0, best = distance(bins[i].lab, centers[0]);
      for (let c = 1; c < centers.length; c++) {
        const d = distance(bins[i].lab, centers[c]);
        if (d < best) { best = d; group = c; }
      }
      moved ||= assignments[i] !== group;
      assignments[i] = group;
      const sum = sums[group], weight = weights[i], lab = bins[i].lab;
      for (let j = 0; j < 3; j++) sum[j] += weight * lab[j];
      sum[3] += weight;
    }
    for (let c = 0; c < centers.length; c++) if (sums[c][3]) centers[c] = sums[c].slice(0, 3).map(v => v / sums[c][3]);
    if (pass && !moved) break;
  }
  const representatives = centers.map(() => null), errors = centers.map(() => Infinity);
  for (let i = 0; i < bins.length; i++) {
    const group = assignments[i], delta = distance(bins[i].lab, centers[group]);
    if (delta < errors[group]) { representatives[group] = bins[i]; errors[group] = delta; }
  }
  const stops = representatives.filter(Boolean).sort((a, b) => a.lab[0] - b.lab[0]).map(({ color, x, y }) => ({ color, x, y }));
  // A monochrome image stays monochrome, including in effects that require at least two gradient stops.
  while (stops.length < count) stops.push({ ...stops.at(-1) });
  return stops;
}
