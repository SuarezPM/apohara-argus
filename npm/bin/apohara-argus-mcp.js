#!/usr/bin/env node
// MCP server entrypoint for @apohara/argus.
//
// Same pattern as bin/apohara-argus.js but spawns the MCP server
// binary. The MCP binary is a separate Rust artifact (the
// `apohara-argus-mcp` crate's `[[bin]] name = "argus-mcp"`).
//
// CRITICAL (mirrors codesearch-mcp): the MCP server speaks JSON-RPC
// over stdin/stdout and the framing must flow byte-for-byte. We use
// `stdio: "inherit"` so the child's stdio ARE this process's. This
// file must NEVER write to stdout — diagnostics go to stderr only.

"use strict";

const { spawn } = require("node:child_process");
const { getBinaryPath } = require("../scripts/install.js");

const args = process.argv.slice(2);
const env = process.env;

let binaryPath;
try {
  binaryPath = getBinaryPath("argus-mcp", env.ARGUS_VERSION || "latest");
} catch (err) {
  process.stderr.write(
    `apohara-argus-mcp: ${err.message}\n` +
      `Re-run \`npm install @apohara/argus\` with network access to fetch the binary.\n` +
      `If the binary is genuinely missing from the GitHub Release, the MCP server is ` +
      `not yet shipped for this platform — open an issue at ` +
      `https://github.com/SuarezPM/apohara-argus/issues.\n`
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
    `apohara-argus-mcp: failed to spawn ${binaryPath}: ${err.message}\n`
  );
  process.exit(1);
});
