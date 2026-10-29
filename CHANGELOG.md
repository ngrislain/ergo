# Changelog

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
