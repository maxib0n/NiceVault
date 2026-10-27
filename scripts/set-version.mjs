import { allVersions, currentVersion, writeVersion } from "./version-files.mjs";

const next = process.argv[2];
const current = currentVersion();

if (next === undefined) {
  console.log(`nicevault ${current}`);
  for (const [path, v] of allVersions()) console.log(`  ${path}  ${v}`);
  process.exit(0);
}

if (!/^\d+\.\d+\.\d+(?:-(?:beta|rc)\.\d+)?$/.test(next)) {
  console.error(`not a version: ${next} — expected MAJOR.MINOR.PATCH[-beta.N|-rc.N]`);
  process.exit(1);
}

writeVersion(next);
console.log(`nicevault ${current} -> ${next}`);
