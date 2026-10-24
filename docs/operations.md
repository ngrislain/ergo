# Operations runbook

## Current single-sandbox operations

Use [object-store layout/migration](object-store-layout.md) for the current
`ergo/world` namespace and cleanup commands. Legacy per-world/private operations
below are compatibility/upgrade tools, not hosted world-creation workflows.
Use [shared agent editing](agent-shared-editing.md) for normal editing and
[the Haussmann test](haussmann-mcp.md) for reproducible non-destructive proposals.
The release builds GitHub artifacts with npm publication disabled unless
`ERGO_NPM_PUBLISH=true`. Never assume a green image job is an npm publication.

Run commands from the repository root unless a section says otherwise. Current
public endpoints are:

- Website: `https://www.ergosandbox.com` (GitHub Pages).
- Sandbox/API: `https://ergosandbox.com` (Scaleway Serverless Containers).
- Website release tag: `site-v0.1.2`; deployment workflow: `pages.yml`.
- Server image workflow: `release.yml` with `publish_container=true`.

The [server validation source map](validation-gates.md) identifies the overlap,
connection, and gravity checks and their implementation gaps.

## 1. Check tooling and account access

```sh
node --version
npm --version
rustc --version
cargo --version
gh auth status
gh auth switch --hostname github.com --user ngrislain
scw version
scw account project list
tofu version
git status --short
```

Use `gh auth login --hostname github.com --git-protocol ssh --web` if GitHub is
not configured, and `scw init` if Scaleway is not configured. The Scaleway **Ergo**
project is `0045feca-c5a9-4e97-9347-57bf59d963aa`, region `fr-par`.

## 2. Build, test, and preview the static website

Consumers install the prebuilt MCP package; Rust below is for building the site.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci --prefix site --ignore-scripts
npm run build --prefix site
npm test --prefix site
npm run preview --prefix site -- --port 4173
```

Open `http://127.0.0.1:4173`; stop preview with Ctrl+C. Test mouse movement,
dragging, scroll response, Add a brick, Take apart/Build it, mobile navigation,
MCP command/config copying, `/privacy/`, `/terms/`, and `/llms.txt`.
The build runs `cargo xtask build-site` for the shared renderer and canonical
logo; Rust and wasm-bindgen are required alongside Node.

The public homepage package source is a repository variable. Switch it after
the actual npm version has been published, then rebuild the Pages site:

```sh
gh variable set ERGO_MCP_PACKAGE --repo ngrislain/ergo \
  --body '@ngrislain/ergo-mcp@0.3.1'
gh workflow run pages.yml --repo ngrislain/ergo --ref main
```

To keep using the current GitHub-hosted package instead:

```sh
gh variable set ERGO_MCP_PACKAGE --repo ngrislain/ergo \
  --body 'https://github.com/ngrislain/ergo/releases/download/v0.3.1/ngrislain-ergo-mcp-0.3.1.tgz'
```

## 3. Check server validation and infrastructure

```sh
cargo test --locked -p ergo-world validation::tests
cargo test --locked -p ergo-world physical::tests
cargo test --locked -p ergo-server shared::tests
cargo test --locked -p ergo-client

tofu -chdir=infra/scaleway init -input=false
tofu -chdir=infra/scaleway fmt -check -recursive
tofu -chdir=infra/scaleway validate
tofu -chdir=infra/scaleway test
```

OpenTofu tests mock the provider; they do not deploy cloud resources. Rust tests
check the existing model, not unimplemented connector types or single-stud DOFs.

## 4. Commit, tag, and push a new website version

The current release is already tagged. For a **new** website release, bump its
version first; do not move an existing tag:

```sh
npm version patch --prefix site --no-git-tag-version
npm run build --prefix site
npm test --prefix site
git status --short
git diff --check
git diff
git log --oneline -10

git add site .github/workflows/pages.yml .gitignore docs/static-site.md
# Add any other intentional source/documentation changes by their explicit paths.
git diff --cached --check
git diff --cached --stat
git diff --cached
git commit -m "Update the public agent onboarding site"

SITE_VERSION="$(node -p "require('./site/package.json').version")"
SITE_TAG="site-v$SITE_VERSION"
git tag -a "$SITE_TAG" -m "Ergo website $SITE_VERSION"
git push --atomic origin main "$SITE_TAG"
```

`site-v*` identifies website releases. Tags beginning with `v*` trigger the
separate Rust/npm distribution release, which requires matching Cargo/npm
versions and registry authorization. `.env`, `.tfvars`, state, and plans remain
ignored; inspect the staged diff before committing.

## 5. Enable and publish GitHub Pages

One-time Pages setup (skip creation if the site already exists):

```sh
if ! gh api repos/ngrislain/ergo/pages >/dev/null 2>&1; then
  gh api --method POST repos/ngrislain/ergo/pages -f build_type=workflow
fi
gh api --method PUT repos/ngrislain/ergo/pages \
  -f build_type=workflow -f cname=www.ergosandbox.com
```

A relevant push to `main` publishes automatically. To request another deployment:

```sh
gh workflow run pages.yml --repo ngrislain/ergo --ref main
gh run list --repo ngrislain/ergo --workflow pages.yml --limit 5

RUN_ID="$(gh run list --repo ngrislain/ergo --workflow pages.yml \
  --branch main --limit 1 --json databaseId --jq '.[0].databaseId')"
gh run watch "$RUN_ID" --repo ngrislain/ergo --exit-status
```

The shell variable contains a real numeric run ID; `RUN_ID` is not a literal
argument. If the new run has not appeared yet, repeat the list command before
watching. The latest run can also be opened with `gh run view "$RUN_ID" --web`.

## 6. Configure website DNS and enforce HTTPS

The committed `infra/scaleway/pages.tf` creates `www CNAME ngrislain.github.io.`.
With the existing stack's local state and production inputs available:

```sh
umask 077
tofu -chdir=infra/scaleway plan -input=false -out=pages-dns.tfplan
tofu -chdir=infra/scaleway apply -input=false pages-dns.tfplan

scw dns record list ergosandbox.com name=www
dig @1.1.1.1 www.ergosandbox.com CNAME +short
dig @8.8.8.8 www.ergosandbox.com A +short
gh api repos/ngrislain/ergo/pages/health
gh api repos/ngrislain/ergo/pages \
  --jq '{cname,https_certificate,https_enforced}'
```

Once `https_certificate.state` is `approved`:

```sh
gh api --method PUT repos/ngrislain/ergo/pages -F https_enforced=true
curl --fail --head https://www.ergosandbox.com/
curl --fail --head https://www.ergosandbox.com/privacy/
curl --fail --head https://www.ergosandbox.com/terms/
curl --fail https://www.ergosandbox.com/llms.txt
```

Certificate issuance and edge-setting propagation can take time. The apex stays
on Scaleway; do not replace its ALIAS with GitHub Pages addresses.

## 7. Build a server image in CI

```sh
gh workflow run release.yml --repo ngrislain/ergo --ref main \
  -f publish_container=true
gh run list --repo ngrislain/ergo --workflow release.yml --limit 5

RUN_ID="$(gh run list --repo ngrislain/ergo --workflow release.yml \
  --branch main --limit 1 --json databaseId --jq '.[0].databaseId')"
gh run watch "$RUN_ID" --repo ngrislain/ergo --exit-status
gh run view "$RUN_ID" --repo ngrislain/ergo --web
```

Copy the digest-pinned image reference from the container job summary and set
`IMAGE` to it. This image-only mode builds/smoke-tests/pushes with Podman in CI
using `GITHUB_TOKEN`; no local container build or npm publish is needed. Make the
[GHCR package public](https://github.com/users/ngrislain/packages/container/ergo/settings).

## 8. Configure production inputs and deploy Scaleway

Keep real storage credentials, Google OAuth client ID/secret, and the persistent
signing key in the ignored `.env`. A temporary Google access token is not an
OAuth client secret. With shell-compatible assignments in that private file:

```sh
umask 077
set -a
. ./.env
set +a

export TF_VAR_project_id=0045feca-c5a9-4e97-9347-57bf59d963aa
export TF_VAR_s3_access_key_id="$AWS_ACCESS_KEY_ID"
export TF_VAR_s3_secret_access_key="$AWS_SECRET_ACCESS_KEY"
export TF_VAR_google_client_id="$GOOGLE_CLIENT_ID"
export TF_VAR_google_client_secret="$GOOGLE_CLIENT_SECRET"
export TF_VAR_session_secret="$ERGO_SESSION_SECRET"
export TF_VAR_bucket_name="${S3_BUCKET:-ergo}"
export TF_VAR_bucket_region="${S3_REGION:-fr-par}"
export TF_VAR_s3_prefix="${S3_PREFIX:-ergo}"
```

The current deployment also has `infra/scaleway/terraform.tfvars`; file values
override `TF_VAR_*`, so update both copies when rotating credentials. New clones
can use the exports above without creating a variable file. Preserve the session
signing key across deployments. Provisioning credentials come from the Scaleway
CLI profile or `SCW_ACCESS_KEY`/`SCW_SECRET_KEY`, separately from application S3
credentials.

For the already published image (replace with a new CI digest when upgrading):

```sh
export IMAGE=ghcr.io/ngrislain/ergo@sha256:8b961801afb57bbcb0297a84e2280326e53da9e2d753b06d54b71f09cdf88701
tofu -chdir=infra/scaleway init -input=false
```

That known-good v0.4.0 image was built and smoke-tested by
[run 40122040592](https://github.com/ngrislain/ergo/actions/runs/40122040592),
source `e570d7b35f2cf0623f0e50e3a7c5c6b5f3b0ac76`. Set the ignored variable file's
`image` to the same digest before applying a saved plan: OpenTofu checks that
current input values match the saved plan. The v0.4.0 apply changed the image and
added the common-world/prefix environment fields. Migration, cleanup and public
read-backs are recorded in [the store-layout report](object-store-layout.md).

For a **first** deployment, bootstrap DNS before binding the custom domain:

```sh
tofu -chdir=infra/scaleway plan -input=false \
  -var="image=$IMAGE" -var=bind_custom_domain=false -out=bootstrap.tfplan
tofu -chdir=infra/scaleway apply -input=false bootstrap.tfplan
tofu -chdir=infra/scaleway output -raw dns_alias_target
dig @1.1.1.1 ergosandbox.com A +short
dig @8.8.8.8 ergosandbox.com A +short
```

After public DNS resolves to the container, or for an update of the existing
deployment, run the normal plan/apply:

```sh
tofu -chdir=infra/scaleway plan -input=false \
  -var="image=$IMAGE" -out=deployment.tfplan
tofu -chdir=infra/scaleway apply -input=false deployment.tfplan
```

Do not apply this stack from another machine without its existing state. Obtain
the state first rather than attempting to create duplicate resources.

## 9. Google Console operation and live checks

In the actual Google OAuth **Web application** client, save this exact value
under **Authorized redirect URIs**, not JavaScript origins:

```text
https://ergosandbox.com/auth/google/callback
```

This is a Google Console account setting, not an `.env` or DNS change. The
branding URLs are `https://www.ergosandbox.com/`, `/privacy/`, and `/terms/`.
Restart login after saving, rather than reusing a previous Google error URL.

```sh
curl --fail https://ergosandbox.com/api/config
curl --fail --head https://ergosandbox.com/
curl --fail --head https://ergosandbox.com/app
curl --head https://ergosandbox.com/auth/google
tofu -chdir=infra/scaleway plan -input=false -detailed-exitcode
```

Open the sandbox in the browser to complete Google sign-in and generate a token.
OpenTofu exit status `0` means no changes, `2` means pending changes, and `1` means
an error. If the local network still caches an old empty DNS response, diagnose
through a public resolver without disabling TLS verification:

```sh
dig @1.1.1.1 ergosandbox.com A +short
dig @8.8.8.8 ergosandbox.com A +short
curl --fail --doh-url https://cloudflare-dns.com/dns-query \
  https://ergosandbox.com/api/config
```

## 10. Private configuration backup and restore

With authorized S3 credentials in the environment, refresh the existing private
configuration backup after editing `.env`:

```sh
umask 077
aws --endpoint-url https://s3.fr-par.scw.cloud --region fr-par \
  s3 cp .env s3://ergo/_ops/ergo/production.env --acl private
aws --endpoint-url https://s3.fr-par.scw.cloud --region fr-par \
  s3api get-object-acl --bucket ergo --key _ops/ergo/production.env
```

Restore on another machine that already has authorized S3 credentials:

```sh
umask 077
aws --endpoint-url https://s3.fr-par.scw.cloud --region fr-par \
  s3 cp s3://ergo/_ops/ergo/production.env .env
chmod 600 .env
```

The `.env` object is not a state backup. Retain/transfer
`infra/scaleway/terraform.tfstate` separately. If choosing to make a private
manual state snapshot after apply, the commands are:

```sh
aws --endpoint-url https://s3.fr-par.scw.cloud --region fr-par \
  s3 cp infra/scaleway/terraform.tfstate \
  s3://ergo/_ops/ergo/terraform.tfstate --acl private

# Restore only when this is the authoritative snapshot and no other operator is applying.
aws --endpoint-url https://s3.fr-par.scw.cloud --region fr-par \
  s3 cp s3://ergo/_ops/ergo/terraform.tfstate infra/scaleway/terraform.tfstate
chmod 600 infra/scaleway/terraform.tfstate
```

These are manual snapshots, not a configured remote backend or a locking
protocol. The existing module still uses local state. Keep operational backups
outside the application's `ergo/` prefix.
