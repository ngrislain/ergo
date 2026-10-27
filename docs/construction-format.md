# Ergo construction format

Construction downloads and MCP working files use **`ergo-construction-1`**.
Reusable model descriptions use **`ergo-model-1`**, including when stored as linked
CBOR blobs in the shared world. The version identifies Ergo's own layout.
Coordinate `schema: 2` is a separate version domain.

## Model definitions

A model has `format`, `schema`, `root` and a `definitions` map. Each definition is
an ordered array of instances. An instance contains:

- `target`: `{kind: "part" | "definition", id: string}`. A part ID identifies an
  Ergo catalog description; a definition ID identifies another definition.
- `transform`: a finite row-major 4×4 affine matrix, using column vectors.
- Optional `color`: a palette/custom color. Omit it to inherit the enclosing
  instance color; no external-format inheritance code is stored in this field.

The transform maps local part/definition coordinates into its parent. Composition
is parent × child. Translation uses public X east, Y north, Z up construction
units. A basic 2×2 brick's reference is its top center, so this model stands on Z=0:

```json
{
  "format": "ergo-model-1",
  "schema": 2,
  "root": "main",
  "definitions": {
    "main": [
      {
        "target": {"kind": "part", "id": "3003.dat"},
        "color": 4,
        "transform": [
          [1, 0, 0, 1],
          [0, 1, 0, 1],
          [0, 0, 1, 1],
          [0, 0, 0, 1]
        ]
      }
    ]
  }
}
```

Definition names are identifiers, not instructions to open files. Part/definition
target mismatches, cycles, missing targets, singular/nonfinite transforms and
invalid paths are rejected. Expansion is bounded to 200,000 references/leaves,
64 nested levels and one million expansion visits.

## Physical transforms

Horizontal construction units are 8 mm; vertical units are 9.6 mm. For metric
rotation R and unit conversion D, the stored linear transform is:

$$
D=\operatorname{diag}(0.008,0.008,0.0096),\qquad A=D^{-1}RD.
$$

Thus a physically rigid rotation need not be orthonormal in differently scaled
brick coordinates. Physical poses require a proper metric isometry: R is
orthonormal and det(R)=+1. Scale, shear and reflections are rejected. Authoring
and publication check every model instance, including unused definitions, so
opposing scales in a hierarchy cannot cancel to bypass the rule. Visual import
inspection and historical reads may retain affine source data; that does not
authorize physical editing, simulation or publication of those transforms.

## Construction envelope

The JSON envelope has:

- `format: "ergo-construction-1"`, `schema: 2`, `name` and geographic `geo` reference.
- `model`: the native model above.
- `pieces`: depth-first occurrence metadata `{id,rgb,step}`. IDs identify
  occurrences, not reusable templates. A supplied array must match expansion;
  an empty array assigns sequential local IDs/default colors.
- `meshes`: resolved Ergo geometry keyed by part ID.
- `joints`, `fixed_bricks`, `groups` and generated `ground` metadata.

World/global birth IDs are distinct from these bounded local numeric IDs.
Current accepted Piece records remain the authority for live poses. If independent
edits no longer match a reusable source hierarchy, export rebuilds a safe flat
definition rather than restoring old template poses.

Runtime/REST messages use materialized `WorldExport` records with `bricks` for
streaming and job execution. That transport is not an alternate construction-file
format. File readers require the explicit native envelope and reject old leaf
files and unknown formats.

## Imports and MCP

`.io`, `.ldr` and `.mpd` inputs are converted by import adapters. Imported source
units/axes do not define the Ergo storage format. Source IDs, authors and license
credits are retained as provenance.

`place_submodel` accepts `model` for a native description or `document` for an
inline import, exactly one at a time. `transform` is always an Ergo world-space
4×4 matrix. Native inline models are bounded to 768 KiB; imported text to 128 KiB.
Local editing has no 5,000-piece cap; dynamics simulations remain limited to
5,000 bodies. Server admission uses the caller's plan: god-mode authoring has no
brick-count cap, while encoded-byte/request budgets and physical checks apply.

Placement snaps the incoming model with one translation of its nominal bounding
corner (one stud horizontally, one plate vertically). Internal geometry is not
individually rounded. Grounded nominal footprints must register on the baseplate
grid at commit; see [ground and coordinates](ground-and-coordinates.md).

Implementation: `crates/world/src/assembly.rs`, `assembly/format.rs`, `storage.rs`,
`coordinates.rs`; import conversion is separate from these native wire types.

## Standard LDraw/MPD export

`crates/world/src/exchange.rs` exports ordinary UTF-8, CRLF-terminated MPD using
standard type-1 references and type-3 part triangles. Coordinates use LDraw's
20-LDU stud pitch, 24-LDU brick height and negative-Y up. The construction is
centred horizontally, retaining its original ground plane. Relative poses,
standard colour IDs and custom RGB colours are preserved; custom RGB uses the
standard `0x2RRGGBB` notation.

Every referenced resolved part is embedded once, including custom/imported
geometry. Source authors, URLs, licences and geometry-quality labels remain
comments. Embedded resolved meshes are marked unofficial, not certified official
LDraw parts. Corrected winding is declared with BFC. Consumers do not need an
Ergo asset cache or a separate part-library installation to resolve the file.
If an assembly contains different resolved meshes for one catalogue ID, export
gives later variants distinct subfile names and records the original ID in their
quality comments. Geometry and occurrence IDs are retained without pretending
incompatible meshes are one part definition; reimport still needs normal physical
validation for such visual variants.

Select an assembly and choose **Download construction → LDraw / MPD**, or GET
`/api/shared-worlds/{id}/assemblies/{assembly}/export?format=ldraw` (optional
`revision=CID`). `format=ergo` exports the complete native envelope. Both apply
normal world-read permissions and bounded complete-assembly loading; neither
mistakes a clipped browser region for the selected construction. The response
is an attachment and identifies its accepted revision in `X-Ergo-Revision`.
Local workspaces also have **Export workspace MPD**.
Read-only construction downloads do not inherit the 5,000-body dynamics or
ordinary authoring count cap. The complete selected assembly is read under the
encoded-byte budget, without traversing mechanical neighbors or claiming a
standalone physical certificate.

MPD is visual interchange: Ergo identities, geographic placement, groups, frozen
measurements and physical constraints remain in native Ergo files. Reimported
geometry must pass normal validation before publication; export is not a physical
certificate. Proprietary Studio `.io` writing is not implemented.

References: [LDraw file format](https://www.ldraw.org/article/218.html),
[MPD extension](https://www.ldraw.org/article/47.html), and
[BFC extension](https://www.ldraw.org/article/415.html).
