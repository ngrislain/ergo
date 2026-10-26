# Client and server distributions

## Supported client source

The current prebuilt client is the GitHub Release tarball:

```sh
npm install --global --ignore-scripts 'https://github.com/ngrislain/ergo/releases/download/v0.5.6/ngrislain-ergo-mcp-0.5.6.tgz'
ergo-mcp --version
```

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
uses the GitHub asset; npm-registry publication is disabled. Never embed a token,
S3 credentials or a credential-bearing URL in the package configuration.
