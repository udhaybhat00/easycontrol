import { test } from "node:test";
import assert from "node:assert/strict";
import { releasePolicy } from "../scripts/verify-macos-release.mjs";

const distribution = { identifier: "com.easycontrol.desktop", minimumVersion: "10.15", signingInfo: "Authority=Developer ID Application: Example", signatureValid: true, gatekeeperAccepted: true, stapled: true };
test("development signing cannot be mistaken for a public release", () => {
  const checks = releasePolicy({ ...distribution, signingInfo: "Authority=Apple Development: Example", stapled: false });
  assert.equal(checks.find(check => check.name === "Developer ID Application signature").pass, false);
  assert.equal(checks.find(check => check.name === "Stapled notarization ticket").pass, false);
});
test("QA identities, invalid signatures and unsupported OS minimums fail the release gate", () => {
  for (const invalid of [{identifier:"com.easycontrol.desktop.qa"},{minimumVersion:"10.13"},{minimumVersion:"garbage"},{signatureValid:false},{gatekeeperAccepted:false}]) {
    assert.ok(releasePolicy({ ...distribution, ...invalid }).some(check => !check.pass));
  }
});
test("complete distribution metadata satisfies only the automated policy", () => {
  assert.ok(releasePolicy(distribution).every(check => check.pass));
});
