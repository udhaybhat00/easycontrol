import test from "node:test";
import assert from "node:assert/strict";
import { completeSession, sessionDuration } from "../.test-build/session-history.js";

test("disconnect records completion and duration only for its authenticated session", () => {
  const first = { id: "first", address: "127.0.0.1:5899", started: 1000, result: "Authenticated" };
  const other = { ...first, id: "other" };
  const completed = completeSession([first, other], "first", 66000);
  assert.equal(completed[0].result, "Completed");
  assert.equal(sessionDuration(completed[0]), "1m 5s");
  assert.equal(completed[1], other);
  assert.equal(first.result, "Authenticated");
  assert.deepEqual(completeSession(completed, "first", 90000), completed);
  assert.deepEqual(completeSession(completed, "missing", 90000), completed);
});

test("history never invents a duration for old entries or negative clock changes", () => {
  const old = { id: "old", address: "127.0.0.1:5899", started: 1000, result: "Authenticated" };
  assert.equal(sessionDuration(old), "—");
  assert.equal(sessionDuration({ ...old, result: "Completed" }), "Not recorded");
  assert.equal(sessionDuration(completeSession([old], "old", 500)[0]), "0m 0s");
});
