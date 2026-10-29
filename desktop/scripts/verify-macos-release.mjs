import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

export function releasePolicy({ identifier, minimumVersion, signingInfo, signatureValid, gatekeeperAccepted, stapled }) {
  const version = /^\d+\.\d+(?:\.\d+)?$/.test(minimumVersion) ? minimumVersion.split(".").map(Number) : [];
  return [
    { name: "Production bundle identity", pass: identifier === "com.easycontrol.desktop" },
    { name: "Supported macOS minimum", pass: version.length >= 2 && (version[0] > 10 || (version[0] === 10 && version[1] >= 15)) },
    { name: "Strict bundle signature", pass: !!signatureValid },
    { name: "Developer ID Application signature", pass: /^Authority=Developer ID Application:/m.test(signingInfo) },
    { name: "Gatekeeper assessment", pass: !!gatekeeperAccepted },
    { name: "Stapled notarization ticket", pass: !!stapled },
  ];
}

function main() {
  if (process.platform !== "darwin") {
    console.error("Run this release gate on macOS. No other platform is validated by this command.");
    process.exitCode = 1;
    return;
  }
  const app = path.resolve(process.argv[2] || path.join(path.dirname(fileURLToPath(import.meta.url)), "../src-tauri/target/release/bundle/macos/EasyControl.app"));
  const run = (command, args) => spawnSync(command, args, { encoding: "utf8", timeout: 60_000 });
  const plist = key => {
    const result = run("/usr/libexec/PlistBuddy", ["-c", `Print :${key}`, path.join(app, "Contents/Info.plist")]);
    return result.status === 0 ? result.stdout.trim() : "";
  };
  const signing = run("codesign", ["-d", "--verbose=4", app]);
  const signature = run("codesign", ["--verify", "--deep", "--strict", app]);
  const gatekeeper = run("spctl", ["--assess", "--type", "execute", app]);
  const staple = run("xcrun", ["stapler", "validate", app]);
  const checks = releasePolicy({
    identifier: plist("CFBundleIdentifier"),
    minimumVersion: plist("LSMinimumSystemVersion"),
    signingInfo: `${signing.stdout || ""}\n${signing.stderr || ""}`,
    signatureValid: signature.status === 0,
    gatekeeperAccepted: gatekeeper.status === 0,
    stapled: staple.status === 0,
  });
  for (const check of checks) console.log(`${check.pass ? "PASS" : "BLOCKED"} ${check.name}`);
  if (checks.some(check => !check.pass)) {
    console.error("Not eligible for public distribution. Supply a Developer ID signature and notarization; never bypass these checks.");
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
