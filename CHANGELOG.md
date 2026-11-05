# Changelog

## v0.5.24

- Checkpoint browser read preparation in bounded, private S3 cache slots, including
  sparse-directory and spatial cursors, materialized bricks/meshes and membership
  paging. Retry/restart reauthorizes the exact accepted snapshot and resumes the
  last durable boundary, without staging a world edit or treating a preview as complete.
- Retain tiny pending-read descriptors locally across reloads, fence stale
  completions and visibly report resumed/checkpointed progress. Expired historical
  snapshots restart at the current revision without mixing geometry.
- Add lightweight Merkle-revision `HEAD`/ETag freshness checks for views and
  authoring workspaces. Check current permissions and budgets before a 304.
- Reopen a checksum-verified native workspace from its local file after an
  authorized unchanged-token check, avoiding another complete model download.
  Initial authoring-workspace downloads remain atomic, not chunk-checkpointed.

## v0.5.23

- Allow one-hour browser and MCP complete-construction reads, basic-library warming
  and queued MCP jobs. Support the same one-hour hosting deadline, with a 65-minute
  MCP host allowance; ordinary metadata and publication request defaults stay short.
- Expand the adaptive origin-local data cache to 20% of reported quota, bounded to
  128 MiB–2 GiB and 4,096 URLs. Preserve the 64 MiB per-packet cap and existing
  compatible data, active leases and authorization revalidation.
- Expose on-demand local cache counts, capacity, persistence and skipped-write
  diagnostics through `ergoCacheInfo()`, without exposing geometry or URLs.
- Document completed-cache retention versus non-resumable partial previews and
  cold server reconstruction; extending deadlines is not a speed guarantee.

## v0.5.22

- Style OSM vector ground with natural areas, water, roads, street/place names
  and selected non-commercial landmarks; exclude business POIs before rendering.
  Version derived floor caches by source and style, with bounded source/render caches.
- Use cylindrical collision proxies for reviewed circular wheel/tyre datums so
  free wheels can roll and fail a stationary-pose shake test without implicit brakes.
- Preserve canonical accepted geometry when an optional saved editing hierarchy
  is unusable; continue rejecting retired constructs in incoming models.
- Warm bounded immutable piece records during replacement/dependency discovery,
  deduplicate pending closure work and report replacement phases without changing
  transaction predicates, permissions or complete-domain validation.
- Add bounded read-only dependency-phase profiling and optional loopback serving
  with the same warmed verified cache; preserve ordinary API authentication and
  admission. Warm exact selected-member spatial paths and independent neighbour
  pages as hints, retaining precise empty predicates and same-cell independence.
- Add local recipes for flush district parking inlays, a closed-roof Saint-Sulpice
  revision, two-colour jousting-lance standards and an articulated long-beam Eiffel
  prototype. Local checks are distinct from accepted publication; the articulated
  tower still lacks a static hinge-torque certificate and measured bearing capacities.
- Recognize finite round-bearing clearance for freely rolled cross axles and the
  reviewed upright jousting-lance/flag bore, without adding connector strength.

## v0.5.21

- Allow 30-minute browser region and MCP complete-construction reads, with
  revision-pinned read-only preview batches and cancellation/error rollback.
- Cache the complete current basic library locally: 138 sourced parts and five
  constructs, with verified immutable pack/manifest identities and offline reuse.
- Intern duplicate nested definitions during native save/export while preserving
  occurrence identity/order, poses, colours and physical metadata.
- Remove obsolete construct versions, the completed `retrofit_library` migration
  tool and browser legacy-geometry fallback; discard stale asset-cache entries.
- Recognize genuine stepped wall bearing faces and compound-part feet in static
  compression without introducing connector-strength assumptions.
- Cache public floor colour fields locally with source/settings identities and
  share identical GPU floor textures through a bounded pattern cache.
- Omit fully enclosed, verified-source terrain bricks from budgeted colour passes;
  use analytic subpixel stud relief under pressure and reduce Performance-mode
  raster resolution on high-DPI screens. Preserve source/picking/physics and shadows.
- Batch adjacent transparent instances without changing their depth/material order.
- Warm requested immutable identity-index paths in bounded parallel traversals
  before large imports, retaining every collision check and transaction predicate.
- Construct centred compressive reactions on real bearing patches before the
  conservative corner-only alternative, without inventing connector capacities.
- Add verified-source, open-bore Technic display LODs and current-source
  Saint-Sulpice/Eiffel Tower recipes with distinct static, walking, visual and
  authenticated-publication evidence.

## v0.5.20

- Add genuine oval/pine tree pieces, Castle wall panels, arches, capes, helmet
  variants, factory horses and saddle with pinned source geometry and catalogue masses.
- Register disjoint receiving grids for arch feet and horse hooves instead of
  inventing solid cavities through openings; preserve existing-part datums.
- Add an unlisted HTML `/catalogue` with source previews, part features, sourced
  mass/dimensions and a LEGO/LDraw colour-preview palette.
- Load shared views by physical viewing distance plus a 384-stud prefetch margin;
  use bounded sparse-cell reads, explicit radius/fog budget fallback and verified
  packet reuse across small-cell boundaries.
- Fit sunlight shadows to resident geometry and ground receivers independently
  of camera zoom, colour LOD, selection and display clipping.
- Preserve mobile image proportions and verify responsive images and the actual
  WebGL camera projection in browser regression checks.
- Add verified genuine-tree park expansion and castle mountain/moat/arched-bridge
  recipes, including plate-free standing figures, capes, helmets and horses.
  Held weapons and archer poses remain deferred pending measured capacities.

## v0.5.19

- Schedule the app-window reload without waiting inside service-worker activation.
  Waiting for a navigation inside activation can deadlock that controlled page.
- Verify first-install and existing-worker upgrades in real Chrome, including
  incompatible-cache deletion, completed activation and exactly one app reload.
- Derive visible cell dimensions from the shared constants, displaying the stud
  span and brick/plate height; label ground colours as standard LEGO swatches.

## v0.5.18

- Resize cells to 128×128 studs and 128 bricks high (384 plates), retaining one
  arc-second per horizontal cell. Atomically migrate old block indexes and exact
  permission prefixes; run only layout 4 and delete incompatible caches.
- Enforce source geometry, mating poses, canonical anchors, joint kinds and
  bearing axes/stops on the server before any gravity-only shortcut. Reject
  forged/duplicate attachments and remote welds without advancing the head.
- Add reposed genuine arms/wrists, upright spear/shield guards and bow-equipped
  archers. Keep local castle previews explicitly separate from accepted edits.
- Select exact standard LEGO swatches per stud with rounded categorical region
  boundaries, without interpolating material RGB colours.
- Expand the offline source pack to 124 parts, including 63 additions individually
  used in more than seven catalogue sets. Expose recorded usage and source-datum
  grip, pin, axle, wheel-bearing and matching rim/tyre interfaces; add a distinct-set
  inventory coverage audit without claiming exhaustive worldwide coverage.
- Add plate-free figures, guards and cars. Register figure feet by their receiving
  datums, seat wheels on rotating bearings, and hold equipment in actual hand grips.
  Retain measured capacity requirements for load-bearing joints.
- Add six pinned genuine Castle parts, sourced catalogue masses, printed Lion
  Knights guard and supported pennant common constructs. Recognize only the
  reviewed coaxial antenna/flag bore clearance; retain support and measured
  connector-capacity requirements for unsupported accessories.
- Allow explicit reviewed source-only catalogue additions through the reproducible
  official LDraw/BrickLink updater.

## v0.5.17

- Issue MCP/API delegations for 365 days by default, independently of the browser
  session's lifetime; preserve purpose, audience and expiry checks.
- Raise the default free pacing burst/refill and normal authoring working set to
  10,000 pieces; dynamic simulation retains its separate 5,000-body limit.
- Colour virtual baseplates per stud from neighbour-blended OSM terrain colours,
  with shared cell-edge samples and bounded GPU colour fields. Preserve authored
  pieces, physics, source attribution and offline snapshot compatibility.
- Add explicit CI npm publication with OIDC and immutable-artifact integrity checks;
  poll read-only while an accepted version is in asynchronous registry processing.

## Container maintenance after v0.5.16

- Resolve uncertain immutable S3 upload acknowledgements with uncached hash-verified
  reads, and retry an identical create only after a strong NotFound proof, bounded
  to three attempts. Keep mutable publication/CAS and MCP non-retry semantics intact.

## v0.5.16

- Consolidate canonical part names, dimensions, catalogue masses and official
  identity aliases into one deterministic LDraw/BrickLink-backed library lock.
  Ship all 55 selected genuine geometries in a checksum-verified 982,682-byte
  native pack, with reproducible source/metadata update and bake verification.
- Add immutable common minifigure/car references and intern repeated model
  definitions. Resolve genuine parts before submodel placement instead of creating
  another parametric box variant; retain independent occurrence identities.
- Add the ordinary validated `retrofit_library` MCP edit for current legacy
  aliases/proxy meshes, preserving IDs and nominal world bounds, and retain frozen
  operator measurements across official identity aliases.
- Expand S3 documentation with resource/reference types, exact encodings, hash
  domains, native pack/gzip details and discardable catalogue-cache records.
- Memoize repeated ordered transaction pages per exact pending root with bounded
  storage, without weakening read predicates, pending-edit visibility or conflicts.

## v0.5.15

- Derive crease-aware, angle-weighted smooth normals per GPU prototype, including
  existing frozen source meshes, while preserving sharp corners, positions,
  colours, source hashes, picking and physical geometry.
- Render catalogue transparent colours and direct transparent LDraw colours with
  lighting, dielectric highlights, global back-to-front instance ordering and
  premultiplied alpha. Keep source geometry and approximate partial shadow
  attenuation with stable light-buffer coverage.

## v0.5.14

- Revalidate construction catalogues by accepted revision rather than invalidating
  them every minute. Legacy fallback author labels are best-effort snapshot labels.
- Persist small checksum/revision-verified catalogue packets in sixteen bounded
  discardable slots per world, so a restarted API can reuse a warmed list.
  Authorization still precedes every conditional response and cache lookup.

## v0.5.13

- Speed construction discovery with bounded directory lookahead, one legacy-name
  lookup per actor/page, and unknown legacy sizes instead of full member counts.
- Retry transient metadata GET failures once, then recover list refreshes and
  background revision polls with backoff while retaining loaded content. Keep
  authorization/version checks and mutation non-retry guarantees intact.
- Keep orbit-sized sunlight coverage and night-shadow reach when switching to
  walking or flying. Preserve the lighting footprint on walking entry, follow
  subsequent movement, and keep sunlight stable while looking around.
- Preserve the night headlamp's actual eye origin, view-ray alignment, cone and
  brightness. Verify both daylight and night mode changes through real WebGL
  shadow matrices in addition to the camera-alignment regression.

## v0.5.12

- Add bounded packed spatial read pages, verified reusable region packets and
  honest loading progress for dense shared-world views.
- Merge eligible distant brick surfaces and budget color/shadow detail without
  changing source geometry, editing, picking or walking collisions.
- Add content-addressed IndexedDB caching for authorized region packets and
  discovery metadata, with verified migration, usage-aware eviction and active
  region leases. Keep navigation cancellable and bound stalled reads.
- Cache authorized construction discovery on the server and reuse proven
  canonical region packets when navigating without a previous-center hint.
- Add the read-only `get_lego_models` MCP tool for official LDraw model search,
  attributed document/submodel retrieval, configured custom libraries and explicit
  Rebrickable/BrickLink catalogue references.
- Add atomic, ID-preserving `place_submodel` replacements and genuine catalogue
  profiles for classic vehicle parts, minifigure hair and long bridge plates.
  Keep nominal collision approximations and missing connector calibration explicit.
- Add source-attributed district revision recipes for Town-style cars, genuine
  hair and third-storey building links. Shared changes still require accepted
  publication receipts; source recipes alone do not change the public world.

## v0.5.11

- Add bounded two-level immutable-node prefetch for ordered dense-region scans,
  32-way bounded spatial/piece reads and a 256 MiB verified-object cache budget.
- Allow bounded 128 MiB MCP assembly read-back responses, fixing failed confirmation
  after an already accepted large assembly edit.
- Add source-only catalogue profiles for genuine frames, slopes, fences, round
  parts, lights, factory figures and wheel/tyre units. Retain geometry credits,
  catalogue mass provenance and explicit nominal-proxy limitations.
- Expose nominal body bounds/provenance through MCP, check registered specialty
  interfaces, and keep their unknown connector strengths uncalibrated.
- Upgrade older derived asset descriptions without changing visual vertices.
- Add journalled incremental district authoring through real MCP tools and exact
  read-only reconciliation of uncertain outcomes. District decoration remains work
  in progress; accepted receipts record the actual published stages.

## v0.5.10

- Give read-only browser regions independent budgets, defaulting to 50,000 pieces
  and 64 MiB of charged CBOR reads. Ordinary authoring and dynamic body limits
  remain separate.
- Add bounded `ERGO_SHARED_VIEW_PIECES` and `ERGO_SHARED_VIEW_BYTES` settings;
  oversized regions are rejected without truncation or oversized cache reuse.
- Verify dense rendering with a real 6,000-piece browser fixture and retain
  resident-construction focus without additional geometry requests.

## v0.4.0

- Make one public Ergo Sandbox authoritative; authenticated UI/MCP edits commit
  atomically in place with stable piece IDs, conflict detection and author names.
- Add position/selection-based agent handoff and shared authoring workspaces.
- Add LDraw-aligned reusable subparts, affine 4×4 construction storage, legacy
  readers and affine visual previews without weakening physical admission.
- Distinguish DAT geometry/primitive references from physical assembly instances.
- Add progressive stud-specific geometry/relief LOD and collision-aware flight.
- Fix navigation headlamp alignment, complete homepage shadow casters, proportional
  catalog illustrations, immediate sign-out feedback and redundant import controls.
- Add fenced, hash-verified namespace migration and operator cleanup; retain active
  part/terrain caches and private account/operations records.
- Add a reproducible 868-part Haussmann-style MCP test and document its remaining
  gravity-probe/calibration limitations rather than publishing an uncertified model.
- Publish prebuilt GitHub bundles while npm publication remains explicitly disabled.

## v0.3.1

- Add tag-driven GHCR server-image publishing, retaining Podman build/smoke tests.
- Add an npm MCP launcher/bundle with native macOS/Linux binaries and the minimal
  browser/WASM runtime; consumer installs do not need Rust or install hooks.
- Default npm browser login and authenticated publishing to `https://ergosandbox.com`,
  retaining explicit server overrides and token-free local editing.
- Add version/checksum/platform release guards, offline tarball-install/MCP tests,
  and GitHub Actions artifact assembly and npm trusted-publishing/provenance support.
- Document one-time registry authorization/public visibility and distinguish
  prepared automation from actually published packages/images.

## v0.3.0

- Add authenticated MCP publication, accepted-revision read-back, browser view
  links, edit allowance inspection, and browser-assisted sign-in/setup.
- Add landing/Google SSO pages with MCP/API instructions and short-lived token
  generation, clipboard copy, and credential-file download.
- Add collaborative shared-world access, persistent per-user edit pacing
  (5,000 changed bricks refilled per day by default), and operator-managed
  unlimited paid entitlement. Authorization and physical checks still apply.
- Add bounded shared-world navigation, full-screen exploration, and camera-relative
  nearest-construction discovery.
- Resolve Studio/LDraw parts on demand, retain standard-first recommendations,
  embed the sourced full LDraw color catalog, and accept sourced operator mass/
  nominal-bound records while rejecting unmeasured publication.
- Render studded 1024×1024 ground plates with satellite/aerial-image-derived
  standard colors, JPEG imagery decoding, bounded geometric studs, and distant LOD.
- Add Scaleway/OpenTofu definitions for one scale-to-zero API/web container,
  the existing `ergo` bucket, and `ergosandbox.com` apex DNS/managed HTTPS.
- Add Podman-compatible native cross-compilation for the Linux/AMD64 server,
  packaging, and actual-container API/browser smoke coverage on Apple Silicon.
- Correct local loopback-origin validation and handle container SIGTERM shutdown.

Shared-world browser views are read-only spatial regions; edits use MCP/REST.
Billing-provider automation, exact global nearest-neighbor search, and optical
material simulation remain future work. Live cloud deployment is separate from
the repository's mocked infrastructure and isolated application checks.
