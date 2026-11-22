import test from "node:test";
import assert from "node:assert/strict";
import { createArrangement, groupOffset, positionedZone, shiftPosition, turnAngle } from "../web/js/arrange-model.js";

const zone = (key, x, y) => ({ key, device: { online: true }, config: { layout: { shape: "strip", x, y, angle: 0 } }, positions: [[x,y]] });

test("group translation preserves relative placement at an edge, with grid anchoring", () => {
  const poses = [{ x: .17, y: .21 }, { x: 1.56, y: .81 }];
  assert.deepEqual(groupOffset(poses, .15, -.4, true), { dx: .04, dy: -.21 });
  assert.deepEqual(groupOffset(poses, .06, .04, true), { dx: .04, dy: .04 });
  assert.equal(turnAngle(-179, -15), 166);
});

test("alignment and spacing are off-device, ordered and reversible", () => {
  const zones = [zone("A", .8, .7), zone("B", .1, .4), zone("C", .2, .1)];
  const a = createArrangement();
  a.align(zones, "y", false);
  assert.deepEqual(zones.map(z => a.pose(z).y), [.4, .4, .4]);
  a.align(zones, "x", true);
  assert.deepEqual(zones.map(z => a.pose(z).x), [.8, .1, .45]);
  a.history(false); a.history(false);
  assert.equal(a.changed.size, 0);
  a.history(true); a.history(true);
  assert.equal(a.pose(zones[2]).x, .45);
  a.history(false); a.edit(zones, p => shiftPosition(p, .01, 0));
  assert.equal(a.future.length, 0);
  assert.deepEqual(zones.map(z => z.config.layout.x), [.8, .1, .2]);
});

test("rotating a reversed fan bank moves fan centres but keeps each fan's LEDs upright", () => {
  const z = { config: { layout: { x: .5, y: .5, angle: 0, shape: "fans", fans: 2, size: .2, leds_per_fan: 2, reverse: true } },
    positions: [[.612,.6],[.612,.4],[.388,.6],[.388,.4]], fan_index: [1,1,0,0] };
  const moved = positionedZone(z, { x: .6, y: .4, angle: 90 });
  assert.deepEqual(moved.positions, [[.6,.612],[.6,.412],[.6,.388],[.6,.188]]);
  assert.deepEqual(positionedZone({ ...z, fan_index: null }, moved.config.layout).positions, moved.positions);
  assert.equal(z.config.layout.angle, 0);
  const strip = { ...zone("s", .2,.3), positions: [[.1,.3],[.2,.3],[.3,.3]] };
  assert.deepEqual(positionedZone(strip, { x: .6,y: .4,angle: 90 }).positions, [[.6,.3],[.6,.4],[.6,.5]]);
});
