# Canonical parts and common constructs

## One reproducible library

`crates/world/library/sources.lock.json` is the canonical catalogue used by Rust
and Python authoring. It contains 124 selected parts: 44 rectangular brick, plate
and tile shapes, plus 80 source-only architectural, figure, vehicle and Technic parts.
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

`crates/assets/library/parts.pack` ships the complete selected geometry in **2,278,268
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
BrickLink item as appearing in **strictly more than seven sets**. The full library
has 123 such individual-item usage records; the original factory torso uses a
component mass sum and has no inferred whole-unit set count. Counts are metadata
snapshot values, not live statistics. Repeated colours and quantities are not set
counts. Existing reviewed selections remain available independently of popularity.

`scripts/common_parts.py` declares the reviewed candidate families: additional
rectangular plates/tiles, Technic bricks and straight beams, cross axles, friction
and frictionless pins, bushes, hand-held tools/bars, wheel holders, rims and tyres.
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
Old v1/v2 IDs retain immutable definitions. The catalogue recommends the new
guard/archer poses and plate-free v2 people/cars. Regenerate/check poses using
`python3 scripts/update-common-constructs.py --check`; hand datums come from the
same reviewed connector definitions as runtime inference.
The regeneration check tolerates only 1e-12 numeric differences from platform
`libm` rounding; it does not rewrite the stored immutable poses or relax metadata.
The castle repair helper
`scripts/repair-castle-garrison.py` prepares an explicitly local preview, matches
old components by exact occurrence pose, retains architecture/flag identities,
and uses elevated guard slots for archers. It never treats a failed static
certificate or local preview as an accepted live edit.

Repeated local definitions are also interned on append, with independent placement
and colour inheritance. Matching existing prototypes can become library references;
independently edited geometry remains explicit. In the native model regression,
100 cars use 15,661 JSON bytes versus 317,223 bytes for flat part references.
This measures the model section, not complete scene/mesh traffic or S3 request count.

## Existing constructions

`retrofit_library` resolves canonical source parts and performs one normal local/
shared edit. It retains piece IDs, groups, joints and world-space nominal body
bounds, drops unused duplicate meshes and replaces matching local prototypes with
common-construct references. Unsupported parts or changed nominal dimensions reject
the whole edit for an explicit structural retrofit or removal. Shared publication
still uses ordinary permissions, authoritative validation, CAS and accepted readback.

An inventory or local JSON rewrite is not evidence that the live world is converted.
Record actual accepted receipts and a fresh complete inventory for migration results.
Historical accepted roots keep their frozen resources; retention-aware collection
is separate from replacing current legacy usage.

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
