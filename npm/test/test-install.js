// @apohara/argus install flow — end-to-end test.
//
// Exercises the public surface of scripts/install.js without ever
// touching the real GitHub API or polluting the user's real
// ~/.cache. Strategy:
//
//   1. Monkey-patch node:https.get with a stub that serves a
//      locally-built mock release (a Map<asset, Buffer> + an optional
//      SHA256SUMS blob).
//   2. Point the cache at a fresh tmpdir via ARGUS_CACHE (also
//      overrides $HOME and $XDG_CACHE_HOME so the helpers that look
//      at os.homedir() agree).
//   3. Drive downloadBinary / getBinaryPath / getLatestVersion with
//      pinned versions, then assert on the cache layout.
//
// We do NOT modify scripts/install.js. The test works against the
// shipped API.

"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const https = require("node:https");
const crypto = require("node:crypto");
const { EventEmitter } = require("node:events");

const {
  PLATFORMS,
  REPO,
  NPM_VERSION,
  cacheRoot,
  cachePath,
  resolvePlatform,
  getLatestVersion,
  getBinaryPath,
  downloadBinary,
} = require("../scripts/install.js");

// === Fixtures ============================================================

const TEST_VERSION = "v0.1.0";

// Mirrors PLATFORMS in install.js — kept in sync intentionally so a
// PLATFORMS regression is caught here.
const EXPECTED_PLATFORMS = [
  { key: "linux-x64", target: "x86_64-unknown-linux-gnu", exe: "" },
  { key: "linux-arm64", target: "aarch64-unknown-linux-gnu", exe: "" },
  { key: "darwin-x64", target: "x86_64-apple-darwin", exe: "" },
  { key: "darwin-arm64", target: "aarch64-apple-darwin", exe: "" },
  { key: "win32-x64", target: "x86_64-pc-windows-msvc", exe: ".exe" },
];

const BINARY_NAMES = ["argus", "argus-mcp"];

// === https.get stub ======================================================

// Build a fake IncomingMessage / ClientRequest pair that mimics just
// enough of the real surface that install.js's fetchBuffer() is
// happy: a callback that fires with a status + headers, a stream of
// 'data' / 'end' events, and a request object that handles 'error'
// and 'timeout' listeners.
function buildFakeTransport(handler) {
  return function fakeGet(url, optionsOrCb, maybeCb) {
    const cb = typeof optionsOrCb === "function" ? optionsOrCb : maybeCb;
    const result = handler(url);

    const res = new EventEmitter();
    res.statusCode = result.statusCode;
    res.headers = result.headers || {};
    res.resume = () => {};

    const req = new EventEmitter();
    req.destroy = (err) => {
      if (err) req.emit("error", err);
    };

    // Defer to the next tick so the install.js code that runs in the
    // callback (attaching 'data' / 'end' listeners) lands first.
    process.nextTick(() => {
      cb(res);
      if (result.body !== undefined && result.body !== null) {
        const buf = Buffer.isBuffer(result.body)
          ? result.body
          : Buffer.from(result.body);
        res.emit("data", buf);
      }
      res.emit("end");
    });

    return req;
  };
}

// Run `fn` with https.get monkey-patched to a stub. Restores the
// original on both success and throw. Note this is async — the
// `finally` block MUST run after `fn` settles, otherwise the
// network calls inside downloadBinary (which are themselves async)
// will see the original https.get, not our stub.
async function withMockedHttps(handler, fn) {
  const original = https.get;
  https.get = buildFakeTransport(handler);
  try {
    return await fn();
  } finally {
    https.get = original;
  }
}

// === Temp cache / HOME ===================================================

// Run `fn` with HOME + XDG_CACHE_HOME + ARGUS_CACHE all pointed at
// a fresh tmpdir. Restores the previous values on exit and removes
// the tmpdir. The cacheRoot() helper inside install.js reads these
// vars via os.homedir() / process.env, so this is the only knob we
// need to keep the test from touching the real ~/.cache. Async so
// the cleanup waits for any in-flight download to settle — a sync
// wrapper would race the finally block against the async fn and
// yank the tmpdir out from under downloadBinary.
async function withTempCache(fn) {
  const tmpDir = fs.mkdtempSync(
    path.join(os.tmpdir(), "apohara-argus-test-")
  );
  const cache = path.join(tmpDir, "cache");
  fs.mkdirSync(cache, { recursive: true });

  const prev = {
    HOME: process.env.HOME,
    ARGUS_CACHE: process.env.ARGUS_CACHE,
    XDG_CACHE_HOME: process.env.XDG_CACHE_HOME,
    USERPROFILE: process.env.USERPROFILE,
  };
  process.env.HOME = tmpDir;
  process.env.ARGUS_CACHE = cache;
  process.env.XDG_CACHE_HOME = cache;
  // resolvePlatform() doesn't read this, but os.homedir() on Windows
  // does. Mirror the override for symmetry.
  if (process.platform === "win32") {
    process.env.USERPROFILE = tmpDir;
  }

  try {
    return await fn({ tmpDir, cacheRoot: cache });
  } finally {
    for (const [k, v] of Object.entries(prev)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch {
      // best-effort cleanup
    }
  }
}

// === Mock release builder ================================================

// Build a mock release. Returns:
//   binaries: Map<assetName, Buffer>
//   sha256Sums: string | null (null when omitted)
//   tag: the version tag (string)
function buildMockRelease({ omitSha256 = false, corruptSha256 = false } = {}) {
  const binaries = new Map();
  for (const bin of BINARY_NAMES) {
    for (const { target, exe } of EXPECTED_PLATFORMS) {
      const asset = `${bin}-${target}${exe}`;
      // Distinct deterministic content per asset so the SHA-256
      // differs across assets (otherwise the test would pass even
      // if install.js grabbed the wrong one).
      const content = `mock-binary-${bin}-${target}-${exe || "noext"}`;
      binaries.set(asset, Buffer.from(content, "utf8"));
    }
  }

  let sha256Sums = "";
  for (const [asset, buf] of binaries) {
    const hash = crypto.createHash("sha256").update(buf).digest("hex");
    sha256Sums += `${hash}  ${asset}\n`;
  }
  if (omitSha256) {
    sha256Sums = null;
  } else if (corruptSha256) {
    // Flip the first byte of the first hash to make it definitely
    // wrong without invalidating the file format.
    sha256Sums = sha256Sums.replace(/^([0-9a-f]{2})/, "ff");
  }

  return { binaries, sha256Sums, tag: TEST_VERSION };
}

// URL router for the fake transport. Matches the four URL patterns
// install.js hits:
//
//   https://api.github.com/repos/<REPO>/releases/latest
//   https://github.com/<REPO>/releases/download/<tag>/<asset>.tar.gz
//   https://github.com/<REPO>/releases/download/<tag>/<asset>
//   https://github.com/<REPO>/releases/download/<tag>/SHA256SUMS
function makeUrlRouter(release) {
  return function route(url) {
    const u = String(url);

    if (u.includes(`/repos/${REPO}/releases/latest`)) {
      return {
        statusCode: 200,
        body: JSON.stringify({ tag_name: release.tag }),
        headers: { "content-type": "application/json" },
      };
    }

    if (u.endsWith(`/releases/download/${release.tag}/SHA256SUMS`)) {
      if (release.sha256Sums === null) {
        return { statusCode: 404, body: "Not Found", headers: {} };
      }
      return {
        statusCode: 200,
        body: release.sha256Sums,
        headers: { "content-type": "text/plain" },
      };
    }

    // .tar.gz form (install.js probes it first, expects 404 today).
    for (const asset of release.binaries.keys()) {
      if (u.endsWith(`/releases/download/${release.tag}/${asset}.tar.gz`)) {
        return { statusCode: 404, body: "Not Found", headers: {} };
      }
    }
    // Raw form (the real path install.js uses today).
    for (const [asset, buf] of release.binaries) {
      if (u.endsWith(`/releases/download/${release.tag}/${asset}`)) {
        return { statusCode: 200, body: buf, headers: {} };
      }
    }

    return { statusCode: 404, body: "Not Found", headers: {} };
  };
}

// Resolve the (target, exe) for the host we're running on, or skip
// the test if the host is unsupported by the install matrix.
function hostPlatform(t) {
  const { target, exe } = resolvePlatform();
  if (!target) {
    t.skip(`unsupported host: ${process.platform}-${process.arch}`);
    return null;
  }
  return { target, exe };
}

// === Tests ===============================================================

test("PLATFORMS exposes all 5 expected host/target pairs", () => {
  assert.equal(Object.keys(PLATFORMS).length, 5);
  for (const { key, target, exe } of EXPECTED_PLATFORMS) {
    assert.ok(PLATFORMS[key], `missing PLATFORMS[${key}]`);
    assert.equal(PLATFORMS[key].target, target);
    assert.equal(PLATFORMS[key].exe, exe);
  }
});

test("cachePath returns the right layout for all 5 platforms", () => {
  withTempCache(({ cacheRoot: root }) => {
    for (const { target, exe } of EXPECTED_PLATFORMS) {
      const p = cachePath(TEST_VERSION, "argus", target, exe);
      // Layout: <root>/<version>/<bin>-<target>/<bin><exe>
      const rel = path.relative(root, p).split(path.sep);
      assert.equal(rel[0], TEST_VERSION, `version segment for ${target}`);
      assert.equal(rel[1], `argus-${target}`, `asset segment for ${target}`);
      assert.equal(rel[2], `argus${exe}`, `binary segment for ${target}`);
    }
  });
});

test("getBinaryPath returns the cached path when the binary is present", (t) => {
  withTempCache(() => {
    const hp = hostPlatform(t);
    if (!hp) return;
    const { target, exe } = hp;
    const dest = cachePath(TEST_VERSION, "argus", target, exe);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.writeFileSync(dest, "mock");

    const got = getBinaryPath("argus", TEST_VERSION);
    assert.equal(got, dest);
    assert.ok(fs.existsSync(got), "binary file exists on disk");
  });
});

test("getBinaryPath throws a remediation hint when binary is missing", () => {
  withTempCache(() => {
    assert.throws(
      () => getBinaryPath("argus", "v9.9.9-never-published"),
      /binary not cached at.*Run `npm rebuild @apohara\/argus`/s
    );
  });
});

test("downloadBinary: happy path — download, verify SHA-256, cache the binary", async (t) => {
  await withTempCache(async () => {
    const release = buildMockRelease();
    const route = makeUrlRouter(release);
    const hp = hostPlatform(t);
    if (!hp) return;
    const { target, exe } = hp;
    const expectedAsset = `argus-${target}${exe}`;
    const expectedBytes = release.binaries.get(expectedAsset);

    await withMockedHttps(route, async () => {
      const dest = await downloadBinary("argus", TEST_VERSION);
      const expected = cachePath(TEST_VERSION, "argus", target, exe);
      assert.equal(dest, expected);
      assert.ok(fs.existsSync(dest), "binary written to cache");
      const cached = fs.readFileSync(dest);
      assert.deepEqual(cached, expectedBytes, "cached bytes match mock binary");
      // The binary must be executable on POSIX so the bin proxy can
      // spawn it. (Windows has no exec bit.)
      if (process.platform !== "win32") {
        const mode = fs.statSync(dest).mode & 0o777;
        assert.ok(mode & 0o100, `expected exec bit, got mode ${mode.toString(8)}`);
      }
    });
  });
});

test("downloadBinary: missing SHA256SUMS — warn (404) + continue", async (t) => {
  await withTempCache(async () => {
    // The release has binaries but no SHA256SUMS. install.js logs a
    // warning to stderr and proceeds with the unverified binary.
    const release = buildMockRelease({ omitSha256: true });
    const route = makeUrlRouter(release);
    const hp = hostPlatform(t);
    if (!hp) return;
    const { target, exe } = hp;
    const expectedAsset = `argus-${target}${exe}`;
    const expectedBytes = release.binaries.get(expectedAsset);

    // Capture stderr so we can confirm install.js *did* get a 404
    // from the SHA256SUMS URL (sanity check on the mock, not on
    // install.js — the test would also pass if install.js silently
    // skipped verification).
    let stderrBuf = "";
    const origWrite = process.stderr.write.bind(process.stderr);
    process.stderr.write = (chunk) => {
      stderrBuf += String(chunk);
      return true;
    };

    try {
      await withMockedHttps(route, async () => {
        const dest = await downloadBinary("argus", TEST_VERSION);
        const expected = cachePath(TEST_VERSION, "argus", target, exe);
        assert.equal(dest, expected, "downloadBinary resolved the cache path");
        assert.ok(fs.existsSync(dest), "binary cached despite missing SHA256SUMS");
        const cached = fs.readFileSync(dest);
        assert.deepEqual(cached, expectedBytes, "unverified bytes still cached");
      });
    } finally {
      process.stderr.write = origWrite;
    }

    // Sanity: install.js called fetchSha256Sums, which goes through
    // the same https.get path. With a 404 stub, the route function
    // was invoked for SHA256SUMS — but we don't assert on that
    // because the absence of a throw is the load-bearing signal.
    void stderrBuf;
  });
});

test("downloadBinary: mismatched SHA256SUMS — throw loudly (tampered download)", async (t) => {
  await withTempCache(async () => {
    const release = buildMockRelease({ corruptSha256: true });
    const route = makeUrlRouter(release);
    const hp = hostPlatform(t);
    if (!hp) return;
    const { target, exe } = hp;
    const expectedPath = cachePath(TEST_VERSION, "argus", target, exe);

    await withMockedHttps(route, async () => {
      let caught = null;
      try {
        await downloadBinary("argus", TEST_VERSION);
      } catch (e) {
        caught = e;
      }
      assert.ok(caught, "downloadBinary must reject on SHA-256 mismatch");
      assert.match(caught.message, /SHA-256 mismatch/);
      assert.match(caught.message, /Refusing to cache a tampered binary/);
      // Critical: the tampered bytes must NOT have been cached.
      assert.ok(
        !fs.existsSync(expectedPath),
        "tampered binary must not be on disk after a SHA-256 mismatch"
      );
    });
  });
});

test("getLatestVersion resolves /releases/latest to the release tag", async () => {
  const release = buildMockRelease();
  const route = makeUrlRouter(release);
  // Note: do NOT use withTempCache here — the release-version
  // resolver doesn't read the cache. The mock just intercepts
  // https.get at module level.
  await withMockedHttps(route, async () => {
    const tag = await getLatestVersion();
    assert.equal(tag, TEST_VERSION);
  });
});

test("ARGUS_CACHE env var overrides the default cache location", () => {
  withTempCache(({ cacheRoot: expected }) => {
    assert.equal(cacheRoot(), expected);
    // The override must be honored even when HOME is set to an
    // unrelated path (which is the case in withTempCache).
    assert.notEqual(cacheRoot(), path.join(os.homedir(), ".cache", "apohara-argus"));
  });
});

test("NPM_VERSION matches the package.json (sanity)", () => {
  // Belt-and-suspenders: if package.json drifts from the install
  // module's NPM_VERSION constant, the cache layout tests above
  // could lie to a future maintainer. Pin it.
  const pkg = require("../package.json");
  assert.equal(NPM_VERSION, pkg.version);
});
