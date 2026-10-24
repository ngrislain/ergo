# Scaleway deployment with OpenTofu

[`infra/scaleway/`](../infra/scaleway/) defines one Serverless Container service
for the API and built web application at **https://ergosandbox.com**. It looks up
the existing **`ergo`** Object Storage bucket as a data source. Bucket contents,
configuration, and lifecycle remain outside this stack, including on destroy.

The service uses `fr-par` by default, port 8080, HTTP/1 upstream, managed HTTPS,
1000 mvCPU, at least 2 GiB memory (2148 decimal MB to match the API's precision),
a 300-second request timeout, and concurrency-based
autoscaling with **`min_scale = 0`** and **`max_scale = 1`**. The maximum is
configurable. Scaleway controls idle scale-down timing (currently documented as
15 minutes); cold starts add latency, and active requests/SSE streams can keep an
instance running. Object storage, an image registry, and other provider usage
can still incur charges when compute is at zero.

The public project/agent guide is hosted separately on GitHub Pages at
**https://www.ergosandbox.com**. [`pages.tf`](../infra/scaleway/pages.tf) manages
its `www` CNAME to `ngrislain.github.io`; the sandbox's apex ALIAS and OAuth
origin remain on `ergosandbox.com`. See [the website guide](static-site.md).

## Current deployment

The server was deployed on **2026-10-24 UTC** in the dedicated Scaleway **Ergo**
project, in `fr-par`:

- Canonical origin: **https://ergosandbox.com**; workbench: **https://ergosandbox.com/app**.
- Container: `ergo-api` in the `ergo` Serverless Containers namespace.
- Diagnostic endpoint: `https://ergofd9bc093-ergo-api.functions.fnc.fr-par.scw.cloud`.
- Public Linux/AMD64 image:
  `ghcr.io/ngrislain/ergo@sha256:4f4ac52bcd3b52b52728d3d29768d7c9d5938c42c761ef99124cf5a48f3e7882`,
  built from revision `1e206fdcfbf7cbf4c451dc017cbc3b06e822ef3e` by
  [CI run 40054724995](https://github.com/ngrislain/ergo/actions/runs/40054724995).
- The existing `ergo` bucket uses the `ergo` application prefix. The apex ALIAS
  points to the container endpoint, and the custom-domain/TLS binding is ready.
- Production inputs are in the ignored `infra/scaleway/terraform.tfvars`;
  storage, Google OAuth, and persistent session-signing credentials were copied
  from the ignored root `.env`. Update both copies when rotating credentials.

HTTPS responses for the landing/workbench pages, JS/WASM assets, and `/api/config`
were verified. The configuration reports Scaleway S3 storage. Google login
redirects to the configured OAuth client and canonical callback with a secure
HttpOnly state cookie; anonymous sessions have no development identity and
cannot issue MCP tokens. After registering the exact callback in the Google
client, the complete browser sign-in/code-exchange flow was verified: the sandbox
reports a signed-in Google session with development identity disabled.
OpenTofu reports no pending changes.
Public DNS resolution was verified through both `1.1.1.1` and `8.8.8.8`.

## 1. Build and publish the image with Podman

The new [distribution workflow](distribution.md) prepares prebuilt images at
`ghcr.io/ngrislain/ergo:VERSION`. Once a version is published and public, set
OpenTofu's `image` directly to that tag/digest without rebuilding Rust. Scaleway
supports public GHCR images; private external registry images are not supported.
Confirm that the version's GHCR job succeeded and the package is public before
applying the image reference. Local builds remain supported as described below.

### Build and publish in GitHub Actions

For an image-only deployment, use the release workflow's explicit manual
publication option after that workflow is present on `main`:

```sh
gh workflow run release.yml --repo ngrislain/ergo --ref main -f publish_container=true
gh run list --repo ngrislain/ergo --workflow release.yml --limit 5
gh run watch RUN_ID --repo ngrislain/ergo --exit-status
```

Replace `RUN_ID` with the new run's ID. CI builds the Linux/AMD64 API/web image
with Podman, smoke-tests it, and pushes it to GHCR using the automatically
supplied `GITHUB_TOKEN`. No local Podman build, registry personal access token,
or npm publication is required. This mode skips the native/npm build matrix;
ordinary manual runs without `publish_container=true` remain build/test only.

The workflow summary includes a `sha-SHORTSHA-RUN_ID-ATTEMPT` tag and a
digest-pinned image reference. Copy the **digest** into `image` in
`infra/scaleway/terraform.tfvars`. Manual publication does not update release
version tags or `latest`.

Set the GitHub package's visibility to **public** in its
[package settings](https://github.com/users/ngrislain/packages/container/ergo/settings)
before applying OpenTofu: Scaleway cannot pull private external images. Continue
with credentials/Google SSO below, then create DNS before enabling the domain's
managed TLS binding. Image publication does not deploy the Scaleway service.

### Local image build

The root [`Dockerfile`](../Dockerfile) builds the AMD64 API server and both WASM
clients using Rust 1.97.1 and `wasm-bindgen-cli` 0.2.129. Only the server, web
assets, and CA certificates enter the non-root Debian runtime image. Build from
the repository root; local `.env`, data, state, and build outputs are excluded.
Scaleway requires **Linux/AMD64**, including when building on an Apple Silicon
machine. Podman builds the existing Dockerfile directly. On macOS, initialize a
Linux VM once (or start your existing machine); 8 GiB of build memory is useful
for the Rust/WASM toolchain.

Both compilation stages execute on the build machine's native CPU. The server
uses the Rust AMD64 target and, on ARM builders, Debian's cross-GCC toolchain;
Rust/LLVM itself does not run under AMD64 emulation. The final runtime remains
Linux/AMD64, with compatible Debian Bookworm libraries.

```sh
podman machine init --memory 8192
podman machine start
```

For an already initialized, stopped machine, use `podman machine set --memory
8192` instead of initializing another VM. Linux hosts use Podman directly.
An Apple Silicon VM needs AMD64 emulation (QEMU or Rosetta); check it with
`podman run --rm --platform linux/amd64 docker.io/library/debian:bookworm-slim uname -m`
and expect `x86_64`.

Build and test locally before pushing:

```sh
export IMAGE=rg.fr-par.scw.cloud/YOUR-EXISTING-REGISTRY/ergo:YOUR-IMMUTABLE-BUILD-TAG
podman build --platform linux/amd64 --format docker \
  --build-arg BUILDARCH="$(podman info --format '{{.Host.Arch}}')" \
  --tag "$IMAGE" .
python3 scripts/test-container.py --engine podman --image "$IMAGE"
podman login rg.fr-par.scw.cloud
podman push "$IMAGE"
```

The explicit `BUILDARCH` gives Podman's native WASM build an architecture-specific
cache key. The `docker` manifest format is an interoperable image format, not a
Docker daemon dependency. The application still targets Linux/AMD64.

Podman supports a separate registry auth file via `REGISTRY_AUTH_FILE` or
`--authfile`. If your machine's Docker credential helper is unavailable, use
a Podman-specific auth file instead of changing the Docker configuration. Public
base images can be pulled with an explicit auth file containing `{"auths":{}}`;
use `podman login --authfile /path/to/podman-auth.json` for a private registry.

Supply a registry image
accessible to the container's project: for a private Scaleway registry, use the
appropriate same-project image access; a public OCI image is also supported by
Scaleway. Registry creation and image pushes are separate from this stack. Use a
new immutable tag or digest for each deployment, then update the `image` input.

The image listens on `0.0.0.0:8080`. `/api/config` is its startup/liveness probe.
The server drains active HTTP requests on SIGTERM/SIGINT. Forced termination and
unacknowledged publications still need the ordinary transaction recovery process
described in [storage operations](storage-operations.md); ephemeral container
disk is never the production object store.

## 2. Configure credentials and Google SSO

Install OpenTofu 1.11 or newer. The module pins the Scaleway provider to the
2.88.x series; commit its dependency lock file. Terraform-compatible HCL is used,
while the mocked tests use OpenTofu's test runner.

Set provider credentials and the project containing the bucket/container:

```sh
export SCW_ACCESS_KEY=YOUR-PROVISIONING-ACCESS-KEY
export SCW_SECRET_KEY=YOUR-PROVISIONING-SECRET-KEY
export SCW_DEFAULT_PROJECT_ID=YOUR-PROJECT-UUID
```

Copy [`terraform.tfvars.example`](../infra/scaleway/terraform.tfvars.example)
to `infra/scaleway/terraform.tfvars`, set its project/image/Google client ID, and
keep the existing bucket's actual region and prefix. Defaults are bucket `ergo`,
region `fr-par`, prefix `ergo`, and domain `ergosandbox.com`. Changing the prefix
selects a different application namespace; it does not migrate existing objects.

Supply secrets through environment variables or an ignored local variable file:

```sh
export TF_VAR_s3_access_key_id=YOUR-APPLICATION-S3-ACCESS-KEY
export TF_VAR_s3_secret_access_key=YOUR-APPLICATION-S3-SECRET-KEY
export TF_VAR_google_client_secret=YOUR-GOOGLE-CLIENT-SECRET
export TF_VAR_session_secret=YOUR-PERSISTENT-RANDOM-SECRET-OF-AT-LEAST-32-CHARACTERS
```

The application key needs list/read/write/delete access for its bucket objects;
it can be separate from the provisioning credential. Secrets are injected as
Scaleway secret environment variables. They remain sensitive inputs/state:
store OpenTofu state securely and retain it between deployments. State, plans,
and `.tfvars` files are Git-ignored; the default backend is local.

### Private configuration backup

The configured `.env` is backed up as the private object
`s3://ergo/_ops/ergo/production.env`, outside the application's `ergo/` data prefix.
Its object ACL and bucket policy were checked, anonymous access was denied, and
the downloaded bytes matched the local file's checksum. It contains credentials
and the persistent signing key, so access requires authorized S3 credentials.

With `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` set for that bucket, restore
it on another machine with:

```sh
umask 077
aws --endpoint-url https://s3.fr-par.scw.cloud --region fr-par \
  s3 cp s3://ergo/_ops/ergo/production.env .env
chmod 600 .env
```

Refresh that backup after editing or rotating `.env`. Production bind address
and public origin are supplied by the OpenTofu module rather than the local
development defaults in `.env`. The backup does not include OpenTofu state:
retain/transfer `infra/scaleway/terraform.tfstate` separately before managing this
existing stack from another machine. Keep local state and saved plans private;
`umask 077` before running OpenTofu also protects newly created files.

Register **https://ergosandbox.com/auth/google/callback** on the Google OAuth web
client. The module requires Google credentials and a stable signing secret rather
than deploying the shared development identity. The secret must survive cold
starts and be identical across instances; rotating it invalidates existing
sessions and MCP delegations. `ERGO_PUBLIC_URL` is fixed to the configured HTTPS
domain, keeping callback URLs, secure cookies, and write-origin checks aligned.

`extra_environment_variables` supports additional non-secret application
settings. To use measured connector capacities, build a derived image containing
your sourced measurement file and set `ERGO_CONNECTION_MEASUREMENTS` to its image
path. Missing measurements continue to block calibrated publication.

## 3. Establish DNS, then bind the domain

By default, OpenTofu manages an apex **ALIAS** record in the Scaleway DNS zone
`ergosandbox.com`. The registered domain must already be delegated to Scaleway's
nameservers. The ALIAS targets the container endpoint with a trailing dot;
an ordinary apex CNAME is not used. Existing conflicting A/AAAA/ALIAS records
must be reconciled or imported into the module before applying.

Scaleway's custom-domain certificate challenge needs working public DNS. On the
first deployment, create the container and DNS first:

```sh
umask 077
tofu -chdir=infra/scaleway init
tofu -chdir=infra/scaleway plan -var=bind_custom_domain=false
tofu -chdir=infra/scaleway apply -var=bind_custom_domain=false
tofu -chdir=infra/scaleway output -raw dns_alias_target
dig ergosandbox.com @1.1.1.1
dig ergosandbox.com @8.8.8.8
```

Once public resolvers resolve the domain to the container's endpoint addresses,
create the domain binding and let Scaleway issue its managed TLS certificate:

```sh
tofu -chdir=infra/scaleway apply
tofu -chdir=infra/scaleway output -raw app_url
curl --fail https://ergosandbox.com/api/config
```

An upstream client resolver can retain an earlier negative DNS response even
after the public resolvers return the correct address. Wait for that negative
cache to expire or use a working resolver. For an HTTPS diagnostic through
Cloudflare DNS-over-HTTPS, preserving normal certificate verification:

```sh
curl --fail --show-error --doh-url https://cloudflare-dns.com/dns-query \
  https://ergosandbox.com/api/config
```

For external DNS, set `manage_dns = false`, apply with
`bind_custom_domain=false`, create an ALIAS/ANAME or flattened CNAME at your
provider using `dns_alias_target`, wait for propagation, and apply normally.
Use DNS-only routing while Scaleway performs its certificate challenge.

Subsequent image/configuration deployments use normal `tofu plan` / `tofu apply`.
The custom domain and persistent object store remain the same. The native
container endpoint is also output for diagnostics; browser sign-in and writes
should use the canonical `ergosandbox.com` URL.

## Checks

The root page provides Google SSO and MCP/API token setup; `/app` is the
workbench and bounded shared-world viewer. The deployment defaults to satellite
imagery and a free edit allowance of 5,000 changed bricks refilled per day.
`free_bricks_per_period` and `edit_period_seconds` configure this pacing;
operator-managed paid entitlements remain persisted in the same bucket and do
not change authentication or physical validation. See
[onboarding and plans](access-and-onboarding.md).

The actual image smoke test checks Linux/AMD64 metadata, non-root execution,
startup, landing/workbench and WASM assets, import/save/read-back, and graceful
SIGTERM shutdown in an isolated container. It disables imagery/asset fetching.
Add `--browser` in the Playwright Python environment for the complete browser
flow against the running container. Docker also works through `--engine docker`.

The v0.3.0 Linux/AMD64 image has been built and exercised using Podman 6.1 on
Apple Silicon: the native cross-build, non-root runtime, API round-trip,
Playwright browser flow, and graceful shutdown pass. The current cloud deployment
uses the v0.3.1 image produced and smoke-tested by the image-only CI run above;
cloud apply, DNS propagation, and managed-TLS provisioning have completed.

```sh
tofu -chdir=infra/scaleway fmt -check -recursive
tofu -chdir=infra/scaleway init -backend=false
tofu -chdir=infra/scaleway validate
tofu -chdir=infra/scaleway test
```

Tests mock the provider: no cloud resources or bucket objects are changed. They
check scale-to-zero, existing-bucket configuration, apex DNS/origin consistency,
secret injection, and invalid production settings. Actual image builds, cloud
deployment, DNS delegation, certificate issuance, and Google sign-in require the
operator's Podman/registry/cloud environment.

Provider references: [container](https://registry.terraform.io/providers/scaleway/scaleway/latest/docs/resources/container),
[custom domain](https://registry.terraform.io/providers/scaleway/scaleway/latest/docs/resources/container_domain),
[existing bucket](https://registry.terraform.io/providers/scaleway/scaleway/latest/docs/data-sources/object_bucket),
[apex-domain setup](https://www.scaleway.com/en/docs/serverless-containers/how-to/add-a-custom-domain-to-a-container/).
