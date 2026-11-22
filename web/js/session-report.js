import { elapsedText, formatValue } from "./session-traces.js";

const width = 1440, margin = 72;
const colors = { paper: "#211c28", plot: "#2a2431", ink: "#f7eee2", quiet: "#d1c1c8", grid: "#514553",
  runs: ["#ffc984", "#86dac6"] };
const sans = '"Segoe UI", Arial, sans-serif';

// Take just the visible signal and user-selected labels before opening the file picker.
export function chartReport(model, recordings, includeMarkers) {
  const runs = model.traces.map((trace, index) => {
    const run = recordings[index];
    return { name: run.name || run.annotation || "Untitled workload", duration: trace.duration,
      recording: !run.ended_reason, measured: trace.count, total: trace.rows.length, captured: !!trace.metric,
      mean: trace.mean, segments: trace.segments };
  });
  const events = includeMarkers ? model.events.map(({index, marker}) => ({index, time: marker.elapsed, label: marker.label}))
    .sort((a,b) => a.time - b.time || a.index - b.index) : [];
  const title = model.metric.name === model.metric.id ? "Selected reading" : model.metric.name;
  return { title, runs, events, duration: model.duration, bounds: model.bounds, transform: model.transform };
}

function wrap(ctx, text, maxWidth) {
  const rows = [];
  let row = "";
  for (const char of text.trim().replace(/\s+/g, " ")) {
    if (row && ctx.measureText(row + char).width > maxWidth) {
      const space = row.lastIndexOf(" ");
      if (space > 0) { rows.push(row.slice(0, space)); row = row.slice(space + 1); }
      else { rows.push(row); row = ""; }
    }
    if (row || char !== " ") row += char;
  }
  if (row) rows.push(row);
  return rows;
}

function elapsedTicks(duration) {
  if (!duration) return [0];
  const target = duration / 4;
  const steps = [.001,.002,.005,.01,.02,.05,.1,.2,.5,1,2,5,10,15,30,60,120,300,600,900,1800];
  const step = target > steps.at(-1) ? target : steps.reduce((nearest, value) =>
    Math.abs(Math.log(value / target)) < Math.abs(Math.log(nearest / target)) ? value : nearest);
  const ticks = [0];
  // Leave room for the actual endpoint, which often falls a fraction after a whole second.
  for (let time = step; time < duration * .88; time += step) ticks.push(time);
  return [...ticks, duration];
}

export function renderChartReport(report) {
  if (!report.bounds) throw new Error("Choose a reading with measured samples to export.");
  const canvas = document.createElement("canvas"), ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("The chart image could not be created.");
  const { runs, duration, transform, bounds } = report;
  const right = width - margin, inner = width - 2 * margin, gap = 40;
  const col = (inner - gap * (runs.length - 1)) / runs.length;
  const heading = '500 56px "Sitka Display", Georgia, serif';
  const legendFont = "600 25px " + sans, eventFont = "21px " + sans;
  ctx.font = heading;
  const titleRows = wrap(ctx, report.title, inner);
  ctx.font = legendFont;
  const names = runs.map(run => wrap(ctx, run.name, col - 62));
  const legendTop = 126 + titleRows.length * 67;
  const detailsY = legendTop + Math.max(...names.map(lines => lines.length)) * 31 + 8;
  const top = detailsY + 60, bottom = top + 316;
  const eventsY = bottom + 116;
  const eventCol = (inner - gap) / 2;
  ctx.font = eventFont;
  const eventRows = [];
  let rowY = eventsY + 50;
  for (let i = 0; i < report.events.length; i += 2) {
    const pair = report.events.slice(i, i + 2).map(event => ({
      ...event, lines: wrap(ctx, event.label, eventCol - 185),
    }));
    eventRows.push({pair, y: rowY});
    rowY += Math.max(...pair.map(event => event.lines.length)) * 26 + 15;
  }
  const footer = eventRows.length ? rowY + 30 : bottom + 120;
  canvas.width = width; canvas.height = footer + 44;
  function label(text, x, y, font, color = colors.ink, align = "left") {
    ctx.font = font; ctx.fillStyle = color; ctx.textAlign = align; ctx.fillText(text, x, y);
  }
  function line(x1, y1, x2, y2, stroke, weight, dash = []) {
    ctx.beginPath(); ctx.strokeStyle = stroke; ctx.lineWidth = weight; ctx.setLineDash(dash);
    ctx.moveTo(x1,y1); ctx.lineTo(x2,y2); ctx.stroke(); ctx.setLineDash([]);
  }
  ctx.fillStyle = colors.paper; ctx.fillRect(0,0,width,canvas.height);
  label("Airglow", margin, 62, '600 30px "Sitka Display", Georgia, serif', colors.runs[0]);
  label(runs.length === 2 ? "Recording comparison" : "Recording", right, 59, "21px " + sans, colors.quiet, "right");
  line(margin, 82, right, 82, colors.grid, 1);
  titleRows.forEach((row, i) => label(row, margin, 148 + i * 67, heading));
  runs.forEach((run, index) => {
    const left = margin + index * (col + gap), color = colors.runs[index];
    line(left, legendTop - 8, left + 43, legendTop - 8, color, 4, index ? [8,5] : []);
    names[index].forEach((name, i) => label(name, left + 57, legendTop + i * 31, legendFont));
    const prefix = index ? "B" : "A";
    const coverage = run.captured ? `${run.measured}/${run.total} samples` : "Reading not recorded";
    const mean = run.measured ? `   ·   Mean ${formatValue(run.mean, transform)}` : "";
    label(`${prefix}   ·   ${elapsedText(run.duration,true)}${run.recording ? " so far" : ""}   ·   ${coverage}${mean}`, left, detailsY, "18px " + sans, colors.quiet);
  });
  const axisFont = "20px " + sans;
  const tickValue = fraction => formatValue((bounds.hi - (bounds.hi - bounds.lo) * fraction - transform.offset) / transform.scale, transform);
  ctx.font = axisFont;
  const left = margin + Math.max(...[0,.25,.5,.75,1].map(n => ctx.measureText(tickValue(n)).width)) + 19;
  const x = t => left + (duration ? t / duration : 0) * (right - left);
  const y = value => bottom - (value * transform.scale + transform.offset - bounds.lo) / (bounds.hi - bounds.lo) * (bottom - top);
  ctx.fillStyle = colors.plot; ctx.fillRect(left, top, right - left, bottom - top);
  for (const part of [0,.25,.5,.75,1]) {
    const at = top + (bottom - top) * part;
    line(left, at, right, at, colors.grid, 1);
    label(tickValue(part), left - 15, at + 7, axisFont, colors.quiet, "right");
  }
  const times = elapsedTicks(duration);
  times.forEach((time,index) => {
    const at = x(time);
    line(at, bottom, at, bottom + 6, colors.grid, 2);
    label(elapsedText(time, true), at, bottom + 32, axisFont, colors.quiet, index === 0 ? "left" : index === times.length - 1 ? "right" : "center");
  });
  label("Elapsed time from recording start", (left + right) / 2, bottom + 68, axisFont, colors.quiet, "center");
  ctx.lineJoin = "round"; ctx.lineCap = "round";
  runs.forEach((run, index) => {
    const color = colors.runs[index];
    ctx.lineWidth = 4; ctx.strokeStyle = ctx.fillStyle = color; ctx.setLineDash(index ? [9,6] : []);
    for (const segment of run.segments) {
      const first = segment[0];
      ctx.beginPath();
      if (segment.length === 1) {
        ctx.arc(x(first.time), y(first.value), 4, 0, 2 * Math.PI); ctx.fill();
      } else {
        ctx.moveTo(x(first.time), y(first.value));
        for (let i = 1; i < segment.length; i++) ctx.lineTo(x(segment[i].time), y(segment[i].value));
        ctx.stroke();
      }
    }
  });
  ctx.setLineDash([]);
  for (const event of report.events) {
    const at = x(event.time), h = top - 8 - event.index * 10;
    line(at, h - 5, at, h, colors.runs[event.index], 4);
  }
  if (eventRows.length) {
    label("Event markers", margin, eventsY, legendFont);
    for (const {pair, y: rowTop} of eventRows) pair.forEach((event, index) => {
      const start = margin + index * (eventCol + gap);
      label(`${event.index ? "B" : "A"}   ${elapsedText(event.time, true)}`, start, rowTop, eventFont, colors.runs[event.index]);
      event.lines.forEach((row,i) => label(row, start + 175, rowTop + i * 26, eventFont));
    });
  }
  line(margin, footer - 29, right, footer - 29, colors.grid, 1);
  label("Line breaks mark missing readings or sampling pauses longer than six seconds.", margin, footer, "19px " + sans, colors.quiet);
  return canvas;
}

export function reportPng(report) {
  const canvas = renderChartReport(report);
  return new Promise((resolve, reject) => canvas.toBlob(blob => blob ? resolve(blob) :
    reject(new Error("The chart PNG could not be encoded.")), "image/png"));
}
