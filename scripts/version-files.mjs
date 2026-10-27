import { readFileSync, writeFileSync } from "node:fs";

const root = new URL("../", import.meta.url);

const SEMVER = String.raw`\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?`;

export const FILES = [
  {
    url: new URL("Cargo.toml", root),
    find: new RegExp(`^version = "(${SEMVER})"$`, "m"),
    put: (v) => `version = "${v}"`,
  },
  {
    url: new URL("app/src-tauri/tauri.conf.json", root),
    find: new RegExp(`"version": "(${SEMVER})"`),
    put: (v) => `"version": "${v}"`,
  },
  {
    url: new URL("app/ui/package.json", root),
    find: new RegExp(`"version": "(${SEMVER})"`),
    put: (v) => `"version": "${v}"`,
  },
];

export function currentVersion() {
  return FILES[0].find.exec(readFileSync(FILES[0].url, "utf8"))?.[1];
}

export function relativePaths() {
  return FILES.map((f) => f.url.pathname.slice(root.pathname.length));
}

export function allVersions() {
  return relativePaths().map((path, i) => [path, FILES[i].find.exec(readFileSync(FILES[i].url, "utf8"))?.[1]]);
}

export function writeVersion(version) {
  for (const file of FILES) {
    const text = readFileSync(file.url, "utf8");
    const found = file.find.exec(text);
    if (found === null) {
      throw new Error(`no version found in ${file.url.pathname}`);
    }
    writeFileSync(file.url, text.replace(found[0], file.put(version)));
  }
}
