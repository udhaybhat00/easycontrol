import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { decodeFramePacket } from "../.test-build/bridge.js";
import { mapCoordinates, mouseButton, scrollSteps, validEndpoint } from "../.test-build/viewer-utils.js";
import { HeldRemoteKeys, remoteKeyCode } from "../.test-build/keymap.js";

test("Retina scaling uses monitor input bounds with letterbox offsets and negative origins", () => {
  const rect = { left: 100, top: 50, width: 720, height: 450 };
  const monitor = { x: -1440, y: 0, width: 1440, height: 900 };
  assert.deepEqual(mapCoordinates(460, 275, rect, monitor), { x: -720, y: 450 });
  assert.deepEqual(mapCoordinates(820, 500, rect, monitor), { x: -1, y: 899 });
  assert.equal(mapCoordinates(99, 275, rect, monitor), undefined);
  assert.equal(mapCoordinates(100, 50, { ...rect, width: 0 }, monitor), undefined);
});
test("frame decoder handles padded rows and rejects truncated or invalid frames", () => {
  const bytes = new Uint8Array(32 + 16);
  bytes.set([69, 67, 70, 49]);
  const header = new DataView(bytes.buffer);
  header.setBigUint64(4, 1n, true); header.setUint32(12, 1, true); header.setUint32(16, 2, true); header.setUint32(20, 8, true);
  bytes.set([1, 2, 3, 4], 32); bytes.set([5, 6, 7, 8], 40);
  assert.deepEqual([...decodeFramePacket(bytes).pixels], [1, 2, 3, 4, 5, 6, 7, 8]);
  assert.equal(decodeFramePacket(bytes.subarray(0, 47)), undefined);
  bytes[0] = 0;
  assert.equal(decodeFramePacket(bytes), undefined);
});
test("keyboard mapping preserves modifiers and leaves local shortcuts alone", () => {
  const key = code => ({ code, metaKey: false, altKey: false });
  assert.equal(remoteKeyCode(key("ShiftLeft")), 0x11);
  assert.equal(remoteKeyCode(key("ControlLeft")), 0x10);
  assert.equal(remoteKeyCode(key("KeyA")), 65);
  assert.equal(remoteKeyCode({ ...key("KeyQ"), metaKey: true }), undefined);
  assert.equal(remoteKeyCode({ ...key("Tab"), altKey: true }), undefined);
});
test("mouse buttons and wheel units match the Rust input backend", () => {
  assert.equal(mouseButton(0), 1); assert.equal(mouseButton(1), 3); assert.equal(mouseButton(2), 2);
  assert.equal(scrollSteps(4000, 0), 10); assert.equal(scrollSteps(-40, 0), -1);
});

test("shared modifiers release only after both physical keys are up", () => {
  const held = new HeldRemoteKeys();
  assert.equal(held.press("ShiftLeft", 0x11), 0x11);
  assert.equal(held.press("ShiftLeft", 0x11), undefined);
  assert.equal(held.press("ShiftRight", 0x11), undefined);
  assert.equal(held.release("ShiftLeft"), undefined);
  assert.equal(held.release("ShiftRight"), 0x11);
  held.press("ControlLeft", 0x10); held.press("ControlRight", 0x10); held.press("KeyA", 65);
  assert.deepEqual(held.releaseAll(), [0x10, 65]);
  assert.deepEqual(held.releaseAll(), []);
});

test("punctuation and numpad Enter use the host wire key subset", () => {
  for (const [code, expected] of [["Semicolon",0xba],["Equal",0xbb],["Minus",0xbd],["Backslash",0xdc],["Quote",0xde],["NumpadEnter",0x0d]]) {
    assert.equal(remoteKeyCode({code,metaKey:false,altKey:false}), expected);
  }
});
test("address validation rejects invalid numeric addresses and ports", () => {
  assert.ok(validEndpoint("127.0.0.1:5899")); assert.ok(validEndpoint("[::1]:5899"));
  for (const value of ["localhost", "999.0.0.1:5899", "127.0.0.1:0", "127.0.0.1:65536"]) assert.equal(validEndpoint(value), false);
});

test("IPv6 syntax and ambiguous IPv4 literals are validated before connecting", () => {
  for (const value of ["[:::]:5899", "[:]:5899", "[1:2:3]:5899", "[1:2:3:4:5:6:7:8:9]:5899", "127.00.0.1:5899"]) assert.equal(validEndpoint(value), false, value);
  assert.equal(validEndpoint("[2001:db8::1]:5899"), true);
  assert.equal(validEndpoint("[::ffff:192.0.2.1]:5899"), true);
});

test("production and QA policies allow internal binary IPC without public frame endpoints", () => {
  for (const file of ["tauri.conf.json", "tauri.qa.conf.json"]) {
    const config = JSON.parse(readFileSync(new URL(`../src-tauri/${file}`, import.meta.url), "utf8"));
    const policy = config.app.security.csp;
    const connect = policy.split(";").find(value => value.trim().startsWith("connect-src"));
    assert.ok(connect.includes("ipc:"));
    assert.ok(connect.includes("http://ipc.localhost"));
    assert.ok(!connect.includes("*"));
    assert.ok(!/wss?:/.test(connect));
  }
});
