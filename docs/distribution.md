# Prebuilt server image and npm MCP distribution

## Status and release channels

The repository now contains versioned distribution assembly and GitHub Actions
automation for:

- **API/web image:** `ghcr.io/ngrislain/ergo:VERSION` (Linux/AMD64), also tagged
  `latest` after a stable tagged release.
- **MCP package:** `@ngrislain/ergo-mcp@VERSION` on the public npm registry, with
  the native bridge and minimal browser/WASM runtime bundled together.

The first distribution release is **v0.3.1**. A source tag or the presence of the
workflow is not evidence that either registry contains the version: confirm each
publication job separately. npm needs scope/publisher authorization, and GHCR
must have suitable package visibility. Existing tags such as v0.3.0 are not
moved; each release uses a new matching source/package tag.

## Use a published server image

After successful publication and public package visibility:

```sh
podman pull ghcr.io/ngrislain/ergo:0.3.1
python3 scripts/test-container.py --image ghcr.io/ngrislain/ergo:0.3.1
```

Use that published tag (or a pinned digest) as the OpenTofu `image` value. The
image still requires the production S3, Google SSO, public origin, and persistent
session-signing configuration from [Scaleway deployment](scaleway-deployment.md).
Pulling the image does not create cloud resources or configure authentication.

Scaleway supports public external registries, including GHCR. **Private external
registries are not supported**, so set the GitHub container package's visibility
to public before using it directly with Scaleway. GitHub can initially create a
package as private even when the linked repository is public. If the image must
remain private, copy the same version into an accessible Scaleway registry with
Podman pull/tag/push; a Rust rebuild is unnecessary.

The canonical image source is GitHub Packages; the built image includes source,
revision, and version labels. Public consumers do not need a GitHub token to pull
a public image. The workflow uses only its repository `GITHUB_TOKEN` with
`packages: write` to publish it.

## Use the published npm package

Once published:

```sh
npx -y @ngrislain/ergo-mcp@0.3.1 --workspace /absolute/path/construction.ergo.json
npx -y @ngrislain/ergo-mcp@0.3.1 --login --publish-url https://ergosandbox.com
```

Or install globally with `npm install -g @ngrislain/ergo-mcp@0.3.1` and use
`ergo-mcp` normally. Pin a version in an agent's configuration:

```json
{
  "mcpServers": {
    "ergo": {
      "command": "npx",
      "args": ["-y", "@ngrislain/ergo-mcp@0.3.1", "--workspace", "/absolute/path/construction.ergo.json"],
      "env": {
        "ERGO_PUBLISH_URL": "https://ergosandbox.com",
        "ERGO_PUBLISH_TOKEN_FILE": "/absolute/path/ergo-mcp-token"
      }
    }
  }
}
```

Node.js **20+** and an installed browser are needed. Rust/Cargo/wasm-bindgen are
not needed on a consumer's computer. The package contains four native binaries:
macOS ARM64/Intel (13+), and Linux ARM64/x64 (glibc 2.36+). Linux binaries are built
in Debian Bookworm, rather than assuming a newer GitHub runner libc. Windows and
musl/Alpine are not advertised as supported. Headless rendering still requires
Google Chrome or `ERGO_CHROME_BIN`; the package does not install a browser.

One npm tarball deliberately contains all four native binaries plus one copy of
the minimal web runtime, avoiding platform downloads and install hooks. The
wrapper selects the correct binary, preserves the caller's working directory,
arguments, environment and MCP stdio, and sets the packaged `ERGO_WEB_DIR` unless
the operator explicitly overrides it. JSON-RPC remains the native bridge's stdout;
launcher errors go to stderr. `--version` reports the package version.

Only the minimal `mcp.html`, JS/WASM runtime, launcher, native binaries, license,
README, and distribution manifest are packaged. The private source template is
**not** a publishable standalone npm package. No S3/Google credentials, workspaces,
local data, full Leptos application, or `.env` files are included. Physical-data,
publication-permission, and edit-pacing requirements are unchanged.

## Maintainer release automation

[`.github/workflows/release.yml`](../.github/workflows/release.yml) runs on `v*`
tag pushes or a manual dispatch. **Manual dispatch is build/test only**; it never
publishes. A tag push must exactly match Cargo and npm stable semver versions.

The workflow:

1. Validates the tag/source/package versions.
2. Builds the pinned minimal WASM runtime and four native binaries on matching
   Linux/macOS runners. Each native artifact is executed with `--help`, checked
   for the correct ELF/Mach-O architecture, and given a version/checksum manifest.
3. Rejects missing, mixed-version, wrong-architecture, or changed artifacts;
   assembles the complete npm package; installs its real tarball offline with
   `--ignore-scripts`; checks stdio/startup outside the checkout; and runs real
   browser/WASM MCP edits and PNG tests on Linux/x64.
4. Publishes the npm tarball with provenance on a matching release tag.
5. Independently builds/smoke-tests the API image with Podman and publishes the
   versioned/latest image to GHCR on a matching release tag.

The npm and image publication jobs are independent: a green image publication
does not imply the npm job succeeded. Inspect both before announcing a release.
The workflow does not apply OpenTofu or deploy Scaleway resources. Standard
GitHub-hosted macOS/Linux runners build the release; secrets are not available
to untrusted pull-request publication jobs.

### One-time npm setup

Confirm ownership of the intended `@ngrislain` npm scope. Configure a trusted
publisher for `@ngrislain/ergo-mcp` on npmjs.com:

- GitHub owner/user: `ngrislain`
- Repository: `ergo`
- Workflow filename: `release.yml`
- No environment name, unless the workflow is deliberately changed to use one
- Allow direct `npm publish` for this publisher

The job uses Node 24 / npm 11 with `id-token: write` and `--provenance`. First
publication/bootstrap may need the maintainer's npm login or a granular publish
token stored as repository secret **`NPM_TOKEN`**; the workflow supports that
bootstrap path. Configure trusted publishing and remove an unnecessary bootstrap
token afterward. Tokens are not checked into this repository or embedded in npm
artifacts. Published npm versions are immutable; bump all version files rather
than republishing the same version.

### Local assembly and checks

`scripts/package-mcp.py` provides `version`, `native`, `web`, and `npm` commands.
Artifacts live in an ignored `target/` directory. `--allow-partial` permits a
host-only local test bundle but marks it `private: true`, preventing publication.
A complete release requires every native platform. Each output directory must
be new; the assembler never deletes an existing directory.

For an Apple Silicon local test after `cargo build --release -p ergo-mcp` and
`cargo xtask build-web` (use `darwin-x64` on Intel, or the matching Linux key):

```sh
python3 scripts/package-mcp.py native --platform darwin-arm64 \
  --binary target/release/ergo-mcp --output target/npm-check/artifacts/native-darwin-arm64
python3 scripts/package-mcp.py web --output target/npm-check/artifacts/web-runtime
python3 scripts/package-mcp.py npm --artifacts target/npm-check/artifacts \
  --output target/npm-check/npm --allow-partial
python3 scripts/test-npm-package.py --package-dir target/npm-check/npm --browser
node --test packages/mcp/test/*.test.cjs
python3 scripts/test-distribution.py
```

The macOS ARM64 package has been assembled, packed, installed offline with
install scripts disabled, and exercised through actual MCP/WASM editing/rendering
outside the repository. Workflow syntax and release guards are checked locally;
the other native targets and public registry publication use the release CI
and maintainer registry configuration. The ordinary source-build workflow remains
supported until prebuilt releases are available.

References: [npm trusted publishing](https://docs.npmjs.com/trusted-publishers),
[GitHub container publishing](https://docs.github.com/en/actions/tutorials/publish-packages/publish-docker-images),
[Scaleway external registries](https://www.scaleway.com/en/docs/serverless-containers/how-to/deploy-container/#deploy-from-an-external-container-registry).
