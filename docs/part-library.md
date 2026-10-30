# Canonical parts and common constructs

## One reproducible library

`crates/world/library/sources.lock.json` is the canonical catalogue used by Rust
and Python authoring. It contains 55 selected parts: 32 rectangular brick, plate
and tile shapes, plus 23 source-only architectural, figure and vehicle parts.
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

`crates/assets/library/parts.pack` ships the complete selected geometry in **982,682
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

## Pre-assembled common constructs

`crates/world/library/constructs.json` defines immutable, versioned common models:

- `minifigure-v1`: factory legs, torso, classic-grin head and genuine 3901 hair on
  a plate; the instance colour selects the torso.
- `classic-car-v1`: the attributed 6505 open-cabin adaptation, with instance-colour
  bodywork, fixed wheel/trim colours and transparent windscreen. Its tyres rest on
  a supported smooth road/tiled surface; the tyre proxy is not an upright ground-stud footprint.

`get_catalog` lists these IDs, descriptions, part counts and credits. A native
model references one with `{"target":{"kind":"construct","id":"classic-car-v1"}}`,
along with its ordinary transform and optional colour. The client/server expand
the shared definition for rendering, identity assignment and physical validation;
the model payload does not repeat its component placements. Unknown versions and
conflicting definitions are rejected. A changed construct gets a new version ID.

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

See [construction encoding](construction-format.md), [S3 resource formats](object-store-formats.md)
and [part descriptions and physical limits](part-catalog.md).
