# Operations runbook

Run maintainer commands from the repository root. Credentials, token files,
OpenTofu state/variables/plans, generated images and bundles are private/ignored.

## Build and verify

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo xtask build-web
cargo build --locked -p ergo-server -p ergo-mcp
python3 scripts/check-docs.py
python3 scripts/test-distribution.py
python3 scripts/test-mcp.py
```

With Playwright/Chrome/Pillow installed:

```sh
python3 scripts/test-common-world.py
python3 scripts/test-web-isolated.py --script scripts/test-headlamp.py
npm ci --prefix site --ignore-scripts
npm run build --prefix site
npm test --prefix site
```

Serve `site/dist` with the site's preview command for `scripts/test-site-orbit.py`.
The common-world test uses an isolated current-format store; it checks shared
commits/conflicts, public in-world list navigation, pagination, GPS and handoff.
For geometry-dependent walking guarantees, use the native
[walking route checker](navigation.md#checking-a-walking-route) against a complete
construction cache and its explicit route. It verifies continuous traversal,
arrival heights and clearance with the same movement engine as the viewer.

## GitHub releases

Workspace/native/package versions match; the site has its own version. Inspect
the intended diff/status/history, commit and push only the intended files, then
create a matching `vVERSION` tag. `release.yml` builds/tests matching native/WASM
bundles, a complete tarball artifact and server image. Upload the verified tarball
and checksum to the matching GitHub Release. A successful source push alone does not
prove an asset or image was published.

```sh
gh run list --repo ngrislain/ergo --workflow release.yml --limit 5
gh release view v0.5.6 --repo ngrislain/ergo
```

Verify the public tarball version/checksum, consumer install and actual MCP smoke
before changing the displayed installation source. Registry publication remains
disabled; the supported client source is the GitHub Release asset.

After publication, set the public Pages variable and rebuild:

```sh
gh variable set ERGO_MCP_PACKAGE --repo ngrislain/ergo \
  --body 'https://github.com/ngrislain/ergo/releases/download/v0.5.6/ngrislain-ergo-mcp-0.5.6.tgz'
gh workflow run pages.yml --repo ngrislain/ergo --ref main
```

## Deploy the API/workbench

Use the existing private `infra/scaleway/terraform.tfvars` and state, with a
verified Linux/AMD64 GHCR digest. Preserve signing keys and existing resources.

```sh
tofu -chdir=infra/scaleway fmt -check
tofu -chdir=infra/scaleway init -input=false
tofu -chdir=infra/scaleway validate
tofu -chdir=infra/scaleway test
tofu -chdir=infra/scaleway plan -input=false -var="image=$IMAGE" -out=deployment.tfplan
tofu -chdir=infra/scaleway apply -input=false deployment.tfplan
```

`IMAGE` is the verified digest from the successful build. Ensure saved plan/input
values agree. For a new deployment, follow [Scaleway setup](scaleway-deployment.md);
do not create duplicate resources instead of restoring existing state.

## Native-format migration

1. Inventory the whole bucket, take a private backup, verify object hashes and
   typed references. Never print credential/profile bodies.
2. Convert source definitions/assets offline; verify expanded poses/colors,
   stable births/pieces/geometry and the full retained audit/history sequence.
3. Stage new immutable objects and new cache-prefix descriptions. Never overwrite
   different bytes under a content hash.
4. Start `shared-install-history WORLD_ID MANIFEST.json` with the correct private
   storage environment, including effective `ERGO_COMMON_WORLD_ID` and
   `ERGO_WORLD_PREFIX`. It checks the expected source head, fences the world and
   writes its ready receipt.
5. Close public traffic at the hosting gateway and verify denial on both the
   custom domain and default container endpoint. Stage the matching verified
   runtime. Startup reads the world, so it cannot become healthy while fenced.
   Keep the gateway closed, then write the target tip into the manifest's resume
   file to authorize installation and collection.
6. Wait for the installer's completed receipt and released barrier, redeploy if
   the fenced startup failed, and verify the matching runtime is ready before
   restoring public traffic. Verify new head/workspaces, identities, history,
   native part/raster reads and collection, then retire only the explicit
   superseded cache-key list.
7. Audit the complete bucket again, save the operator receipt and refresh the
   private configuration backup. Upgrade/reconnect the GitHub MCP client.

The history installer preserves the complete retained sequence range and runs
schema-aware collection. Cancellation/unknown mutation does not automatically
unlock. [Storage operations](storage-operations.md) defines the manifest and APIs.

## Account plan and maintenance

With the actual deployment's private storage environment:

```sh
target/debug/ergo-server user-plan USER_ID paid
target/debug/ergo-server user-plan USER_ID free
target/debug/ergo-server shared-maintenance-status WORLD_ID
target/debug/ergo-server shared-gc WORLD_ID
target/debug/ergo-server shared-prune WORLD_ID
```

Match the requested email to its stable profile ID before changing entitlement;
verify with the authenticated `get_edit_allowance`. The paid value enables god
mode: unlimited pacing and no authoring brick-count cap. It does not bypass
physics/authorization or byte/request budgets. Pruning requires an explicit
owner retention policy. Recover pins/barriers only after the former holder and
all in-flight mutations are settled.

## Live checks and backups

```sh
curl --fail https://ergosandbox.com/api/config
curl --fail --head https://ergosandbox.com/app
curl --fail --head https://www.ergosandbox.com/
```

Complete Google sign-in in the browser, generate a private delegated token, and
verify actual MCP read-back/rendering. Check camera GPS and saved position, shared
list navigation and source/migration version fields.

The bucket-root `_ops/ergo/production.env` is a private plaintext recovery copy.
Refresh and checksum it without logging its contents; verify private access.
Backing it up does not replace preserving OpenTofu state. Keep source and target
heads fenced through an uncertain migration rather than relying on object age.
