# Operations runbook

Run maintainer commands from the repository root. Credentials, token files,
OpenTofu state/variables/plans, generated images and bundles are private/ignored.

## Build and verify

Native shared edits now need writable private temporary disk space for
[working-state spilling](native-working-state.md). Defaults are 512 MiB main-file
quota and a 1 MiB SQLite cache per working store; configure
`ERGO_SHARED_SPILL_BYTES` / `ERGO_SHARED_SPILL_CACHE_KIB` and provision `TMPDIR`
for concurrent edits. Scratch is disposable, never accepted state or a resumable
session. Disk failures refuse publication rather than falling back to RAM.
Commit metadata also uses private temporary files. Configure
`ERGO_SHARED_COMMIT_BYTES` (128 MiB default, also the aggregate batch-record budget)
and `ERGO_SHARED_COMMIT_ITEM_BYTES` (1 MiB default); see
[native commit records](commit-history.md#bounded-native-commit-records) for the
Rust API migration and remaining conditional-upload buffer. These do not rewrite
accepted records or increase server RAM settings.

On Scaleway sandbox v2, `/tmp` is RAM-backed. The v0.5.35 image and module set
`TMPDIR=SQLITE_TMPDIR=/var/lib/ergo-tmp` on the writable disk layer, owned by UID/GID
10001 with mode 0700. The module adds approximately 4 GiB of ephemeral disk and
keeps the existing RAM/CPU settings. Verify the image's scratch variables and
filesystem smoke; do not solve disk exhaustion by silently using tmpfs or more RAM.

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
cargo xtask build-web
cargo build --locked -p ergo-server -p ergo-mcp
python3 scripts/check-docs.py
python3 scripts/test-distribution.py
python3 scripts/test-mcp.py
python3 scripts/test-model-library.py
python3 scripts/test-mcp-atomic-replace.py
python3 scripts/test-render-materials.py
```

With Playwright/Chrome/Pillow installed:

```sh
python3 scripts/test-common-world.py
python3 scripts/test-dense-world.py
python3 scripts/test-web-isolated.py --script scripts/test-browser-cache.py
python3 scripts/test-service-worker.py
python3 scripts/test-web-isolated.py --script scripts/test-catalogue.py
python3 scripts/test-web-isolated.py --script scripts/test-headlamp.py
python3 scripts/test-web-isolated.py --script scripts/test-navigation-lighting.py
npm ci --prefix site --ignore-scripts
npm run build --prefix site
npm test --prefix site
```

Serve `site/dist` with the site's preview command for `scripts/test-site-orbit.py`.
The common-world test uses an isolated current-format store; it checks shared
commits/conflicts, public in-world list navigation, pagination, GPS and handoff.
It also exercises paged pieces, authoritative group headers, complete group
loading and sparse group moves. Its disposable startup certifies the read-only
pavilion before listening; unoptimized numerical kernels can be slow. To test
matching optimized binaries without changing the memory/work budgets:

```sh
cargo build --release --locked -p ergo-server -p ergo-mcp
ERGO_TEST_BIN_DIR=target/release python3 scripts/test-common-world.py
```

`ERGO_TEST_BIN_DIR` affects the common-world, dense-world and isolated-browser test
harnesses only, defaults to `target/debug`,
and is not passed into the child server's production configuration.
For geometry-dependent walking guarantees, use the native
[walking route checker](navigation.md#checking-a-walking-route) against a complete
construction cache and its explicit route. It verifies continuous traversal,
arrival heights and clearance with the same movement engine as the viewer.

For read-only transfer timings and JSON/gzip/CBOR comparisons, run
`python3 scripts/profile-shared-loading.py --cell 156 0 0 --radius 21 --output target/loading-measurements.json`.
This reports wire/decoded sizes, server cache headers and TTFB, not WebGL frame
time. Compare cold, persistent and warm requests explicitly; network warmth is
not a performance guarantee. See [loading and caching](loading-and-cache.md).

For a bounded **read-only** profile of the authoring dependency walk, use the
operator CLI with the configured storage namespace:

```sh
cargo build --release --locked -p ergo-server
target/release/ergo-server shared-profile-dependencies WORLD_ID ASSEMBLY_ID 120
```

The optional deadline defaults to 120 seconds and accepts 1–1,800. The command pins
the accepted starting revision, loads the selected assembly's rule records and
warms its piece records, then runs the same combined-member-bounds dependency
discovery as whole-assembly replacement, with an independent 100,000-piece diagnostic cap and the configured
working-byte budget. It reports completion/timeout/errors, loaded
counts, cache statistics and phase times for piece/rule/mesh reads, external joints
and spatial neighbours. Timeout output contains partial **diagnostic counts**,
never an authoring scene or a passing validation certificate. The new
`rules_milliseconds` and `prefetch_milliseconds` fields record completed phases;
null means that phase had not finished when the deadline expired. The `discovery`
phase timings cover the ordinary external frontier; scanning the selected members
and their combined bounding region happens before that frontier and is included
in the total elapsed time, not those sub-phase counters. Compare a cold
run with the same immutable revision and the current release build; packed browser
regions do not themselves accelerate authoring member rules. It stages no edits,
does not publish a candidate/head and explicitly closes its snapshot/transaction
handles. It may create and release normal GC activity pins; it never clears other
requests' pins. Run a fresh process for a cold process-cache comparison. Concurrent
accepted edits do not change the pinned diagnostic revision.

To reproduce an explicit **full assembly materialization** without returning its scene,
use `ergo-server shared-profile-workspace WORLD_ID ASSEMBLY_ID ACTOR_ID [SECONDS]`
with the same private storage configuration. Its default deadline is 600 seconds
(allowed range 1–1,800). It checks that actor's normal read permission and
authoring entitlement, pins the current revision, then reports only the brick
count, elapsed time, errors and cache statistics. It does not mutate the world,
log credentials, or print construction geometry. In particular, a successful
`HEAD` freshness check does not establish that a full `GET` will finish.
Operator JSON goes to stdout; progress and server logs go to stderr so a
completed profile can be parsed without stripping log prefixes. This operator
diagnostic still materializes a complete scene; the new paged MCP open does not.

For the v0.5.35 paged-authoring transport/cache cutover, follow
[paged authoring](paged-authoring.md#forward-migration). Deploy matching server,
native and WASM first, stop clients, reconcile pending outcomes, and run the
locked dry-run cache migration before `--apply`. Accepted geometry and native
construction/proposal formats are unchanged; do not rewrite world roots merely
to convert disposable caches.

For `shared-profile-dependencies` only, an explicit trailing `--serve` retains
that process's verified immutable cache
and starts the ordinary API after the bounded profile. It requires an explicit
loopback `ERGO_BIND` and the configured common-world ID. Authentication, pacing,
frozen physics and authoring budgets remain the normal server settings; a
completed profile is not permission to publish. A timeout/error still reports
incomplete diagnostics. This operator workflow avoids discarding warmed reads
before a separately journalled ordinary edit; it adds no profiling HTTP endpoint.

To isolate the canonical encoding pass of a large shared replacement without
accessing storage or publishing, use a native construction cache:

```sh
cargo build --release --locked -p ergo-server --example profile_encoding
/usr/bin/time -l target/release/examples/profile_encoding /absolute/path/construction.ergo.json
```

The example reports piece count, encoded bytes, content ID and encoding time.
On macOS, `time -l` additionally reports whole-process peak resident memory,
including JSON decoding and scene materialization; it is not an isolated allocator
measurement or a complete server-save benchmark. Linux can use `/usr/bin/time -v`.
Compare the same input and build profile; byte length and content ID must stay
identical when measuring encoding changes.
One local release-build comparison on the 27,833-piece castle cache produced
15,877,389 identical bytes/content ID while whole-process peak RSS fell from
338,542,592 to 212,746,240 bytes (about 37%). Encoding took 130 ms before and
97 ms after. This isolates encoding and cache decoding on macOS; it does not
measure the complete save path or establish that production publication succeeds.

For the actual native static gate, with the target origin's installed part library:

```sh
cargo build --release --locked -p ergo-world --example validate_native
/usr/bin/time -l target/release/examples/validate_native --library /absolute/path/part-library.json /absolute/path/construction.ergo.json
```

Read `stability_report.stability_components` and
`stability_report.largest_equilibrium_component`. The latter counts bodies in the
largest general solve, not total bodies retained by the caller. The 27,833-piece
castle cache passed with 3,626 islands and no general solves. It already passed
compression before partitioning; a local whole-process comparison did **not** show
lower peak RSS for this input. Do not present island partitioning as demonstrated
end-to-end castle memory savings: setup/materialization, the spatial index and
the caller's scene remain resident. These are read-only local diagnostics, not
accepted publication receipts.

## GitHub releases

Workspace/native/package versions match; the site has its own version. Inspect
the intended diff/status/history, commit and push only the intended files, then
create a matching `vVERSION` tag. `release.yml` builds/tests matching native/WASM
bundles, a complete tarball artifact and server image. Upload the verified tarball
and checksum to the matching GitHub Release. A successful source push alone does not
prove an asset or image was published.

```sh
gh run list --repo ngrislain/ergo --workflow release.yml --limit 5
gh release view v0.5.35 --repo ngrislain/ergo
```

Verify the public tarball version/checksum, consumer install and actual MCP smoke
before changing the displayed installation source. Promote
`@ngrislain/ergo-mcp@0.5.35` only after registry and consumer checks; retain the
last verified public setting while building the candidate. GitHub also hosts the
identical complete tarball and checksum. The package includes all 138 source parts
and the reviewed wall bearing profile; check an outside-checkout npm installation
and WASM smoke rather than treating a successful source push as publication.

Registry CI uses the approved npm trusted publisher for `ngrislain/ergo` /
`release.yml`, with `ERGO_NPM_PUBLISH=true`. Run
`gh workflow run release.yml --ref main -f publish_npm=true`
for an explicit full build/test/publication, or publish the next matching version
tag. The job uses GitHub OIDC rather than a long-lived npm secret. Confirm registry
version/integrity before switching `ERGO_MCP_PACKAGE` to the npm package spec;
see [npmjs publication](distribution.md#npmjs-trusted-publication).

When verifying an upgrade in an existing browser, fetch the current application
JavaScript/WASM assets as well as checking `/api/config`. The 128-cell release
deletes incompatible IndexedDB/CacheStorage data and reloads app windows; it does
not lazily import old packets. Accepted world storage is separately migrated with
the [atomic cell migration](cells-and-migration.md), never deleted as a cache.

After publication, set the public Pages variable and rebuild:

```sh
gh variable set ERGO_MCP_PACKAGE --repo ngrislain/ergo \
  --body '@ngrislain/ergo-mcp@0.5.35'
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
tofu -chdir=infra/scaleway apply -input=false -var="image=$IMAGE" deployment.tfplan
```

`IMAGE` is the verified digest from the successful build. Ensure saved plan/input
values agree; pass the same image override when applying so automatically loaded
variable files cannot supply a different image. For a new deployment, follow [Scaleway setup](scaleway-deployment.md);
do not create duplicate resources instead of restoring existing state.

## Part library

The server refuses to start without a published part library. Before the first
deployment of a release that changes the library format, and whenever the
official LDraw library or reviews change, run the synthesis against the
production store:

```sh
REBRICKABLE_API_KEY=... ergo-server parts-sync --dry-run   # report only
REBRICKABLE_API_KEY=... ergo-server parts-sync             # publish, then move `current`
ergo-server parts-check                                     # re-synthesize from stored inputs
```

- **Rebrickable key:** without it, BrickLink numbers are neither deduplicated nor
  listed.
- **Publishing:** writes the inputs, about 4.9 GB of write-once mesh objects and
  the library before the pointer.
- **Restart:** running servers keep their installed revision until restarted.
- **Retired cache:** after the new release is live, the per-part `assets/parts/`
  prefix is unused and can be deleted.
- **Store selection:** `ergo-server` loads `.env` from its working directory, so a
  checkout `.env` with `S3_*` makes every command (including `parts-publish`)
  target that bucket. Run local and test commands from a directory without one.

Operators listed in `ERGO_OPERATOR_IDS` can also start or watch a synthesis from
a signed-in browser session: `POST`/`GET /api/admin/parts-sync`. See
[the part library](part-library.md#commands).

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
Scaleway resource-state secret values are opaque fingerprints, not usable
credentials. Preserve the verified backup or use the original private effective
inputs; never reconstruct a recovery file from those state fingerprints.
Backing it up does not replace preserving OpenTofu state. Keep source and target
heads fenced through an uncertain migration rather than relying on object age.
