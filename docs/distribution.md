# Client and server distributions

## Supported client source

The current prebuilt client is published on npm with GitHub Actions provenance:

```sh
npm install --global --ignore-scripts @ngrislain/ergo-mcp@0.5.19
ergo-mcp --version
```

The identical complete tarball and `SHA256SUMS` are also available on the
[v0.5.19 GitHub Release](https://github.com/ngrislain/ergo/releases/tag/v0.5.19).
Verify its `SHA256SUMS`, registry integrity/provenance and an installed MCP client
using the release verification workflow below.

It bundles native bridges for macOS/Linux ARM64/x64 and their matching WASM client.
Consumers need Node20+ and Chrome/Chromium, not Rust. Supported baselines are
macOS13+ and Linux glibc2.36+; Windows/musl are not advertised platforms.

The launcher chooses OS/architecture, verifies bundle/native versions/checksums
and locates packaged runtime assets. It handles browser login, origin overrides,
headless/manual attachment and token-free local proposals. Authenticated shared
editing defaults to the canonical apex service. Local files are caches when bound.

## Release automation

`release.yml` validates workspace/npm/tag consistency, builds the four native
targets and WASM assets, assembles/tests the tarball artifact and publishes the
server image. The maintainer uploads the verified tarball/checksum to its GitHub
Release. The source package alone is not the assembled bundle.
The assembled package contains the native/runtime files, license and distribution
manifest. Platform checks prevent accidentally packaging a host-only binary.

The consumer check installs the assembled package outside the checkout and exercises
real MCP discovery, local WASM editing and images. `scripts/test-distribution.py`
checks release boundaries; `scripts/test-npm-package.py` checks an actual bundle.
Canonical-library/retrofit checks launch the package's `bin/ergo-mcp.cjs`
entrypoint, which supplies its bundled `ERGO_WEB_DIR`; a bare native executable
does not discover a package's separate WASM directory automatically.

### npmjs trusted publication

Configure npm's GitHub trusted publisher for `@ngrislain/ergo-mcp`, repository
`ngrislain/ergo`, workflow **`release.yml`**, with direct publish permission. The
publish job uses GitHub OIDC (`id-token:write`), public access and npm provenance;
it does not require an `NPM_TOKEN` repository secret. npm may require an interactive
owner/security-key approval when establishing that trust.

Set repository variable `ERGO_NPM_PUBLISH=true` to publish successful `vVERSION`
release builds automatically. Manual `workflow_dispatch` with `publish_npm=true`
also builds/tests all four native bridges and matching WASM before publication.
It is mutually exclusive with the image-only `publish_container=true` path.

`scripts/publish-npm.py` accepts only a complete assembled bundle, never the private
source package. Tarball inputs are resolved to absolute local paths so npm cannot
interpret a relative directory/file name as GitHub repository shorthand. A repeated
run accepts an existing npm version only when its exact SHA-512 integrity matches
the candidate; a different artifact requires a new version.
Registry/authentication failures stop before publication. After npm accepts the
publish, read-only integrity polling allows up to ten minutes of registry
processing; it never republishes to resolve a delay. If the deadline expires,
reconcile the exact registry version/integrity before retrying. Successful publication
is confirmed by reading the npm integrity back. Source setup/CI configuration
alone does not establish that a package is available on npmjs.

## Server image

`ghcr.io/ngrislain/ergo` contains the API and generated workbench assets, runs
non-root and supports clean SIGTERM. Production pins an immutable verified
Linux/AMD64 digest in the existing Scaleway configuration. The image smoke checks
startup, API/data flow and shutdown.

### Build and publish in GitHub Actions

A `vVERSION` tag runs the complete release. `workflow_dispatch` with
`publish_container=true` runs the image-only path using GitHub's built-in registry
credentials. Verify image publication/visibility before deployment. See
[operations](operations.md) and [Scaleway deployment](scaleway-deployment.md).

## Version boundaries

The current client/server use Ergo construction/model/part descriptions and
current indexed/paged storage. Format readers do not guess older layouts. A
coordinated storage migration and matching client upgrade accompany a breaking
format change. Bound caches reload authoritative state; they are not data roots.

`ERGO_MCP_PACKAGE` is the public website's installation-source setting. Production
uses `@ngrislain/ergo-mcp@0.5.19`; npm trusted publication is enabled. Never embed a token,
S3 credentials or a credential-bearing URL in the package configuration.
