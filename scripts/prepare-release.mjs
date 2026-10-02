import { execFileSync } from "node:child_process";
import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";

const tag = process.env.RELEASE_TAG;
const match = /^v((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)(?:-(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*)(?:\.(?:0|[1-9]\d*|\d*[A-Za-z-][0-9A-Za-z-]*))*)?)$/.exec(tag ?? "");
if (!match) {
  throw new Error("RELEASE_TAG must look like v1.2.3 or v1.2.3-beta.1.");
}
const version = match[1];
const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const metadata = JSON.parse(execFileSync("cargo", [
  "metadata", "--manifest-path", "src-tauri/Cargo.toml",
  "--format-version", "1", "--no-deps", "--locked",
], { encoding: "utf8" }));
const cargoPackage = metadata.packages.find((pkg) => pkg.name === "codex-hud");
const lock = readJson("package-lock.json");
const versions = {
  "package.json": readJson("package.json").version,
  "package-lock.json": lock.version,
  "package-lock.json root package": lock.packages?.[""]?.version,
  "src-tauri/tauri.conf.json": readJson("src-tauri/tauri.conf.json").version,
  "src-tauri/Cargo.toml": cargoPackage?.version,
};
for (const [path, actual] of Object.entries(versions)) {
  if (actual !== version) {
    throw new Error(`${path} version ${actual} does not match tag ${tag}.`);
  }
}

let notes = `Automated release for ${tag}.`;
if (existsSync("CHANGELOG.md")) {
  const lines = readFileSync("CHANGELOG.md", "utf8").split(/\r?\n/);
  const start = lines.findIndex((line) => {
    const heading = /^\s*##\s+\[?(v?\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)\]?(?:\s|$)/.exec(line);
    return heading && (heading[1] === version || heading[1] === tag);
  });
  if (start >= 0) {
    let end = start + 1;
    while (end < lines.length && !/^\s*##\s+/.test(lines[end])) end++;
    notes = lines.slice(start + 1, end).join("\n").trim() || notes;
  }
}
mkdirSync("artifacts", { recursive: true });
writeFileSync("artifacts/release-notes.md", `${notes}\n`);
const outputs = {
  executable_filename: `CodexHUD-${tag}-win-x64.exe`,
  is_prerelease: String(version.includes("-")),
};
if (process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT,
    Object.entries(outputs).map(([key, value]) => `${key}=${value}\n`).join(""));
}
console.log(`Validated ${tag}: ${outputs.executable_filename} (prerelease: ${outputs.is_prerelease})`);
