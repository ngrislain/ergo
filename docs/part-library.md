# Canonical parts and common constructs

## One reproducible library

`crates/world/library/sources.lock.json` is the canonical catalogue used by Rust
and Python authoring. It contains 138 selected parts: 44 rectangular brick, plate
and tile shapes, plus 94 source-only architectural, figure, vehicle and Technic parts.
Names, dimensions, catalogue masses and aliases are read from this registry;
there are no independent standard-size or standard-mass tables in the generator.

The two authorities are:

- **Official LDraw:** part identity, geometry/dimensions, dependency files and identity aliases,
  from `https://library.ldraw.org/library/official` and its complete official archive.
- **BrickLink:** the catalogue mass records linked by each entry. Factory-unit
  component sums retain their individual source URLs and quantities.

Every selected part pins all its source files with SHA-256. Source text is UTF-8
without a BOM, with CRLF/CR normalized to LF; other text is preserved. A complete
official archive checksum records the input snapshot. Update builds verify local
files against these pins and use the same official URLs as remote files.
Changing a local directory, mirror or upstream file cannot silently replace a
pinned standard part. The full internal float32 vertex stream is also fingerprinted.

Official `~Moved to` chains consolidate identity aliases, including `3023.dat` to
`3023b.dat`, `970c00.dat` to `3815c01.dat`, and `4073.dat` to `6141.dat`. Each followed
alias must contain an identity placement; an origin-changing alias requires an
explicit pose migration. Similar bounding boxes alone are not identity evidence.
The rectangular selection has one entry per footprint, height and stud condition.
Aliases resolve to an existing entry rather than creating another mesh resource.

The library revision covers the source/metadata snapshot, baker format and vertex
fingerprint. It is distinct from an accepted world revision, mesh-object CID,
physical-model version or connector calibration.

## Compact native asset pack

`crates/assets/library/parts.pack` ships the complete selected geometry in **2,813,617
bytes**. It concatenates independent deterministic gzip members in canonical part-ID
order. Each member decodes to one ordinary native Mesh CBOR object; it is not a ZIP
or a top-level CBOR container. The lock records `asset:{offset,bytes,mesh_hash}` for
each part. The mesh hash covers decoded CBOR, while `vertex_hash` covers the complete
internal float32 stream with signed zero normalized, matching deterministic world CBOR.

Runtime resolution decompresses only the requested entry (at most 32 MiB), verifies
both hashes and identity, and populates the ordinary asset cache. No complete LDraw
download or primitive-by-primitive upstream fetching is needed to use these parts.
The pack and manifest are generated together from the two authorities; their
byte-for-byte regeneration is checked with `--check --bake`.

The browser proactively caches the complete current pack and construct manifest
through public, content-versioned `/api/library/{revision}/…` resources. See
[local basic-library caching](loading-and-cache.md#complete-current-basic-library).
Stale asset-cache records are discarded and resolved from the checked pack;
there is no runtime legacy-geometry upgrade path.

## Source updates

Regenerate and verify an existing official snapshot:

```sh
python3 scripts/update-part-library.py --source-dir /path/to/ldraw --check --bake
```

To take a new official snapshot and current linked BrickLink masses:

```sh
python3 scripts/update-part-library.py \
  --download-archive /private/operator-cache/complete.zip \
  --extract-to /private/operator-cache/official-update \
  --refresh-masses --bake
```

The archive comes from one fixed official HTTPS endpoint, is size-bounded and ZIP
checked, and extraction rejects traversal and links. Updates have no generated
timestamps: the same source files, catalogue values and baker produce identical
lock bytes. The generator follows identity aliases deterministically and fails on
conflicting selections or changed source classification. Changed dependencies
invalidate affected part revisions and asset descriptions. A reviewed baker change
can use `--bake --rebake` after updating its format identifier.

The output is a source change to test and release. It does not modify accepted
world geometry, discard historical snapshots or supply new connector measurements.
Part resolution rejects changed/unavailable canonical sources rather than creating
another flat-bottomed box variant. Parametric meshes remain internal diagnostics
and render-detail derivatives, not newly resolved catalogue assets.

To select an additional reviewed source-only part, first implement its nominal
profile, then use `--add-part LDRAW_ID.dat=BRICKLINK_ID --bake` with `--source-dir`.
The updater fetches that item's linked catalogue mass and records the complete
official dependency closure. Existing selections are rejected rather than
silently overwritten. Subsequent deterministic checks need no `--add-part` flag.

## Common-part selection and coverage

The 2026-11-03 expansion adds **63 parts**, each individually recorded by its linked
BrickLink item as appearing in **strictly more than seven sets**. The subsequent
tree/Castle/garrison selection adds 14 more individually sourced common pieces.
The full library has 137 such individual-item usage records; the original factory torso uses a
component mass sum and has no inferred whole-unit set count. Counts are metadata
snapshot values, not live statistics. Repeated colours and quantities are not set
counts. Existing reviewed selections remain available independently of popularity.

`scripts/common_parts.py` declares the reviewed candidate families: additional
rectangular plates/tiles, Technic bricks and straight beams, cross axles, friction
and frictionless pins, bushes, hand-held tools/bars, wheel holders, rims and tyres,
genuine oval/pine trees, Castle panels, arches, capes, helmets, factory horses and a saddle.
Its profiles and functional datums use the pinned official geometry. Standalone
tool/axle envelopes come from source triangles; genuine visual geometry is baked
normally. No generic box is substituted for a missing part.

```sh
python3 scripts/update-part-library.py --source-dir /path/to/ldraw \
  --add-common-parts --refresh-usage --bake
python3 scripts/check-common-parts.py
python3 scripts/check-common-parts.py inventory.csv --require-complete
python3 scripts/test-common-parts.py
```

The optional inventory CSV has `set_num,part` columns with official LDraw IDs.
The audit unions canonical aliases and counts distinct set IDs, ignores repeated
rows/colours/quantities in a set, and reports unavailable common IDs. It deliberately
does not guess mappings from another catalogue's part numbering. `--require-complete`
fails on gaps in the supplied corpus. The reviewed selection is **not an exhaustive
claim that every LEGO part used in eight sets is bundled**; a complete external
inventory is needed to measure that broader target. The default audit's universe
is explicitly the bundled items with recorded usage.

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
  No road tile, pedestal or display plate is added. Joint-dependent publication
  still requires measured connector capacities.
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

The source-geometry/construct migration is complete; the `retrofit_library` tool
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
