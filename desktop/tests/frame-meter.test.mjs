import test from "node:test";
import assert from "node:assert/strict";
import { FrameMeter } from "../.test-build/frame-meter.js";
const frame = { width: 2880, height: 1800, dropped: 2 };

test("a stalled stream reports zero current FPS and bandwidth without another frame", () => {
  const meter = new FrameMeter(0);
  meter.receive(frame, 100, 0);
  const live = meter.sample(1000);
  assert.equal(live.state, "live");
  assert.equal(live.fps, 1);
  assert.equal(live.bytesPerSecond, 100);
  const stalled = meter.sample(4000);
  assert.equal(stalled.state, "stalled");
  assert.equal(stalled.fps, 0);
  assert.equal(stalled.bytesPerSecond, 0);
  assert.equal(stalled.frameAgeMs, 4000);
  assert.equal(stalled.width, 2880);
  assert.equal(stalled.dropped, 2);
});

test("fresh frames recover the meter after a stall", () => {
  const meter = new FrameMeter(0);
  meter.receive(frame, 100, 0);
  meter.sample(4000);
  meter.receive(frame, 200, 4100);
  const recovered = meter.sample(5000);
  assert.equal(recovered.state, "live");
  assert.equal(recovered.fps, 1);
  assert.equal(recovered.bytesPerSecond, 200);
  assert.equal(recovered.frameAgeMs, 900);
});

test("a new session cannot inherit previous frame health or metadata", () => {
  const meter = new FrameMeter(10000);
  const waiting = meter.sample(11000);
  assert.equal(waiting.state, "waiting");
  assert.equal(waiting.frameAgeMs, undefined);
  assert.equal(waiting.width, 0);
  assert.equal(waiting.fps, 0);
});
