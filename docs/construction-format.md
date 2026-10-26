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
brick coordinates. Visual affine imports retain supplied scale/shear, but physical
simulation and publication require nominal metric dimensions.

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

Implementation: `crates/world/src/assembly.rs`, `assembly/format.rs`, `storage.rs`,
`coordinates.rs`; import conversion is separate from these native wire types.
