import assert from "node:assert/strict";
import test from "node:test";
import { DEFAULT_FACE, durationText, facePreferences, minuteTrace, plateForDay, readTimer, remaining } from "../web/js/desk-state.js";

test("the companion accepts older or damaged local preferences with independent field defaults", () => {
  assert.deepEqual(facePreferences(), DEFAULT_FACE);
  assert.deepEqual(facePreferences({layout:"instruments",scenery:"bad",format:"24",seconds:1}),
    { layout:"instruments",scenery:"glasshouse",format:"24",seconds:false });
});
test("daily scenery uses the local calendar day, including across a daylight saving boundary", () => {
  assert.equal(plateForDay("plate-3",new Date()),3);
  assert.equal(plateForDay("plate-7",new Date()),null);
  const morning=new Date(2026,2,29,1,10),night=new Date(2026,2,29,23,59);
  assert.equal(plateForDay("daily",morning),plateForDay("daily",night));
  assert.equal(plateForDay("daily",new Date(2026,2,30)),(plateForDay("daily",night)+1)%6);
});
test("clock countdowns use the wall deadline across sleep, reload and backwards time", () => {
  const start = 1234000, timer = readTimer({mode:"running",minutes:15,deadline:start+900000});
  assert.equal(remaining(timer,start-5000),900000);
  assert.equal(durationText(remaining(timer,start+10301)),"14:50");
  assert.equal(remaining(timer,start+3000000),0);
  assert.equal(remaining(readTimer({mode:"paused",minutes:1,left:1200}),9000000),1200);
  for (const t of [{ minutes:0 }, {mode:"running",minutes:5,deadline:Infinity},{mode:"paused",minutes:4,left:240001}])
    assert.equal(readTimer(t),null);
});
test("minute traces include zero, tolerate missing rows, and break at long gaps or backwards time", () => {
  const r=minuteTrace([37,40,42,45,48,52,65,63,93,105],[99,0,1,null,2,3,4,5,6,98],100);
  assert.equal(r.min,0); assert.equal(r.max,6);
  assert.equal(r.path.match(/M/g).length,5);
  assert.equal(r.path.match(/L/g).length,2);
  assert.doesNotMatch(r.path,/NaN|Infinity/);
  assert.deepEqual(minuteTrace([90,95],[],100),{path:"",min:null,max:null});
});
