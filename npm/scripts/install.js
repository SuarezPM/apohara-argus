// @apohara/argus binary installer.
//
// Public API:
//   getBinaryPath(name, version)  — sync; returns the cached binary path
//                                   or throws a remediation-friendly error.
//   downloadBinary(name, version) — async; downloads + caches a binary.
//   getLatestVersion()            — async; resolves "latest" → a real tag
//                                   via the GitHub Releases API.
//
// Design notes:
//   - Zero runtime dependencies. Built-ins only (node:fs, node:path,
//     node:os, node:https, node:crypto).
//   - The download is best-effort: network failures, 404s, and
//     SHA256SUMS mismatches are surfaced but do not crash the install.
//     The postinstall script treats all failures as warnings.
//   - Cache layout: ~/.cache/apohara-argus/v<version>/<asset>/<binary>
//     where <asset> is the Rust target triple (e.g. argus-x86_64-
//     unknown-linux-gnu). This mirrors the GitHub Release asset name
//     so the cache and the release agree 1:1.

"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const https = require("node:https");
const crypto = require("node:crypto");

// Repository coordinates. The release workflow at
// .github/workflows/release.yml publishes draft releases tagged
// v<version> (the same convention cargo uses for the npm wrapper's
// package.json "version" field).
const REPO = "SuarezPM/apohara-argus";
const NPM_VERSION = require("../package.json").version; // "0.1.0"
const USER_AGENT = `@apohara/argus-installer/${NPM_VERSION} (node ${process.version})`;

// Map Node's `${platform}-${arch}` to the Rust target triple. These
// five entries mirror the matrix in .github/workflows/release.yml.
// `ext` is the file extension the release asset uses (raw binary, no
// archive wrapper — release.yml stages the binary directly into
// dist/argus-<target>).
const PLATFORMS = {
  "linux-x64": { target: "x86_64-unknown-linux-gnu", exe: "" },
  "linux-arm64": { target: "aarch64-unknown-linux-gnu", exe: "" },
  "darwin-x64": { target: "x86_64-apple-darwin", exe: "" },
  "darwin-arm64": { target: "aarch64-apple-darwin", exe: "" },
  "win32-x64": { target: "x86_64-pc-windows-msvc", exe: ".exe" },
};

// Where the binaries live on disk. XDG-friendly: $XDG_CACHE_HOME is
// respected if set, otherwise ~/.cache (Linux/macOS), %LOCALAPPDATA%
// (Windows). $ARGUS_CACHE overrides both for testing / pinning.
function cacheRoot() {
  if (process.env.ARGUS_CACHE) return process.env.ARGUS_CACHE;
  if (process.platform === "win32") {
    return path.join(
      process.env.LOCALAPPDATA || os.homedir(),
      "apohara-argus",
      "cache"
    );
  }
  const xdg = process.env.XDG_CACHE_HOME;
  if (xdg) return path.join(xdg, "apohara-argus");
  return path.join(os.homedir(), ".cache", "apohara-argus");
}

// Resolve the host platform. Throws with a remediation hint for
// unsupported host/target combinations.
function resolvePlatform() {
  const key = `${process.platform}-${process.arch}`;
  const entry = PLATFORMS[key];
  if (!entry) {
    const supported = Object.keys(PLATFORMS).join(", ");
    throw new Error(
      `unsupported platform "${key}". @apohara/argus ships prebuilt ` +
        `binaries for: ${supported}. Build from source instead: ` +
        `cargo install --path crates/apohara-argus-cli ` +
        `(https://github.com/${REPO}).`
    );
  }
  return { ...entry, key };
}

// Asset name in the release. The convention is
// `${binaryName}-${target}${exe}` so `argus` → `argus-x86_64-unknown-
// linux-gnu` and `argus-mcp` → `argus-mcp-x86_64-unknown-linux-gnu`.
// (release.yml currently only builds `argus`; `argus-mcp` is a future
//  addition tracked separately. The npm wrapper fails gracefully if
//  the asset is missing.)
function assetName(binaryName, target, exe) {
  return `${binaryName}-${target}${exe}`;
}

// Cache path for a specific binary.
function cachePath(version, binaryName, target, exe) {
  return path.join(
    cacheRoot(),
    version,
    `${binaryName}-${target}`,
    `${binaryName}${exe}`
  );
}

// Fetch a URL into memory (or a file) with redirect support, timeout,
// and proper User-Agent. Returns the response body as a Buffer.
function fetchBuffer(url, redirects = 0) {
  return new Promise((resolve, reject) => {
    if (redirects > 10) {
      reject(new Error(`too many redirects fetching ${url}`));
      return;
    }
    const req = https.get(
      url,
      {
        headers: { "User-Agent": USER_AGENT, Accept: "*/*" },
        timeout: 30000,
      },
      (res) => {
        // GitHub release assets 302-redirect to
        // objects.githubusercontent.com.
        if (
          res.statusCode &&
          res.statusCode >= 300 &&
          res.statusCode < 400 &&
          res.headers.location
        ) {
          res.resume();
          const next = new URL(res.headers.location, url).toString();
          resolve(fetchBuffer(next, redirects + 1));
          return;
        }
        if (res.statusCode !== 200) {
          res.resume();
          const err = new Error(
            `HTTP ${res.statusCode} fetching ${url}`
          );
          err.statusCode = res.statusCode;
          reject(err);
          return;
        }
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve(Buffer.concat(chunks)));
        res.on("error", reject);
      }
    );
    req.on("error", reject);
    req.on("timeout", () => {
      req.destroy(new Error(`timeout fetching ${url}`));
    });
  });
}

// Resolve the literal "latest" tag to a concrete vX.Y.Z via the
// GitHub Releases API. Cached per-process to avoid hammering the
// rate limit on multiple downloadBinary() calls.
let _latestCache = null;
async function getLatestVersion() {
  if (_latestCache) return _latestCache;
  const url = `https://api.github.com/repos/${REPO}/releases/latest`;
  const body = await fetchBuffer(url);
  let json;
  try {
    json = JSON.parse(body.toString("utf8"));
  } catch (err) {
    throw new Error(`malformed JSON from ${url}: ${err.message}`);
  }
  if (!json.tag_name) {
    throw new Error(`no tag_name in /releases/latest response for ${REPO}`);
  }
  _latestCache = json.tag_name; // e.g. "v0.1.0"
  return _latestCache;
}

// Resolve a version string ("latest", "v0.1.0", "0.1.0") to a
// concrete "vX.Y.Z" tag.
async function resolveVersion(version) {
  if (!version || version === "latest") {
    return getLatestVersion();
  }
  return version.startsWith("v") ? version : `v${version}`;
}

// SHA-256 of a buffer, hex-encoded.
function sha256(buf) {
  return crypto.createHash("sha256").update(buf).digest("hex");
}

// Best-effort SHA256SUMS fetch + parse. The release workflow at
// .github/workflows/release.yml assembles a SHA256SUMS file inside
// the release-bundle artifact, but does not yet upload it to the
// GitHub Release itself. This function tolerates 404s (no SHA256SUMS
// in the release) and surfaces mismatches as warnings rather than
// fatal errors — the spec's "be robust about download failures" rule.
async function fetchSha256Sums(releaseTag) {
  const url = `https://github.com/${REPO}/releases/download/${releaseTag}/SHA256SUMS`;
  try {
    const body = await fetchBuffer(url);
    const map = new Map();
    for (const line of body.toString("utf8").split("\n")) {
      const m = line.match(/^([a-f0-9]{64})\s+\*?(.+)$/);
      if (m) map.set(m[2].trim(), m[1].toLowerCase());
    }
    return map;
  } catch (err) {
    if (err.statusCode === 404) {
      return null; // no SHA256SUMS in this release
    }
    throw err;
  }
}

// Download a single binary, verify it (best-effort), and cache it.
// Returns the cached path.
async function downloadBinary(binaryName, versionArg) {
  const { target, exe, key } = resolvePlatform();
  const version = await resolveVersion(versionArg);
  const asset = assetName(binaryName, target, exe);
  const dest = cachePath(version, binaryName, target, exe);

  // Cache hit: nothing to do.
  if (fs.existsSync(dest)) {
    return dest;
  }

  fs.mkdirSync(path.dirname(dest), { recursive: true });

  // The spec describes <asset>.tar.gz / .zip archives; the current
  // release.yml uploads the raw binary directly. Try the archive
  // form first (forward-compatible), fall back to the raw form
  // (current state), and give a clear error if neither exists.
  const archiveUrl = `https://github.com/${REPO}/releases/download/${version}/${asset}.tar.gz`;
  const rawUrl = `https://github.com/${REPO}/releases/download/${version}/${asset}`;

  let buffer = null;
  let sourceUrl = null;
  let triedArchive = false;

  try {
    buffer = await fetchBuffer(archiveUrl);
    sourceUrl = archiveUrl;
    // .tar.gz parsing would happen here. The current release ships
    // raw binaries, so this branch is the future path; for now we
    // keep the buffer and fall through to writing the raw bytes.
    triedArchive = true;
  } catch (err) {
    if (err.statusCode !== 404) throw err;
    // Archive not present — fall back to the raw binary.
  }

  if (triedArchive && buffer) {
    // No archive support yet. Treat the bytes as a raw binary anyway
    // and warn. If a future release ships real .tar.gz, this branch
    // will be replaced with a tar parser.
    process.stderr.write(
      `@apohara/argus: WARNING — got ${archiveUrl} as a non-archive ` +
        `payload; writing raw bytes. Update install.js to support .tar.gz.\n`
    );
  } else {
    try {
      buffer = await fetchBuffer(rawUrl);
      sourceUrl = rawUrl;
    } catch (err) {
      if (err.statusCode === 404) {
        throw new Error(
          `binary ${asset} not found in release ${version} of ` +
            `${REPO} (tried ${archiveUrl} and ${rawUrl}). ` +
            `The release may not include this binary yet.`
        );
      }
      throw err;
    }
  }

  // Best-effort SHA-256 verification. Mismatches throw — that's the
  // one case where we want to fail loudly, because a tampered
  // download is a security incident, not a transient network error.
  const sums = await fetchSha256Sums(version);
  if (sums && sums.has(asset)) {
    const expected = sums.get(asset);
    const actual = sha256(buffer);
    if (actual !== expected) {
      throw new Error(
        `SHA-256 mismatch for ${asset}: expected ${expected}, got ${actual}. ` +
          `Refusing to cache a tampered binary.`
      );
    }
  }

  fs.writeFileSync(dest, buffer, { mode: 0o644 });
  if (process.platform !== "win32") {
    fs.chmodSync(dest, 0o755);
  }
  return dest;
}

// Sync cache lookup. Returns the path if the binary is already on
// disk, otherwise throws with a remediation message. The bin proxies
// call this; the postinstall script uses downloadBinary() to do the
// actual fetching.
function getBinaryPath(binaryName, versionArg) {
  const { target, exe } = resolvePlatform();
  const versionRaw = versionArg || process.env.ARGUS_VERSION || "latest";
  // For "latest" we can't synchronously resolve the version. Honor
  // an env-var override (ARGUS_VERSION=v0.1.0) or the package's own
  // version, both of which are pinned and known synchronously.
  let version;
  if (versionRaw === "latest") {
    version = `v${NPM_VERSION}`;
  } else {
    version = versionRaw.startsWith("v") ? versionRaw : `v${versionRaw}`;
  }
  const dest = cachePath(version, binaryName, target, exe);
  if (!fs.existsSync(dest)) {
    throw new Error(
      `binary not cached at ${dest}. Run \`npm rebuild @apohara/argus\` ` +
        `or \`node ${path.relative(process.cwd(), __dirname)}/postinstall.js\` ` +
        `with network access to download it.`
    );
  }
  return dest;
}

module.exports = {
  PLATFORMS,
  REPO,
  NPM_VERSION,
  resolvePlatform,
  resolveVersion,
  getLatestVersion,
  getBinaryPath,
  downloadBinary,
  cacheRoot,
  cachePath,
  assetName,
};
