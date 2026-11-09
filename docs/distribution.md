# Client and server distributions

## Supported client source

The current prebuilt client is published on npm with GitHub Actions provenance:

```sh
npm install --global --ignore-scripts @ngrislain/ergo-mcp@0.5.35
ergo-mcp --version
```

The identical complete tarball and `ngrislain-ergo-mcp-0.5.35.tgz.sha256` are also available on the
[v0.5.35 GitHub Release](https://github.com/ngrislain/ergo/releases/tag/v0.5.35).
Verify its checksum sidecar, registry integrity/provenance and an installed MCP client
using the release verification workflow below.

It bundles native bridges for macOS/Linux ARM64/x64 and their matching WASM client.
Consumers need Node20+ and Chrome/Chromium, not Rust. Supported baselines are
macOS13+ and Linux glibc2.36+; Windows/musl are not advertised platforms.

The launcher chooses OS/architecture, verifies bundle/native versions/checksums
and locates packaged runtime assets. It handles browser login, origin overrides,
headless/manual attachment and token-free local proposals. Authenticated shared
editing defaults to the canonical apex service. Local files are caches when bound.

Version 0.5.26 changes tokenless npm startup to the hosted Sandbox with
human-approved browser token pickup; `--local` explicitly selects isolated
proposals. This requires a rebuilt client and matching browser deployment and is
not retroactively available in the published v0.5.25 artifact. Source plugin
wrappers for Codex/Claude are in `plugins/ergo`, with repo-local marketplace
manifests; they require an installed bridge and are not a public marketplace
release. See [plugin setup](mcp-plugins.md) and [browser approval](access-and-onboarding.md).

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
Current-only canonical-library checks launch the package's `bin/ergo-mcp.cjs`
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
is promoted to `@ngrislain/ergo-mcp@0.5.35` only after registry/consumer verification;
npm trusted publication is enabled. Never embed a token,
S3 credentials or a credential-bearing URL in the package configuration.

## Verified v0.5.35 release

The tagged source is `296bd2833e77ad0a4542e849c6a561f6f3dc4903`.
[Release workflow 42593581588](https://github.com/ngrislain/ergo/actions/runs/42593581588)
and [engine/admission workflow 42593573347](https://github.com/ngrislain/ergo/actions/runs/42593573347)
passed. Local release validation passed 417 tests, with one live-S3 test skipped;
browser/MCP integration, complete 6,000-piece browser views, service-worker and
catalogue checks, ten infrastructure tests and cache-migration guards also passed.

The complete four-platform npm bundle matches the tested CI artifact and GitHub
Release asset. Its SHA-256 is
`09c8a926b3e75458b1595fd01c3110a9969cd0cc92ce7764d2511a1b8229f3fe`;
npm integrity is
`sha512-KZreZOdF1WbgPfSk6x4wraVFlmu0a59ZGMlz6VjKBjOYGZUtIbm/o12iw2XdJFnnoCRj/8oa/duHXuXRntT7wA==`.
An outside-checkout, offline `--ignore-scripts` installation passed the actual
native/WASM MCP editing, physics, grouping and rendering/browser smoke.

The API/workbench runs the publicly pullable Linux/AMD64 image
`ghcr.io/ngrislain/ergo@sha256:8d1f4f9a37b8e9752af5357e1b0af20399542b61c38a1875454763b22eed3728`.
Its source/version labels match the tag; the actual container smoke verifies
non-root writes to disk-backed scratch. The inspected/apply-matched OpenTofu plan
updated one existing container in place: image, ephemeral disk (1,000,000,000 to
4,295,000,000 bytes) and `TMPDIR`/`SQLITE_TMPDIR` (`/var/lib/ergo-tmp`). CPU (1 vCPU),
RAM (2,148,000,000 bytes), one-hour deadline, scale-to-zero settings, secrets and
all other environment/resources were unchanged. The private image pointer and
backup were refreshed, with exact backup readback and unsigned access denied.
A post-deployment plan using the pinned private configuration reported no changes.

After deployment `/api/config` returned v0.5.35, `ergo-workspace-2` and `ergo-edit-1`.
The world ID, accepted head, frozen model and part-library revision were unchanged;
HTML/JS/WASM endpoints returned 200. An existing construction returned a metadata-only
manifest and a revision-pinned one-piece `ergo-authoring-page-1` with continuation.
No production construction edit or large-castle save was attempted; these checks
are not a fixed-RSS or arbitrary-size publication guarantee. Upgrade matching
clients and use the [safe bound-cache migration](paged-authoring.md#forward-migration).

## Historical v0.5.34 verification

The tagged source is `c2c5e43d80007390f02e0e92d499a141f75d4ea6`.
[Release workflow 42498676528](https://github.com/ngrislain/ergo/actions/runs/42498676528)
passed all four native platforms, matching web/WASM/WebGL checks, package/consumer
checks, articulation tests, npm trusted publication and container verification.
The public npm tarball is byte-identical to the tested CI artifact and the GitHub
Release asset. Its SHA-256 is
`c19a0deb5874e9b726e5e4dbbb4b7139b17839d73d2db5502d7a043e21ff794e`.
An outside-checkout, offline `--ignore-scripts` consumer installation also passed
the real native/WASM MCP editing, physics and rendering smoke.

That release deployed the verified Linux/AMD64 image
`ghcr.io/ngrislain/ergo@sha256:7f4634eca088e738100e7c81e0d344dc97ceb66e2b708dc613ffada6347cb559`.
Its applied plan changed only that image: CPU (1 vCPU), RAM (2,148,000,000 bytes),
one-hour deadline, scale-to-zero settings, authentication/environment, world and
part library were preserved. The pinned private configuration had no post-deploy
OpenTofu drift. At that verification `/api/config` reported v0.5.34, and the served viewer/MCP
WASM included the component-wise stability report. Authenticated read-back of
the castle header confirms the same 27,833 pieces and accepted revision as before
deployment. No castle edit was attempted as part of this release; a successful
read is not proof that a large save now succeeds.

## Historical v0.5.21 verification

The tagged source is `1c03e4e2340300b7df5f8432dd42f8fa667d6ab3`.
[Release workflow 41804627564](https://github.com/ngrislain/ergo/actions/runs/41804627564)
passed all four native platforms, matching web/WASM, package/consumer checks and
container verification. The public npm and GitHub tarballs are byte-identical to
the tested CI artifact. Their SHA-256 is
`c87fbb6dadbd88e3c0ab0b83827fbb8b6eca1aa935afd906cda0684b1a6f5e65`.
An independent outside-checkout npm installation passed actual native/WASM
editing/rendering and current-only library checks: 138 sourced parts, five
constructs and 30 MCP tools.

That release deployed the verified Linux/AMD64 image
`ghcr.io/ngrislain/ergo@sha256:dad962af48e3f1fbc3be01d1c0726d60145bf3fc1a0ca223e72a1ddce07dbd96`.
The inspected rollout changed only the image and container timeout (to 1800
seconds); authentication/environment, world metadata, frozen physical model and
policy were preserved. Post-deployment OpenTofu verification found no drift with
the same explicit image/timeout inputs. At that verification, the public Pages package source was
`@ngrislain/ergo-mcp@0.5.21`.
