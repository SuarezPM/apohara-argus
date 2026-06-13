#!/usr/bin/env node
// CLI entrypoint for @apohara/argus.
//
// Thin proxy: resolves the cached argus binary for the host
// platform/architecture and execs it with the user-supplied args.
//
// The binary is downloaded on `npm install` (see scripts/postinstall.js)
// and cached under ~/.cache/apohara-argus/. If the cache is missing
// (e.g. offline install, or the user wiped the cache), this script
// fails loudly with a clear remediation message rather than spawning
// a stale or missing file.

"use strict";

const { spawn } = require("node:child_process");
const { getBinaryPath } = require("../scripts/install.js");

const args = process.argv.slice(2);
const env = process.env;

let binaryPath;
try {
  binaryPath = getBinaryPath("argus", env.ARGUS_VERSION || "latest");
} catch (err) {
  process.stderr.write(
    `apohara-argus: ${err.message}\n` +
      `Re-run \`npm install @apohara/argus\` with network access to fetch the binary.\n`
  );
  process.exit(1);
}

const child = spawn(binaryPath, args, {
  stdio: "inherit",
  env: { ...env, ARGUS_NPM_WRAPPER: "1" },
});

child.on("exit", (code) => process.exit(code || 0));
child.on("error", (err) => {
  process.stderr.write(
    `apohara-argus: failed to spawn ${binaryPath}: ${err.message}\n`
  );
  process.exit(1);
});
