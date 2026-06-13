// Postinstall hook for @apohara/argus.
//
// Tries to download both the CLI binary (`argus`) and the MCP server
// binary (`argus-mcp`) for the host platform. Prints a friendly
// summary on success and a clear warning on failure.
//
// CRITICAL: this script must NEVER exit non-zero on a network or
// version-resolution failure. The user might be installing offline,
// the GitHub Release might not be published yet, or the MCP server
// binary might genuinely not exist for this version (release.yml
// only ships `argus` today). All of those are warnings, not errors —
// the `bin/apohara-argus.js` proxy will surface a clear error at
// runtime if the user tries to invoke a binary we couldn't fetch.

"use strict";

const fs = require("node:fs");
const {
  NPM_VERSION,
  resolvePlatform,
  downloadBinary,
  cachePath,
} = require("./install.js");

const BINARIES = [
  { name: "argus", label: "apohara-argus" },
  { name: "argus-mcp", label: "apohara-argus-mcp" },
];

async function main() {
  const { target, exe, key } = resolvePlatform();
  const version = process.env.ARGUS_VERSION || "latest";
  const pinned = version === "latest" ? `v${NPM_VERSION}` : version;
  const results = [];

  for (const bin of BINARIES) {
    const expected = cachePath(pinned, bin.name, target, exe);
    try {
      const actual = await downloadBinary(bin.name, version);
      if (actual !== expected) {
        // downloadBinary returns the resolved cache path; if the
        // caller pinned a different version via ARGUS_VERSION, the
        // paths can diverge. Reflect what we actually wrote.
        results.push({ label: bin.label, path: actual, ok: true });
      } else {
        results.push({ label: bin.label, path: actual, ok: true });
      }
    } catch (err) {
      results.push({ label: bin.label, path: expected, ok: false, err });
    }
  }

  // Friendly summary on stdout (NOT stderr — npm surfaces stderr to
  // the user, and we want the success message to be visible without
  // the yellow warning chrome).
  process.stdout.write(`@apohara/argus v${NPM_VERSION} installed\n`);
  for (const r of results) {
    if (r.ok) {
      process.stdout.write(`- ${r.label} binary: ${r.path}\n`);
    } else {
      process.stdout.write(`- ${r.label} binary: not downloaded (${r.err.message})\n`);
    }
  }
  process.stdout.write(
    `\nRun \`npx @apohara/argus --help\` to get started.\n` +
      `If a binary failed to download, re-run with network access ` +
      `or build from source: cargo install --path crates/apohara-argus-cli\n`
  );

  // Don't fail the install. The bin proxy will surface the error
  // at runtime if the user actually tries to invoke a missing binary.
}

main().catch((err) => {
  // The postinstall itself must not crash npm install. Print a
  // warning to stderr and exit 0.
  process.stderr.write(`@apohara/argus postinstall warning: ${err.message}\n`);
  process.exit(0);
});
