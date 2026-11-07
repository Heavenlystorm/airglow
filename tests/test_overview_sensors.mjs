import assert from 'node:assert/strict';
import { test } from 'node:test';
import { readFileSync } from 'node:fs';
const source = readFileSync(new URL('../web/js/overview-sensors.js', import.meta.url), 'utf8');
const { sensorFamily, instrumentModel } = await import(`data:text/javascript;base64,${Buffer.from(source).toString('base64')}`);

test('instruments distinguish temperature, capacity, load and timelines', () => {
  assert.equal(sensorFamily({unit:'°C'}), 'thermal');
  assert.equal(sensorFamily({unit:'%', name:'Memory usage'}, 'auto/ram_load'), 'capacity');
  assert.equal(sensorFamily({unit:'%'}), 'load');
  assert.equal(sensorFamily({unit:'GB'}), 'capacity');
  assert.equal(sensorFamily({unit:'B/s'}), 'network');
  assert.equal(sensorFamily({unit:'W'}), 'power');
});
test('zero is empty progress and missing is unknown, histories ignore gaps', () => {
  const zero = instrumentModel({value:0, lo:0, hi:100, samples:[0,null,12,Infinity,6]});
  assert.equal(zero.fraction, 0); assert.equal(zero.missing, false);
  assert.deepEqual(zero.stats, {min:0,max:12,mean:6,count:3});
  const missing = instrumentModel({value:null,lo:0,hi:100,samples:[null]});
  assert.equal(missing.missing,true); assert.equal(missing.stats,null);
});
test('threshold markers share actual reading scale without inventing limits', () => {
  const m = instrumentModel({value:85,lo:20,hi:120,thresholds:[80,90]});
  assert.equal(m.fraction,.65); assert.equal(m.warning,.6); assert.equal(m.critical,.7);
  assert.equal(instrumentModel({value:0,lo:0,hi:1}).warning,null);
});
