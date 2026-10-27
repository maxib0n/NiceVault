import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { existsSync, readFileSync } from "node:fs";
import {
  ARTEFACTS,
  LEGACY_ASSETS,
  LEGACY_REPO,
  SITE_ASSETS,
  UPDATER_ASSETS,
  releaseBody,
  signedPlatformsFor,
  PACK_EXT,
  PACK_MAGIC,
  openPack,
  packKey,
  parsePackSecret,
  releaseReadme,
  seal,
  buildChangelogSection,
  buildLatestJson,
  buildNotesTemplate,
  changeFromMessage,
  computeNextVersion,
  ghReleaseArgs,
  highestChange,
  platformKeysFor,
  readyNotes,
  releaseTarget,
} from "./release.mjs";

describe("changeFromMessage", () => {
  it("defaults a missing trailer to patch", () => assert.equal(changeFromMessage("a fix"), "patch"));
  it("reads each level, case-insensitively", () => {
    assert.equal(changeFromMessage("x\n\nChange: none"), "none");
    assert.equal(changeFromMessage("x\n\nChange: MINOR"), "minor");
    assert.equal(changeFromMessage("x\n\nChange: Major"), "major");
  });
  it("takes the last trailer", () => {
    assert.equal(changeFromMessage("mentions Change: major\n\nChange: patch"), "patch");
  });
  it("throws on a malformed value", () => assert.throws(() => changeFromMessage("x\n\nChange: bugfix"), /malformed/));
});

describe("highestChange", () => {
  it("takes the highest", () => {
    assert.equal(highestChange(["Change: patch", "Change: minor", "Change: none"]), "minor");
    assert.equal(highestChange(["Change: none"]), "none");
    assert.equal(highestChange([]), "none");
    assert.equal(highestChange(["no trailer", "Change: none"]), "patch");
  });
});

describe("computeNextVersion", () => {
  const cases = [
    ["1.2.3", "patch", null, "1.2.4"],
    ["1.2.3", "minor", null, "1.3.0"],
    ["1.2.3", "major", null, "2.0.0"],
    ["1.2.3", "none", null, null],
    ["1.2.3", "minor", "beta", "1.3.0-beta.1"],
    ["1.3.0-beta.1", "patch", "beta", "1.3.0-beta.2"],
    ["1.3.0-beta.2", "minor", "rc", "1.3.0-rc.1"],
    ["1.3.0-rc.1", "none", null, "1.3.0"],
  ];
  for (const [cur, level, ch, want] of cases) {
    it(`${cur} + ${level}${ch ? ` --pre ${ch}` : ""} -> ${want}`, () => assert.equal(computeNextVersion(cur, level, ch), want));
  }
});

describe("changelog and notes", () => {
  it("groups subjects and omits empty groups", () => {
    const s = buildChangelogSection("1.3.0", "2026-10-24", { minor: ["new"], patch: ["fix"] });
    assert.match(s, /## 1\.3\.0 \(2026-10-24\)/);
    assert.match(s, /### Features\n\n- new/);
    assert.match(s, /### Fixes\n\n- fix/);
    assert.doesNotMatch(s, /Breaking|Housekeeping/);
  });
  it("drafts notes from major+minor and refuses the untouched draft", () => {
    const t = buildNotesTemplate({ major: ["big"], minor: ["new"], patch: ["tiny"] });
    assert.match(t.en, /- big\n- new/);
    assert.doesNotMatch(t.en, /tiny/);
    assert.throws(() => readyNotes(t), /draft/);
    assert.throws(() => readyNotes({ it: "x" }), /both/);
    assert.deepEqual(readyNotes({ it: "novità", en: "news" }), { it: "novità", en: "news" });
  });
});

describe("releaseTarget", () => {
  it("reads the GitHub repo from the endpoint", () => {
    const t = releaseTarget("https://github.com/maxib0n/NiceVault/releases/latest/download/latest.json");
    assert.equal(t.kind, "github");
    assert.equal(t.repo, "maxib0n/NiceVault");
    assert.equal(t.assetBase("1.2.3"), "https://github.com/maxib0n/NiceVault/releases/download/v1.2.3");
  });
  it("treats any other https URL as a static host", () => {
    const t = releaseTarget("https://dl.example.org/nicevault/latest.json");
    assert.equal(t.kind, "static");
    assert.equal(t.assetBase("1.2.3"), "https://dl.example.org/nicevault/v1.2.3");
  });
  it("refuses http", () => assert.throws(() => releaseTarget("http://example.org/latest.json"), /https/));
});

describe("release assets", () => {
  it("has exactly the names the website links to", () => {
    assert.deepEqual(SITE_ASSETS, [
      "NiceVault-linux-x86_64.AppImage",
      "NiceVault-linux-x86_64.tar.gz",
      "NiceVault-windows-x86_64.zip",
      "NiceVault-macos-universal.zip",
    ]);
    assert.deepEqual(ARTEFACTS, [...SITE_ASSETS, ...UPDATER_ASSETS]);
  });
  it("binds every asset's signature to a platform, the archives to what they hold", () => {
    for (const f of ARTEFACTS) assert.ok(signedPlatformsFor(f).length > 0, f);
    assert.deepEqual(signedPlatformsFor("NiceVault-linux-x86_64.tar.gz"), ["linux-x86_64"]);
    assert.deepEqual(signedPlatformsFor("NiceVault-windows-x86_64.zip"), ["windows-x86_64"]);
    assert.deepEqual(signedPlatformsFor("NiceVault-linux-x86_64.AppImage"), ["linux-x86_64-appimage"]);
  });
  it("the app's endpoint is the public repo and nothing needs a token", () => {
    const conf = JSON.parse(readFileSync(new URL("../app/src-tauri/tauri.conf.json", import.meta.url), "utf8"));
    const t = releaseTarget(conf.plugins.updater.endpoints[0]);
    assert.equal(t.kind, "github");
    assert.notEqual(t.repo, LEGACY_REPO);
    const body = releaseBody({ it: "solo italiano", en: "e" });
    assert.match(body, /NiceVault-linux-x86_64\.AppImage/);
    assert.doesNotMatch(body, /solo italiano/);
  });
});

describe("buildLatestJson", () => {
  it("maps the artefacts to five platform keys, the universal zip serving both Macs", () => {
    const base = (v) => `https://github.com/o/r/releases/download/v${v}`;
    const m = buildLatestJson({
      version: "1.2.3",
      notes: { it: "novità", en: "news" },
      pubDate: "2026-10-24T00:00:00Z",
      assets: [...ARTEFACTS, "SHA256SUMS"].map((filename) => ({ filename, signature: `SIG:${filename}` })),
      assetBase: base,
    });
    assert.equal(m.version, "1.2.3");
    assert.equal(m.notes, "news");
    assert.deepEqual(m.notes_i18n, { it: "novità", en: "news" });
    assert.equal(m.pub_date, "2026-10-24T00:00:00Z");
    assert.deepEqual(Object.keys(m.platforms).sort(), ["darwin-aarch64", "darwin-x86_64", "linux-x86_64", "linux-x86_64-appimage", "windows-x86_64"]);
    assert.equal(m.platforms["darwin-aarch64"].url, m.platforms["darwin-x86_64"].url);
    assert.equal(m.platforms["windows-x86_64"].url, `${base("1.2.3")}/NiceVault-windows-x86_64.exe`);
    assert.equal(m.platforms["linux-x86_64"].signature, "SIG:NiceVault-linux-x86_64");
    assert.equal(m.platforms["linux-x86_64-appimage"].url, `${base("1.2.3")}/NiceVault-linux-x86_64.AppImage`);
    assert.equal(m.pack, undefined);
  });
  it("knows only the shipped artefacts", () => {
    assert.deepEqual(platformKeysFor("SHA256SUMS"), []);
    assert.deepEqual(platformKeysFor("NiceVault-linux-x86_64.sig"), []);
  });
});

describe("ghReleaseArgs", () => {
  it("creates the tag's release on the releases repo with every file", () => {
    const a = ghReleaseArgs({ repo: "o/r", version: "1.2.3", files: ["a", "latest.json"], notesFile: "n.md" });
    assert.deepEqual(a.slice(0, 5), ["release", "create", "v1.2.3", "a", "latest.json"]);
    assert.equal(a[a.indexOf("--repo") + 1], "o/r");
    assert.equal(a[a.indexOf("--notes-file") + 1], "n.md");
  });
});

const LEGACY_KEY = new URL("./legacy/release-pack.key", import.meta.url);
describe("release packs", { skip: !existsSync(LEGACY_KEY) }, () => {
  const secret = existsSync(LEGACY_KEY) ? parsePackSecret(readFileSync(LEGACY_KEY, "utf8")) : Buffer.alloc(32);
  const key = packKey(secret, Buffer.from("salt of this release"));

  it("matches the vectors the 0.2.x apps were tested with (Node packs, those apps open)", () => {
    assert.equal(packKey(Buffer.alloc(32, 1), Buffer.alloc(32, 2)).toString("hex"), "d5f927426142b3d12e1017e9e96e59240f93ac0f5bb181bc0201968db66de4ef");
    const v = seal(Buffer.alloc(32, 7), Buffer.from("NiceVault pack test vector\n"), Buffer.alloc(12, 9));
    assert.equal(
      v.toString("hex"),
      "4e565041434b0001090909090909090909090909b21098b2b26ff0c9a9487e59a38c84a51b7d3ead4e622eec33de34ee019202cda9e43d926731d0a73936ad",
    );
  });
  it("round-trips, with a fresh nonce each time", () => {
    const plain = Buffer.from(Array.from({ length: 100_000 }, (_, i) => (i * 31) & 0xff));
    const a = seal(key, plain);
    const b = seal(key, plain);
    assert.ok(a.subarray(0, PACK_MAGIC.length).equals(PACK_MAGIC));
    assert.notDeepEqual(a, b);
    assert.ok(openPack(key, a).equals(plain) && openPack(key, b).equals(plain));
    assert.equal(a.length, plain.length + 8 + 12 + 16);
  });
  it("refuses tampering, a wrong key and non-packs", () => {
    const p = seal(key, Buffer.from("build"));
    for (const at of [0, 10, 21, p.length - 1]) {
      const t = Buffer.from(p);
      t[at] ^= 1;
      assert.throws(() => openPack(key, t), undefined, `byte ${at}`);
    }
    assert.throws(() => openPack(packKey(secret, Buffer.from("another release")), p));
    assert.throws(() => openPack(key, Buffer.from("MZ plain exe")), /not a NiceVault release pack/);
    assert.throws(() => parsePackSecret("abc"), /64 hex/);
  });
  it("latest.json points at the packs and carries the salt", () => {
    const m = buildLatestJson({
      version: "1.2.3",
      notes: { it: "n", en: "n" },
      pubDate: "x",
      assets: LEGACY_ASSETS.map((filename) => ({ filename, signature: "S", published: `${filename}${PACK_EXT}` })),
      assetBase: (v) => `https://h/v${v}`,
      pack: { salt: "c2FsdA==" },
    });
    assert.deepEqual(m.pack, { v: 1, salt: "c2FsdA==" });
    assert.equal(m.platforms["darwin-x86_64"].url, "https://h/v1.2.3/NiceVault-macos-universal.zip.nvpack");
    assert.equal(m.platforms["linux-x86_64"].signature, "S");
    const plain = buildLatestJson({ version: "1.2.3", notes: { it: "n", en: "n" }, pubDate: "x", assets: [{ filename: "NiceVault-linux-x86_64", signature: "S" }], assetBase: (v) => `https://h/v${v}` });
    assert.equal(plain.pack, undefined);
    assert.equal(plain.platforms["linux-x86_64"].url, "https://h/v1.2.3/NiceVault-linux-x86_64");
  });
  it("the README says to download through the app, in both languages", () => {
    const r = releaseReadme("1.2.3");
    assert.match(r, /NiceVault 1\.2\.3/);
    assert.match(r, /solo l'app/);
    assert.match(r, /only\s+the NiceVault app/);
  });
});
