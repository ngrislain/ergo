# Ergo MCP

A prebuilt native MCP bridge plus the local browser/WASM construction runtime.
Node.js 20+ and a browser are required; Rust, Cargo, and wasm-bindgen are not
required by package consumers. The package includes macOS ARM64/Intel and Linux
ARM64/x64 binaries. macOS 13+ and Linux glibc 2.36+ are the release baselines;
Windows and Linux musl/Alpine binaries are not included.

After the package is published:

```sh
npx -y @ngrislain/ergo-mcp@0.3.1 --workspace /absolute/path/construction.ergo.json
```

MCP client configuration:

```json
{
  "mcpServers": {
    "ergo": {
      "command": "npx",
      "args": ["-y", "@ngrislain/ergo-mcp@0.3.1", "--workspace", "/absolute/path/construction.ergo.json"],
      "env": {
        "ERGO_PUBLISH_TOKEN_FILE": "/absolute/path/ergo-mcp-token"
      }
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
precedence. Local editing needs no account or token and does not contact the
hosted server at startup. To publish, supply `ERGO_PUBLISH_TOKEN_FILE`,
`--publish-token-file`, or `ERGO_PUBLISH_TOKEN` after signing in.

The default opens your browser; `--attach` prints the local attachment URL.
`--headless` requires an installed Google Chrome (or `ERGO_CHROME_BIN`). The
package does not install a browser or download executable code at startup.

For sign-in/token setup:

```sh
npx -y @ngrislain/ergo-mcp@0.3.1 --login
```

Publishing retains server authorization, pacing, and measured-physics checks.
All binaries and browser assets are bundled together at the same release version.
There are no install/postinstall scripts, optional platform downloads, or
runtime build steps. Package publication does not deploy the hosted API.

Source, setup, and tool documentation:
https://github.com/ngrislain/ergo/blob/main/docs/local-construction-mcp.md
