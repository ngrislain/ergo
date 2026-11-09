# Ergo MCP

A prebuilt native MCP bridge plus the local browser/WASM construction runtime.
Node.js 20+ and a browser are required; Rust, Cargo, and wasm-bindgen are not
required by package consumers. The package includes macOS ARM64/Intel and Linux
ARM64/x64 binaries. macOS 13+ and Linux glibc 2.36+ are the release baselines;
Windows and Linux musl/Alpine binaries are not included.

For a version published on npm, install the complete bundle with:

```sh
npm install --global --ignore-scripts @ngrislain/ergo-mcp
ergo-mcp --workspace /absolute/path/construction.ergo.json
```

Verified GitHub Release tarballs can also be installed by their `.tgz` URL.
An npm bootstrap staging placeholder is not a complete client release.

MCP client configuration:

```json
{
  "mcpServers": {
    "ergo": {
      "command": "ergo-mcp",
      "args": ["--workspace", "/absolute/path/construction.ergo.json", "--headless"]
    }
  }
}
```

The workspace parent directory must exist. The wrapper preserves the caller's
working directory, arguments, environment, and MCP stdio, while resolving the
bundled WASM assets independently of the source checkout. `ERGO_WEB_DIR` or the
native `--web-dir` option can explicitly override the packaged assets.

Login and authenticated publishing default to **https://ergosandbox.com**.
Override the server with `ERGO_PUBLISH_URL` or `--publish-url`; the CLI option takes
precedence. With a production token, accepted edits automatically update the one
public Ergo Sandbox and the file is a cache. In the current source, missing or
HTTP-401-rejected credentials automatically open browser sign-in and explicit
human approval, then save the approved token privately for the configured origin.
Tokenless npm startup uses the hosted Sandbox; `--local` explicitly selects
isolated developer/proposal tests. Supply `ERGO_PUBLISH_TOKEN_FILE`,
`--publish-token-file`, or `ERGO_PUBLISH_TOKEN` to override credential lookup.
These source changes require a rebuilt bundle and matching browser deployment;
they are not retroactively present in the already-published v0.5.25 package.

The default opens your browser; `--attach` prints the local attachment URL.
`--headless` requires an installed Google Chrome (or `ERGO_CHROME_BIN`). The
package does not install a browser or download executable code at startup.

For sign-in/token setup:

```sh
ergo-mcp --login
```

This completes approval and saves the token before exiting. Use it before
connecting clients whose startup deadline is too short for interactive sign-in.
The default token path is `~/.config/ergo/tokens/<origin-sha256>.token`
(`$XDG_CONFIG_HOME` is honored), with Unix mode 0600. No token is put in chat,
workspace files, URLs, or MCP stdout. Source plugin wrappers for Codex and Claude
Code are available under `plugins/ergo` in the repository; they require the bridge
to be installed and are not a public plugin-directory listing.

Publishing retains server authorization, pacing, and measured-physics checks.
The client uses native Ergo construction/model/part descriptions. `get_user_position`
reads your private saved camera context and a ground-level transform for model
creation; `place_submodel` accepts native definitions and public-unit transforms.
Existing shared constructions stay at their accepted world positions.
All binaries and browser assets are bundled together at the same release version.
There are no install/postinstall scripts, optional platform downloads, or
runtime build steps. Package publication does not deploy the hosted API.

v0.5.35 uses paged shared authoring: opening loads metadata, and
`read_construction_page` explicitly loads geometry before sparse edits. Upgrade
the server, native bridge and browser/WASM together. Bound pre-v0.5.35 caches need
the locked, backup-preserving [forward migration](https://github.com/ngrislain/ergo/blob/main/docs/paged-authoring.md#forward-migration);
do not discard or migrate a cache with an uncertain publication outcome.

Source, setup, and tool documentation:
https://github.com/ngrislain/ergo/blob/main/docs/local-construction-mcp.md
