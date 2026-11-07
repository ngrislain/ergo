# The part library and common constructs

## One synthesized library

Ergo has exactly one part library: a content-addressed document of format
`ergo-part-library-2`, produced by the operator synthesis
(`ergo-server parts-sync`, implemented in `crates/assets/src/library.rs`) and
installed once per process by `ergo_world::part_library`. Every part of the
**official LDraw library** that can be baked and profiled is buildable. Its
geometry is deduplicated against **BrickLink** item numbers, using a
**Rebrickable** catalogue snapshot for the LDraw ↔ BrickLink mapping.

Each entry is one of two tiers:

- **Reviewed** (138 parts): human-reviewed physical records committed in
  `crates/world/library/reviews.json`. These are BrickLink catalogue masses and
  set counts, rectangular sizes, nominal bodies, stud/socket grids and connectors.
  44 are rectangular bricks, plates and tiles; 94 are source-only architectural,
  figure, vehicle and Technic parts. They are the recommended building set.
- **Derived** (`derivation: "auto-v1"`): every other buildable official part.
  Its nominal body, top studs and receiving grids are derived automatically from
  the official geometry (`crates/world/src/derive.rs`). Its mass is a
  calibrated volume estimate. These are not reviewed records.

BrickLink items without official LDraw geometry are kept as **listed** entries.
They can be searched but not built, and no substitute shape is invented.

Each entry pins its geometry by `mesh_hash` (SHA-256 of the native Mesh CBOR
stored at `objects/<mesh_hash>.cbor`) and `vertex_hash`. The latter covers the
complete internal float32 stream, with signed zero normalized. A part `revision`
covers the synthesis version, every source file of its closure with its SHA-256,
and its catalogue record. The library `revision` is the SHA-256 of the compact
document serialized with an empty revision. Every reader verifies it before
installing: the server, the browser, the MCP and the static site.

## Deterministic synthesis

`parts-sync` downloads the official `complete.zip` and, when
`REBRICKABLE_API_KEY` is set, a Rebrickable snapshot. That snapshot is the paged
`/api/v3/lego/parts/?inc_part_details=1` listing with external IDs, plus part
categories, canonicalized and sorted. Every input is stored content-addressed at
`catalog/inputs/<sha256>`, including the reviews document used. The library
records their hashes in `inputs`. `synthesize` is a pure function of these input
bytes, so the same inputs always produce byte-identical library JSON and mesh
objects. Parts are processed in parallel; the output is ordered by ID.

1. **Universe.** Classify every top-level `parts/*.dat` by its `!LDRAW_ORG`
   header:
   - `Part` and `Shortcut` files are candidates.
   - `Physical_Colour` and `Flexible_Section` files are excluded.
   - Obsolete `~`/`_` files are kept only when reviewed or when the Rebrickable
     catalogue still maps to them.
   - Source text is UTF-8 without BOM, with CRLF/CR normalized to LF.
2. **Identity deduplication.** Official `~Moved to` files and `Part Alias` files
   whose only geometry is one identity placement (colour 16, zero offset,
   identity matrix) become aliases of their target. Chains are followed. Similar
   bounding boxes are never identity evidence; an origin-changing alias would
   need a pose migration.
3. **BrickLink deduplication.** A Rebrickable part groups its LDraw and
   BrickLink numbers. Each BrickLink number becomes an alias of the canonical
   LDraw geometry it maps to (`bricklink` lists them all), with three exceptions:
   - when the number is ambiguous (claimed by two geometries);
   - when it is itself the name of an official LDraw file;
   - when it is a reviewed ID.

   A Rebrickable part without official geometry becomes a listed entry.
4. **Bake and profile.**
   - Each part is baked by the same LDraw baker as everywhere else.
   - Reviewed parts take their review record.
   - Derived parts take an `auto-v1` profile; see [part descriptions](part-catalog.md#derived-profiles-auto-v1).
   - Parts that cannot be baked or have no enclosed volume are excluded and
     listed in the synthesis report. In the 2026-11 archive these were 2,460
     stickers, about 70 cloth/sheet/string items and 16 parts with official
     dependencies missing from the archive.
5. **Mass calibration.**
   - The ABS voxel volume estimate (1.05 g/cm³) is fitted to the reviewed
     BrickLink masses by a single median factor.
   - The library records the factor, the sample count and the p50/p90 relative
     errors.
   - The 2026-11 synthesis measured factor 0.9562 over 138 parts, with p50 8.3%
     and p90 28.3%. Large outliers remain where official models are solid but
     mouldings are thin; the 32 × 32 baseplate is estimated at about three
     times its real mass.

The 2026-11 official archive (SHA-256 `0230839…610f`, the same snapshot as the
previous reviewed lock) gives **17,856 buildable parts**: 138 reviewed, 17,718
derived, and 1,284 official identity aliases. Without a Rebrickable key there are
no BrickLink aliases or listed items. The library JSON is about 16.8 MB (3.6 MB
gzip). The mesh objects total about 4.9 GB. A full synthesis takes about one
minute on a 10-core laptop.

## Commands

```sh
ergo-server parts-sync              # pull, synthesize, publish and move `current`
ergo-server parts-sync --dry-run    # pull and synthesize; print the report only
ergo-server parts-check             # re-synthesize `current` from stored inputs, byte for byte
ergo-server parts-publish LIBRARY.json OBJECTS.cbor.gz  # publish a prepared, verified bundle
```

- **Publication order:** inputs, then objects (write-once), then the library
  document, then the `catalog/part-library/current.json` pointer.
- **Memory:** objects stream to the store while baking, through a bounded
  channel, and each worker's source cache is capped at 32 MiB. A full synthesis
  peaks below 2 GB with ten workers.
- **Startup:** a server whose store has no published library runs the first
  synthesis before serving. Production publishes beforehand with the CLI,
  because a 1-vCPU container needs about six minutes, longer than its startup
  probe allows.
- **HTTP API:**
  - `POST /api/admin/parts-sync` (body `{"dry_run": bool}`, optional) starts one
    background job; a second request while one runs gets `409`.
  - `GET /api/admin/parts-sync` reports its state (`idle`/`running`/`succeeded`/`failed`),
    timestamps, report or error.
  - Only signed-in accounts listed in `ERGO_OPERATOR_IDS` (comma-separated user
    IDs) may call either, or the development account when Google sign-in is off.
  - A published revision is installed immediately (hot swap): new requests and
    `/api/config` see it, and clients pick it up on their next load. Earlier
    revisions stay valid in memory until restart.
  - On scale-to-zero hosting an idle instance may stop before a background job
    finishes; the CLI is the reliable path there.
- **Development:** `cargo run -p ergo-assets --example synthesize -- complete.zip`
  synthesizes from a local archive and prints the report.
- **Fixture:**
  - `--fixture LIBRARY OBJECTS` writes the committed test fixture: the reviewed
    tier plus `FIXTURE_DERIVED` sample parts, and their objects.
  - Its files are `crates/world/tests/fixtures/part-library.json` and
    `crates/assets/tests/fixtures/library-objects.cbor.gz`.
  - Tests and isolated stores publish it with `parts-publish`
    (`scripts/library_fixture.py`).
- **Verification:** the reviewed tier of the 2026-11 synthesis reproduces the
  vertex streams pinned by the previous reviewed lock for all 138 parts.

## Distribution

- **Server:**
  - serves `/api/library/{revision}/library.json` (immutable, gzip transport);
  - serves `/api/library/{revision}/reviewed.json`, the reviewed tier as its own
    verified document;
  - serves `/api/part-objects/{mesh_hash}`, content-addressed and public only
    for hashes the installed library names.
- **Browser:** the viewer installs the library from the IndexedDB cache or the
  network before loading any construction. See
  [loading and caching](loading-and-cache.md#part-library-document).
- **MCP:** mirrors the origin's library and objects into `.ergo-assets`, and keeps
  working offline from that verified mirror.
- **Static site:** the landing preview installs the reviewed tier shipped with
  its engine build.

## Reviews

`reviews.json` (`ergo-part-reviews-1`) holds the human-reviewed records.
Geometry, names, aliases and file pins always come from the official sources
during synthesis.

- **Adding a reviewed part:**
  1. Add its record: a sourced BrickLink mass and either a rectangular `size` or
     a source-LDU `nominal` profile, with optional `connectors`.
  2. Re-run the synthesis.
- **Refreshing masses:** `scripts/refresh-part-masses.py` prints current BrickLink
  masses for rectangular reviewed parts for manual review; it never overwrites them.
- **What a review does not supply:** new connector measurements or capacities.

### Scenic and Castle mould datums

The selected additions are `3470` oval tree, `3471` pine tree, `4444` wall panel,
`2345` corner panel, `3659`/`3307`/`3308a` arches, `4524` plastic cape,
`50231c01` formed cloth cape, `3896`/`89520` helmets, `4493c01`/`4493c02` factory
horses and `4491a` saddle. The corrected `3308a` source is used rather than the
obsolete `3308` geometry; it is not falsely declared an official identity alias.

Nominal profiles retain source datums: tree bases receive a 2×2 stud pattern at
8 LDU; the 4444 panel has stepped stud rows; arches receive studs only at their
two end feet; and horses have four separate hoof cavities. A nominal record can
add `additional_cavities` beside its first cavity. Matching accepts the union of
those reviewed grids, never a filled rectangle across an arch opening. Snapping
uses a real receiving datum and grounded registration checks every grid.

Factory horse back/body cores, saddle floors, helmet caps and cape neck rings are
nominal bearing proxies, with full genuine details retained visually. Plastic
cape collars are 2 LDU thick; the formed cloth source specifies a 0.6-LDU head
lift. Cloth remains a fixed formed source shape, not a cloth dynamics model.
No new retention capacities are supplied by these source records.

The 4444 panel uses its central 3×1 foot/header as its mass/collision core. Upright
static compression also recognizes the real stepped upper wing faces (source
Y=24), rear shelf (Y=8), and lower wing receivers (Y=120) alongside the central
foot (Y=144). These finite mould faces support actual joining bricks without
filling an arrow opening or inventing a retaining bond. Upright compound arches
and horses bear only on their declared feet/hooves, never their empty envelope
middle. Tree bearing cores use their 2×2 base/trunk footprint;
canopies remain genuine
visual overhangs rather than a fictitious solid column down to surrounding grass
studs. Optional `collision_boxes_ldu` preserve open space between horse legs and
under arches. Arch columns are conservatively derived from pinned source triangles;
existing parts retain their earlier collision interpretation. These are nominal
compound proxies, not exact curved-surface or material simulations.

`get_catalog` exposes `set_count` where sourced, and part-local connector endpoints
in public east/north/up brick units with `radius_m` in SI. See
[attachment behavior](part-catalog.md#hand-wheel-and-technic-attachments).

## Pre-assembled common constructs

`crates/world/library/constructs.json` defines immutable, versioned common models:

- `minifigure-v2`: four factory legs/torso/head/hair units. Feet register directly
  on the ground receiving grid; no display plate is added. Instance colour selects the torso.
- `classic-car-v2`: the attributed 6505 open-cabin adaptation, with instance-colour
  bodywork, fixed wheel/trim colours and transparent windscreen. Two genuine 4600
  holders seat four wheel/tyre units on revolute bearings. The 27 parts include
  internal axle spacers and a structural chassis plate; tyres alone touch ground.
  No road tile, pedestal or display plate is added. Its wheel bearings and tyres
  use the reviewed connector estimates.
- `medieval-guard-v3`: a ten-part Lion Knights guard with genuine **973p4f** printed
  torso, separately posed **3818/3819** arms and **3820** hands, **3844** helmet,
  **4497** spear and **3846p4h** shield. Shoulder elevation and wrist roll align
  the actual hands with an upright spear and a forward-facing shield handle.
  Feet touch ground directly; no display base or shield-stop tile is added.
  Instance colour selects the torso; blue arms, yellow hands and heraldry remain
  fixed. Joint-dependent publication needs measured capacities.
- `medieval-archer-v1`: nine source parts, with the same printed torso and posed
  arm/wrist interfaces, a genuine **4499 bow with arrow** held in the left hand,
  and a relaxed right arm. The grip lies beside the arrow crossing, not through
  the arrow. This is an upright ready/carry pose, not a drawn-bow simulation.
- `castle-standard-v1`: an eleven-part supported standard with genuine **4495a**
  wavy pennant and **3957a** antenna. One capped post bears the asymmetric cloth,
  avoiding an uncalibrated cantilever. Place the two post centres on half-stud
  coordinates over ordinary brick studs; the instance colour selects the flag.
  This does not certify an unsupported flag or any connector's shake strength.

Both Castle constructs retain the Stefan Frenz **6080-1** reference attribution
(CCAL 2.0); each genuine part also retains its own official LDraw author/licence.

`get_catalog` lists these IDs, descriptions, part counts and credits. A native
model references one with `{"target":{"kind":"construct","id":"classic-car-v2"}}`,
along with its ordinary transform and optional colour. The client/server expand
the shared definition for rendering, identity assignment and physical validation;
the model payload does not repeat its component placements. Unknown versions and
conflicting definitions are rejected. A changed construct gets a new version ID.
Only the five listed current IDs are shipped and resolved. The migrated obsolete
`minifigure-v1`, `classic-car-v1` and `medieval-guard-v1/v2` definitions are removed;
they are rejected rather than aliased, upgraded or cached. Regenerate/check current poses using
`python3 scripts/update-common-constructs.py --check`; hand datums come from the
same reviewed connector definitions as runtime inference.
The regeneration check tolerates only 1e-12 numeric differences from platform
`libm` rounding; it does not rewrite the stored immutable poses or relax metadata.
Repeated local definitions are interned both on append and on native save/export,
with independent placement and colour inheritance. Bottom-up interning also shares
duplicate nested definitions in existing migrated current-format models. Exact
current common prototypes become construct references; independently edited
geometry remains explicit. Depth-first leaf order, occurrence IDs, colours, steps,
groups and joints are preserved. No geometry matching merges physical occurrences.

## Existing constructions

The synthesized library keeps every reviewed canonical ID and its exact vertex
stream, so existing constructions need no geometry or ID rewrite. Moved, alias
and BrickLink numbers remain identity aliases for input normalization. A new
synthesis replaces the current library for every world; frozen world physics
models pin measurements and tolerances, not a library revision.

The earlier source-geometry/construct migration is complete; the `retrofit_library` tool
is retired. Current readers and new saves use the same canonical sourced parts
and current construct definitions. Save/export compaction is a derived encoding
change, not a rewrite of accepted history. Ordinary authoring still requires
permissions, authoritative validation, CAS and accepted readback.

### Accepted normalization checkpoint (2026-10-30)

Revision `3d97e56e19988497f951de55474f355522cd05e96ddc3c77e45ccd1b6345e549`
verified all **30 existing constructs / 102,629 pieces**: 23 accepted MCP retrofits
and seven already-canonical readbacks. Independent reads checked occurrence IDs,
colours, groups/joints, nominal world bounds and full canonical vertex fingerprints.
The live assembly headers shared **50 canonical part IDs with exactly one mesh CID
per ID**; no construct was removed. This is the library-normalization checkpoint,
before the separately journalled tower facade and final height/terrace publications.

After those geometry refinements, a second fresh inventory at revision
`3f185c572b1d8c3c534edb7ca3a4784bf517fa7e5bfd55b2be396de0d60808fd`
verified **30 constructs / 85,654 pieces**, zero legacy resources and the same fifty
canonical live part IDs with one mesh CID each. The complete district's accepted
geometry, 1,242-waypoint walking proof and source hashes are recorded in
[the final construction verification](model-recipes.md#final-accepted-district-and-canonical-world-verification-2026-10-30).

See [construction encoding](construction-format.md), [S3 resource formats](object-store-formats.md)
and [part descriptions and physical limits](part-catalog.md).
