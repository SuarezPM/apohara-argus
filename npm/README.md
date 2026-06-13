# @apohara/argus

> AI slop defense layer for code review. Rust binary, npm wrapper.

`@apohara/argus` is the npm entry point for [ARGUS](https://github.com/SuarezPM/apohara-argus), a hybrid (deterministic regex + LLM semantic) defense layer that catches AI-generated code slop during code review. The package wraps the prebuilt Rust binaries published on the [GitHub Releases page](https://github.com/SuarezPM/apohara-argus/releases) and caches them locally, so `npx @apohara/argus --help` works without a Rust toolchain.

## Quick start

```sh
npx @apohara/argus --help
```

## Install

```sh
npm install @apohara/argus
```

The postinstall hook downloads the matching prebuilt binary for your platform to `~/.cache/apohara-argus/v<version>/<target>/`. Subsequent installs reuse the cache.

The package exposes two binaries:

| Command | Spawns | Purpose |
|---------|--------|---------|
| `apohara-argus` | `argus` | CLI: `argus scan`, `argus verify`, etc. |
| `apohara-argus-mcp` | `argus-mcp` | MCP server (JSON-RPC over stdio) |

## How the binary download works

See [`scripts/install.js`](./scripts/install.js). On `npm install`:

1. The host's `process.platform` + `process.arch` is mapped to one of five Rust target triples.
2. The matching asset is downloaded from the GitHub Release for the npm package's `version` (default: `latest`).
3. If a `SHA256SUMS` file is published with the release, the binary's SHA-256 is verified against it.
4. The binary is cached at `~/.cache/apohara-argus/v<version>/<asset>/<binary>` and made executable.

The `bin/apohara-argus.js` proxy is a 20-line `child_process.spawn` that resolves the cached path and `exec`s the binary with the user's args. The `ARGUS_NPM_WRAPPER=1` env var is added so the spawned binary can detect it was launched through npm.

## Supported platforms

| Platform | Target triple | Asset name |
|----------|---------------|------------|
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `argus-x86_64-unknown-linux-gnu` |
| Linux aarch64 | `aarch64-unknown-linux-gnu` | `argus-aarch64-unknown-linux-gnu` |
| macOS x86_64 | `x86_64-apple-darwin` | `argus-x86_64-apple-darwin` |
| macOS aarch64 (Apple Silicon) | `aarch64-apple-darwin` | `argus-aarch64-apple-darwin` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `argus-x86_64-pc-windows-msvc.exe` |

Unsupported platforms fail with a clear error message pointing to `cargo install` as a fallback.

## Pinning a version

The wrapper defaults to the version in `package.json`. To pin a different version (useful for reproducibility), set the `ARGUS_VERSION` env var:

```sh
ARGUS_VERSION=v0.1.0 npx @apohara/argus --help
```

## What to do if the binary download fails

If the postinstall hook can't reach GitHub (offline install, firewall, rate limit), it prints a warning and exits 0 so the install itself still succeeds. The first invocation of `apohara-argus` or `apohara-argus-mcp` will then fail with a clear remediation message.

To retry the download manually:

```sh
# Force a rebuild
npm rebuild @apohara/argus --foreground-scripts

# Or run the postinstall script directly
node node_modules/@apohara/argus/scripts/postinstall.js
```

To wipe the cache and start fresh:

```sh
rm -rf ~/.cache/apohara-argus
```

## License

MIT, copyright 2026 Apohara. See [LICENSE](./LICENSE).

## Links

- GitHub repo: <https://github.com/SuarezPM/apohara-argus>
- Security policy: <https://github.com/SuarezPM/apohara-argus/security/policy>
- Issue tracker: <https://github.com/SuarezPM/apohara-argus/issues>
