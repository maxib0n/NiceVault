// NiceVault releases (modelled on Latente's scripts/release.mjs).
//
//   node scripts/release.mjs [--pre beta|rc] [--version X.Y.Z] [--dry-run]
//       cut: next version from the `Change: none|patch|minor|major` trailers since the last tag
//       (missing trailer = patch), the version files, CHANGELOG.md, a "release X.Y.Z" commit,
//       tag vX.Y.Z, and a draft of the user notes in release-notes/vX.Y.Z.json (it + en).
//       --dry-run prints all that and writes nothing.
//
//   node scripts/release.mjs publish [--dry-run] [--skip-build]
//       for the current version: scripts/build-all.sh, sign every dist/ artefact with
//       ~/.config/nicevault/updater-secret.key (app/src-tauri/examples/sign_update.rs -> .sig,
//       trusted comment binds version + platforms; the updater refuses anything else), write
//       dist/release/ (the assets, their .sig, SHA256SUMS, latest.json) and
//       `gh release create vX.Y.Z ...` on the public releases repo, with the owner's own `gh`
//       login. Nothing secret is embedded in the app and nothing is encrypted: the release is
//       public, plain files that any browser can download. --dry-run does everything except the
//       upload (and prints it).
//
//   node scripts/release.mjs bridge [--dry-run]
//       ONE-TIME migration for the 0.2.x apps that still read the old PRIVATE repo with an
//       embedded token and expect encrypted `.nvpack` assets there. After `publish`, this
//       repacks the already signed raw builds in dist/ in that old format
//       (dist/release-legacy/) and creates the same version's release on LEGACY_REPO, so those
//       apps update to the first build that reads the public repo. Needs
//       app/src-tauri/release-pack.key's old home, scripts/legacy/release-pack.key.
//
// Assets of a release (the website links to the first four by these exact names):
//   NiceVault-linux-x86_64.AppImage    Linux, self-contained (bundles WebKitGTK); also what the stick carries
//   NiceVault-linux-x86_64.tar.gz      Linux, the bare binary (needs the system WebKitGTK)
//   NiceVault-windows-x86_64.zip       Windows, NiceVault.exe
//   NiceVault-macos-universal.zip      macOS, NiceVault.app
//   NiceVault-linux-x86_64             the bare binary, what the bare-binary Linux build updates from
//   NiceVault-windows-x86_64.exe       the bare exe, what the Windows build updates from and the stick carries
//   <each>.sig, SHA256SUMS, latest.json
//
// Where releases live: the ONE place is app/src-tauri/tauri.conf.json `plugins.updater.endpoints[0]`
// (the app fetches latest.json from there). A GitHub URL
//   https://github.com/<owner>/<repo>/releases/latest/download/latest.json
// publishes to <owner>/<repo> with gh; any other HTTPS URL is a static host: assets are expected
// at <same folder>/v<version>/<file> and the upload is left to you (printed).
import { execFileSync } from "node:child_process";
import { createCipheriv, createDecipheriv, createHash, hkdfSync, randomBytes } from "node:crypto";
import { constants, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
import { allVersions, currentVersion, relativePaths, writeVersion } from "./version-files.mjs";

const root = new URL("../", import.meta.url);
const ROOT = fileURLToPath(root);
const KEY_DIR = `${homedir()}/.config/nicevault`;
const SECRET_KEY = `${KEY_DIR}/updater-secret.key`;
// The release-pack secret the 0.2.x apps embed: only the one-time `bridge` uses it.
const PACK_SECRET_FILE = new URL("scripts/legacy/release-pack.key", root);
// The old private releases repo the 0.2.x apps read; only `bridge` publishes there.
export const LEGACY_REPO = "maxib0n/NiceVault-releases";

// ---------------------------------------------------------------------------------------------
// pure parts (scripts/release.test.mjs)

const LEVELS = ["none", "patch", "minor", "major"];
const VALID = new Set(LEVELS);

export function changeFromMessage(message) {
  const matches = [...message.matchAll(/^\s*Change:\s*(\S+)\s*$/gim)];
  if (matches.length === 0) return "patch";
  const value = matches[matches.length - 1][1].toLowerCase();
  if (!VALID.has(value)) {
    throw new Error(`malformed Change trailer: "${value}"`);
  }
  return value;
}

export function highestChange(messages) {
  let best = "none";
  for (const message of messages) {
    const level = changeFromMessage(message);
    if (LEVELS.indexOf(level) > LEVELS.indexOf(best)) best = level;
  }
  return best;
}

function splitPre(version) {
  const m = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z]+)\.(\d+))?$/.exec(version);
  if (m === null) throw new Error(`not a version: ${version}`);
  const base = { major: +m[1], minor: +m[2], patch: +m[3] };
  const pre = m[4] !== undefined ? { channel: m[4], n: +m[5] } : null;
  return { base, pre };
}

function applyLevel(base, level) {
  if (level === "major") return { major: base.major + 1, minor: 0, patch: 0 };
  if (level === "minor") return { major: base.major, minor: base.minor + 1, patch: 0 };
  return { major: base.major, minor: base.minor, patch: base.patch + 1 };
}

const fmt = (b) => `${b.major}.${b.minor}.${b.patch}`;

export function computeNextVersion(current, level, channel = null) {
  const { base, pre } = splitPre(current);
  if (channel) {
    if (pre && pre.channel === channel) return `${fmt(base)}-${channel}.${pre.n + 1}`;
    if (pre) return `${fmt(base)}-${channel}.1`;
    if (level === "none") return null;
    return `${fmt(applyLevel(base, level))}-${channel}.1`;
  }
  if (pre) return fmt(base);
  if (level === "none") return null;
  return fmt(applyLevel(base, level));
}

export function buildChangelogSection(version, date, groups) {
  const headings = [
    ["major", "Breaking changes"],
    ["minor", "Features"],
    ["patch", "Fixes"],
    ["none", "Housekeeping"],
  ];
  let out = `## ${version} (${date})\n`;
  for (const [level, heading] of headings) {
    const lines = groups[level] ?? [];
    if (lines.length === 0) continue;
    out += `\n### ${heading}\n\n`;
    for (const subject of lines) out += `- ${subject}\n`;
  }
  return `${out}\n`;
}

export function buildNotesTemplate(groups) {
  const worthMentioning = [...(groups.major ?? []), ...(groups.minor ?? [])];
  const draft =
    worthMentioning.length > 0
      ? worthMentioning.map((s) => `- ${s}`).join("\n")
      : "- (nothing user-facing: write a line anyway)";
  return {
    _README:
      "DRAFT: rewrite as at most 3 short lines a user notices (no commit subjects, no internals, " +
      "no adjectives), in both languages, then run " +
      "`node scripts/release.mjs publish --dry-run`. Not the commit changelog.",
    en: draft,
    it: `DA TRADURRE:\n${draft}`,
  };
}

/** Notes file -> {it, en}; refuses the untouched draft. */
export function readyNotes(notes) {
  const { it, en } = notes;
  if (typeof it !== "string" || typeof en !== "string" || !it.trim() || !en.trim()) {
    throw new Error("release notes need both `it` and `en`");
  }
  if ("_README" in notes || it.startsWith("DA TRADURRE")) {
    throw new Error("release notes are still the draft: edit them and delete _README");
  }
  return { it, en };
}

/** Where releases go, from tauri.conf.json's updater endpoint. */
export function releaseTarget(endpoint) {
  const gh = /^https:\/\/github\.com\/([^/]+)\/([^/]+)\/releases\/latest\/download\/latest\.json$/.exec(endpoint);
  if (gh) {
    const repo = `${gh[1]}/${gh[2]}`;
    return { kind: "github", repo, assetBase: (v) => `https://github.com/${repo}/releases/download/v${v}` };
  }
  if (!/^https:\/\/[^/]+\/.*latest\.json$/.test(endpoint)) {
    throw new Error(`updater endpoint must be an https URL ending in latest.json: ${endpoint}`);
  }
  const dir = endpoint.slice(0, endpoint.lastIndexOf("/"));
  return { kind: "static", repo: null, assetBase: (v) => `${dir}/v${v}` };
}

/**
 * dist/ file -> the latest.json platform keys it serves. `linux-x86_64-appimage` is Tauri's
 * convention for the AppImage build (the app picks it when it runs from an AppImage, and the
 * stick takes it); plain `linux-x86_64` stays the bare binary, so 0.2.x apps and bare-binary
 * installs keep getting what they can run.
 */
export function platformKeysFor(filename) {
  if (filename === "NiceVault-linux-x86_64.AppImage") return ["linux-x86_64-appimage"];
  if (filename === "NiceVault-linux-x86_64") return ["linux-x86_64"];
  if (filename === "NiceVault-windows-x86_64.exe") return ["windows-x86_64"];
  if (filename === "NiceVault-macos-universal.zip") return ["darwin-aarch64", "darwin-x86_64"];
  return [];
}

/** Everything signed and uploaded, from dist/. The first four are the website's downloads. */
export const SITE_ASSETS = [
  "NiceVault-linux-x86_64.AppImage",
  "NiceVault-linux-x86_64.tar.gz",
  "NiceVault-windows-x86_64.zip",
  "NiceVault-macos-universal.zip",
];
/** Served only to the app's updater (and the stick). */
export const UPDATER_ASSETS = ["NiceVault-linux-x86_64", "NiceVault-windows-x86_64.exe"];
export const ARTEFACTS = [...SITE_ASSETS, ...UPDATER_ASSETS];

/** The platforms a signature binds: the manifest's keys, or (archives) what the archive holds. */
export function signedPlatformsFor(filename) {
  const keys = platformKeysFor(filename);
  if (keys.length > 0) return keys;
  if (filename === "NiceVault-linux-x86_64.tar.gz") return ["linux-x86_64"];
  if (filename === "NiceVault-windows-x86_64.zip") return ["windows-x86_64"];
  return [];
}

/** dist/ name -> asset name in the old private repo, for the one-time `bridge`. */
export const LEGACY_ASSETS = ["NiceVault-linux-x86_64", "NiceVault-windows-x86_64.exe", "NiceVault-macos-universal.zip"];

/** `pack` = { salt: base64 } when the assets are release packs (`published` = `<name>.nvpack`). */
export function buildLatestJson({ version, notes, pubDate, assets, assetBase, pack = null }) {
  const platforms = {};
  for (const { filename, signature, published } of assets) {
    for (const key of platformKeysFor(filename)) {
      platforms[key] = { signature, url: `${assetBase(version)}/${published ?? filename}` };
    }
  }
  const m = { version, notes: notes.en, notes_i18n: { it: notes.it, en: notes.en }, pub_date: pubDate, platforms };
  if (pack) m.pack = { v: 1, salt: pack.salt };
  return m;
}

/** The gh invocation that publishes (argv, no shell). */
export function ghReleaseArgs({ repo, version, files, notesFile }) {
  return [
    "release", "create", `v${version}`, ...files,
    "--repo", repo, "--title", `NiceVault ${version}`, "--notes-file", notesFile, "--latest",
  ];
}

// ---------------------------------------------------------------------------------------------
// release packs (mirror of app/src-tauri/src/release_pack.rs; cross-checked by a shared vector)

export const PACK_MAGIC = Buffer.from("NVPACK\x00\x01", "latin1");
export const PACK_INFO = "NiceVault release pack v1";
export const PACK_EXT = ".nvpack";
const NONCE_LEN = 12;
const TAG_LEN = 16;

/** release-pack.key text (64 hex chars) -> 32 bytes. */
export function parsePackSecret(text) {
  const hex = text.trim();
  if (!/^[0-9a-f]{64}$/i.test(hex)) throw new Error("release-pack.key must be 64 hex characters");
  return Buffer.from(hex, "hex");
}

export function packKey(secret, salt) {
  return Buffer.from(hkdfSync("sha256", secret, salt, PACK_INFO, 32));
}

export function seal(key, plain, nonce = randomBytes(NONCE_LEN)) {
  const header = Buffer.concat([PACK_MAGIC, nonce]);
  const c = createCipheriv("chacha20-poly1305", key, nonce, { authTagLength: TAG_LEN });
  c.setAAD(header, { plaintextLength: plain.length });
  const body = Buffer.concat([c.update(plain), c.final()]);
  return Buffer.concat([header, body, c.getAuthTag()]);
}

/** The plaintext, or throws (not a pack, wrong key, tampered). */
export function openPack(key, packed) {
  const h = PACK_MAGIC.length + NONCE_LEN;
  if (packed.length < h + TAG_LEN || !packed.subarray(0, PACK_MAGIC.length).equals(PACK_MAGIC)) {
    throw new Error("not a NiceVault release pack");
  }
  const nonce = packed.subarray(PACK_MAGIC.length, h);
  const d = createDecipheriv("chacha20-poly1305", key, nonce, { authTagLength: TAG_LEN });
  d.setAAD(packed.subarray(0, h), { plaintextLength: packed.length - h - TAG_LEN });
  d.setAuthTag(packed.subarray(packed.length - TAG_LEN));
  return Buffer.concat([d.update(packed.subarray(h, packed.length - TAG_LEN)), d.final()]);
}

/** README.txt uploaded with the bridge release (the old private repo's only explanation). */
export function releaseReadme(version) {
  return [
    `NiceVault ${version}`,
    "",
    "IT  Questi file non si usano scaricandoli da qui: sono pacchetti cifrati che solo l'app",
    "    NiceVault sa aprire. Per aggiornare usa \"Cerca aggiornamenti\" nell'app; per mettere",
    "    NiceVault su una chiavetta usa \"Crea dispositivo\" (scarica Windows, macOS e Linux).",
    "",
    "EN  These files are not meant to be downloaded from here: they are encrypted packages only",
    "    the NiceVault app can open. To update, use \"Check for updates\" in the app; to put",
    "    NiceVault on a USB stick, use \"Create device\" (it fetches Windows, macOS and Linux).",
    "",
    "latest.json: what the app reads. Every program is signed (minisign) and checked by the app.",
    "",
  ].join("\n");
}

// ---------------------------------------------------------------------------------------------
// side effects

function git(args) {
  return execFileSync("git", args, { cwd: ROOT }).toString();
}

function commitsSinceLastTag() {
  let range;
  try {
    const tag = execFileSync("git", ["describe", "--tags", "--abbrev=0", "--match", "v*"], {
      cwd: ROOT,
      stdio: ["ignore", "pipe", "ignore"],
    })
      .toString()
      .trim();
    range = `${tag}..HEAD`;
  } catch {
    range = "HEAD";
  }
  const raw = git(["log", range, "--no-merges", "--format=%H%x1f%s%x1f%B%x1e"]);
  return raw
    .split("\x1e")
    .map((r) => r.trim())
    .filter((r) => r.length > 0)
    .map((r) => {
      const [hash, subject, message] = r.split("\x1f");
      return { hash, subject, message };
    });
}

function parseArgs(argv) {
  const args = { cmd: "cut", pre: null, version: null, dryRun: false, skipBuild: false };
  let i = 0;
  if (argv[0] === "publish" || argv[0] === "bridge") {
    args.cmd = argv[0];
    i = 1;
  }
  for (; i < argv.length; i++) {
    if (argv[i] === "--pre") args.pre = argv[++i];
    else if (argv[i] === "--version") args.version = argv[++i];
    else if (argv[i] === "--dry-run") args.dryRun = true;
    else if (argv[i] === "--skip-build") args.skipBuild = true;
    else throw new Error(`unknown argument: ${argv[i]}`);
  }
  if (args.pre !== null && args.pre !== "beta" && args.pre !== "rc") {
    throw new Error(`--pre takes beta or rc, not "${args.pre}"`);
  }
  if (args.version !== null) splitPre(args.version);
  return args;
}

const notesPath = (version) => new URL(`release-notes/v${version}.json`, root);

function updaterConfig() {
  const conf = JSON.parse(readFileSync(new URL("app/src-tauri/tauri.conf.json", root), "utf8"));
  return conf.plugins.updater;
}

function cut(args) {
  const current = currentVersion();
  const commits = commitsSinceLastTag();
  const malformed = [];
  const groups = { major: [], minor: [], patch: [], none: [] };
  for (const c of commits) {
    try {
      groups[changeFromMessage(c.message)].push(c.subject);
    } catch {
      malformed.push(c);
    }
  }
  if (malformed.length > 0) {
    console.error("malformed Change: trailers, fix these before releasing:");
    for (const c of malformed) console.error(`  ${c.hash.slice(0, 9)} ${c.subject}`);
    process.exit(1);
  }
  const level = highestChange(commits.map((c) => c.message));
  const next = args.version ?? computeNextVersion(current, level, args.pre);
  if (next === null) {
    console.log(`every commit since the last tag is 'none': no release (still ${current})`);
    return;
  }
  const date = new Date().toISOString().slice(0, 10);
  const section = buildChangelogSection(next, date, groups);

  if (args.dryRun) {
    console.log(`${current} -> ${next}  (highest change: ${level}${args.pre ? `, --pre ${args.pre}` : ""})`);
    console.log("\n--- CHANGELOG.md section ---\n");
    console.log(section);
    console.log(`would write ${relativePaths().join(", ")}, CHANGELOG.md, Cargo.lock, commit and tag v${next}`);
    return;
  }

  writeVersion(next);
  // refresh the workspace versions in Cargo.lock (no network)
  execFileSync("cargo", ["metadata", "--format-version", "1", "--offline", "--no-deps"], { cwd: ROOT, stdio: "ignore" });

  const changelogPath = new URL("CHANGELOG.md", root);
  const header = "# Changelog\n\nEvery release is cut by `node scripts/release.mjs` from the commit trailers.\n\n";
  const existing = existsSync(changelogPath) ? readFileSync(changelogPath, "utf8") : header;
  const body = existing.startsWith("# Changelog") ? existing.slice(header.length) : existing;
  writeFileSync(changelogPath, header + section + body);

  const notesFile = notesPath(next);
  if (!existsSync(notesFile)) {
    mkdirSync(new URL("release-notes/", root), { recursive: true });
    writeFileSync(notesFile, `${JSON.stringify(buildNotesTemplate(groups), null, 2)}\n`);
  }

  git(["add", ...relativePaths(), "Cargo.lock", "CHANGELOG.md", `release-notes/v${next}.json`]);
  git(["commit", "-m", `release ${next}`, "-m", "Cut by scripts/release.mjs from the commit trailers.", "-m", "Change: none"]);
  git(["tag", `v${next}`]);

  console.log(`released ${current} -> ${next}, tagged v${next} (not pushed)`);
  console.log(`1. write the user notes: release-notes/v${next}.json (it + en), amend the release commit`);
  console.log("2. node scripts/release.mjs publish --dry-run   (build, sign, latest.json)");
  console.log("3. node scripts/release.mjs publish             (upload)");
}

function sign(file, version) {
  const name = file.slice(file.lastIndexOf("/") + 1);
  const platforms = signedPlatformsFor(name).join(",");
  execFileSync("cargo", ["run", "-q", "-p", "nicevault-app", "--example", "sign_update", "--", SECRET_KEY, file, version, platforms], {
    cwd: ROOT,
    stdio: ["ignore", "ignore", "inherit"],
    env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD ?? "" },
  });
  return readFileSync(`${file}.sig`, "utf8").trim();
}

/** The text of the release page (English, the repo's language): the notes, then what each download is. */
export function releaseBody(notes) {
  return [
    notes.en,
    "",
    "| Download | |",
    "|---|---|",
    "| `NiceVault-linux-x86_64.AppImage` | Linux |",
    "| `NiceVault-linux-x86_64.tar.gz` | Linux, bare binary (needs WebKitGTK 4.1) |",
    "| `NiceVault-windows-x86_64.zip` | Windows |",
    "| `NiceVault-macos-universal.zip` | macOS (Apple silicon and Intel) |",
    "",
    "Every file has a minisign `.sig` and a line in `SHA256SUMS`.",
    "",
  ].join("\n");
}

function releaseContext(args) {
  const version = currentVersion();
  const skew = allVersions().filter(([, v]) => v !== version);
  if (skew.length > 0) throw new Error(`version files out of sync: ${skew.map(([p, v]) => `${p}=${v}`).join(", ")}`);
  const cfg = updaterConfig();
  const notesFile = notesPath(version);
  if (!existsSync(notesFile)) throw new Error(`no notes at release-notes/v${version}.json (run the cut first)`);
  const notes = readyNotes(JSON.parse(readFileSync(notesFile, "utf8")));
  const dist = process.env.NV_DIST_DIR ?? `${ROOT}dist`;
  return { version, cfg, notes, dist, args };
}

function ghCreate(repo, version, files, bodyFile, dryRun) {
  const gh = ghReleaseArgs({ repo, version, files, notesFile: bodyFile });
  if (dryRun) {
    console.log("dry run: not uploaded. Would run:");
    console.log(`  gh ${gh.map((a) => (/\s/.test(a) ? JSON.stringify(a) : a)).join(" ")}`);
    return false;
  }
  try {
    git(["rev-parse", "-q", "--verify", `refs/tags/v${version}`]);
  } catch {
    throw new Error(`tag v${version} does not exist here: cut the release first`);
  }
  // uses the owner's own `gh auth login`; nothing is read or printed here
  execFileSync("gh", gh, { cwd: ROOT, stdio: "inherit" });
  console.log(`published v${version} on ${repo}`);
  console.log(`check: gh release view v${version} --repo ${repo}`);
  return true;
}

function publish(args) {
  const { version, cfg, notes, dist } = releaseContext(args);
  const target = releaseTarget(cfg.endpoints[0]);
  if (!existsSync(SECRET_KEY)) throw new Error(`no signing key at ${SECRET_KEY}`);
  const pub = readFileSync(`${SECRET_KEY}.pub`, "utf8").trim();
  if (pub !== cfg.pubkey) {
    throw new Error("tauri.conf.json plugins.updater.pubkey is not the public half of updater-secret.key");
  }
  if (!args.skipBuild) {
    execFileSync("bash", ["scripts/build-all.sh"], { cwd: ROOT, stdio: "inherit" });
  }
  const present = existsSync(dist) ? readdirSync(dist) : [];
  const missing = ARTEFACTS.filter((f) => !present.includes(f));
  if (missing.length > 0) throw new Error(`missing in ${dist}: ${missing.join(", ")}`);

  // public, plain files: sign each one, list them, point latest.json at them
  const out = `${dist}/release`;
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  const sums = [];
  const assets = ARTEFACTS.map((filename) => {
    const signature = sign(`${dist}/${filename}`, version);
    copyFileSync(`${dist}/${filename}`, `${out}/${filename}`, constants.COPYFILE_FICLONE);
    writeFileSync(`${out}/${filename}.sig`, `${signature}\n`);
    sums.push(`${createHash("sha256").update(readFileSync(`${out}/${filename}`)).digest("hex")}  ${filename}`);
    return { filename, signature };
  });
  writeFileSync(`${out}/SHA256SUMS`, `${sums.join("\n")}\n`);
  const manifest = buildLatestJson({ version, notes, pubDate: new Date().toISOString(), assets, assetBase: target.assetBase });
  writeFileSync(`${out}/latest.json`, `${JSON.stringify(manifest, null, 2)}\n`);
  const bodyFile = `${dist}/release-notes.md`;
  writeFileSync(bodyFile, releaseBody(notes));
  console.log(`dist/release/latest.json: ${Object.keys(manifest.platforms).join(", ")} for ${version}`);

  const files = [...ARTEFACTS.flatMap((f) => [`${out}/${f}`, `${out}/${f}.sig`]), `${out}/SHA256SUMS`, `${out}/latest.json`];
  console.log("files to upload:");
  for (const f of files) console.log(`  ${f}  (${statSync(f).size} bytes)`);
  if (target.kind !== "github") {
    console.log(`static host: upload these to ${target.assetBase(version)}/ and latest.json to ${cfg.endpoints[0]}`);
    return;
  }
  if (ghCreate(target.repo, version, files, bodyFile, args.dryRun)) {
    console.log("next, once for the 0.2.x apps: node scripts/release.mjs bridge");
  }
}

/** One-time: the same version in the old private repo, in the old encrypted layout, for the 0.2.x apps. */
function bridge(args) {
  const { version, notes, dist } = releaseContext(args);
  const missing = LEGACY_ASSETS.flatMap((f) => [f, `${f}.sig`]).filter((f) => !existsSync(`${dist}/${f}`));
  if (missing.length > 0) throw new Error(`missing in ${dist}: ${missing.join(", ")} (run \`publish\` first: it signs the builds)`);
  if (!existsSync(PACK_SECRET_FILE)) throw new Error("scripts/legacy/release-pack.key is missing");
  const out = `${dist}/release-legacy`;
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  const salt = randomBytes(32);
  const key = packKey(parsePackSecret(readFileSync(PACK_SECRET_FILE, "utf8")), salt);
  const assets = LEGACY_ASSETS.map((filename) => {
    const plain = readFileSync(`${dist}/${filename}`);
    const packed = seal(key, plain);
    if (!openPack(key, packed).equals(plain)) throw new Error(`${filename}: the pack does not open back`);
    const published = `${filename}${PACK_EXT}`;
    writeFileSync(`${out}/${published}`, packed);
    return { filename, signature: readFileSync(`${dist}/${filename}.sig`, "utf8").trim(), published };
  });
  const base = (v) => `https://github.com/${LEGACY_REPO}/releases/download/v${v}`;
  const manifest = buildLatestJson({ version, notes, pubDate: new Date().toISOString(), assets, assetBase: base, pack: { salt: salt.toString("base64") } });
  writeFileSync(`${out}/latest.json`, `${JSON.stringify(manifest, null, 2)}\n`);
  writeFileSync(`${out}/README.txt`, releaseReadme(version));
  const bodyFile = `${dist}/release-notes-legacy.md`;
  writeFileSync(bodyFile, `${notes.it}\n\n---\n\n${notes.en}\n\n(Bridge release: from here on NiceVault updates from its public releases page.)\n`);
  const files = [...assets.map((a) => `${out}/${a.published}`), `${out}/latest.json`, `${out}/README.txt`];
  console.log(`bridge for ${version} -> ${LEGACY_REPO}:`);
  for (const f of files) console.log(`  ${f}  (${statSync(f).size} bytes)`);
  ghCreate(LEGACY_REPO, version, files, bodyFile, args.dryRun);
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.cmd === "publish") publish(args);
  else if (args.cmd === "bridge") bridge(args);
  else cut(args);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    main();
  } catch (e) {
    console.error(`release: ${e.message}`);
    process.exit(1);
  }
}
