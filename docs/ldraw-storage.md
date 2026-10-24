# LDraw-aligned construction storage

Construction downloads and MCP working files use `format: "ergo-ldraw-1"`
with coordinate metadata `schema: 2`. Old schema-2 files containing `bricks`
remain readable. New writers emit an authoritative `model`, not a second saved
copy of every flattened brick pose. The schema identifies Ergo's geographic,
joint, group, mesh and ground coordinates; **model matrices use LDraw coordinates**.

## Definitions and occurrences

`model.root` names a definition in `model.subparts`. Each definition is a list
of references with `file`, `color`, and `matrix`. A reference names either another
definition or a `.dat` mesh from the envelope's `meshes` map. Definitions can be
nested and reused any number of times within the expansion limits. Names use
normalized lower-case relative paths; absolute paths, traversal and control
characters are rejected. Resolving this graph performs no filesystem/network I/O.

For example, this is the `model` field of a construction (other envelope fields
and the `3003.dat` mesh are omitted):

```json
{
  "root": "main.ldr",
  "subparts": {
    "main.ldr": [
      {"file": "pillar.ldr", "color": 4,
       "matrix": [[1,0,0,0],[0,1,0,0],[0,0,1,0],[0,0,0,1]]},
      {"file": "pillar.ldr", "color": 1,
       "matrix": [[0,0,1,100],[0,1,0,0],[-1,0,0,40],[0,0,0,1]]}
    ],
    "pillar.ldr": [
      {"file": "3003.dat", "color": 16,
       "matrix": [[1,0,0,20],[0,1,0,-24],[0,0,1,0],[0,0,0,1]]}
    ]
  }
}
```

The pillar is defined once and instantiated twice: red, then blue with a quarter
turn and a translation. Color **16 inherits** the referencing occurrence's color;
24 is an edge color and is rejected for references. Missing `color` defaults to
16; missing `matrix` defaults to identity. Use a concrete color at the top level
for actual brick placement.

## Homogeneous transforms

Following [LDraw type 1](https://www.ldraw.org/article/218.html#lt1), arrays are
**four rows**, multiplying column vectors:

$$
T=\begin{pmatrix}a&b&c&x\\d&e&f&y\\g&h&i&z\\0&0&0&1\end{pmatrix},
\qquad p'=Tp,\qquad T_{world}=T_{parent}T_{child}.
$$

The equivalent text reference is:

```text
1 color x y z a b c d e f g h i filename
```

These translations are **LDraw units**, not Ergo construction units: 20 LDU per
stud, 24 per brick height, 8 per plate; -Y is up. The existing conversion to
the internal X-east/Y-up/Z-south kernel is
`C = diag(1/20, -1/24, -1/20)`, so `p_kernel = C p_ldraw` and
`A_kernel = C A_ldraw C⁻¹`. Public east/north/up coordinates remain unchanged
outside this explicitly identified model field.

Matrices must be finite, nonsingular and affine, with last row `[0,0,0,1]`.
The graph representation supports rotation, reflection, scaling and shear as
LDraw does. File imports preserve these affine transforms for viewing and export,
with an explicit note for non-rigid occurrences. **Simulation and publication
still require real, rigid brick dimensions**; affine draft import does not
weaken either check or silently round/deform imported matrices. MCP placement
also retains its rigid-pose/floor/collision checks. A valid matrix or renderable
model is not a physical certificate.

## Ergo metadata and compatibility

The envelope retains `name`, `geo`, `meshes`, `joints`, `fixed_bricks`, `ground`
and authoring `groups`. Its `pieces` array stores `id`, explicit `rgb`, and `step`
in depth-first expanded leaf order. Thus two instances of a template have distinct
identities and can participate in different joints/groups. If `pieces` is omitted
or empty, IDs start at 1 and RGB comes from each resolved color; otherwise its
length must equal the leaf count and IDs must be positive and unique.

WASM/REST messages still materialize `WorldExport.bricks` for editing, validation
and streaming. The same scene reader accepts canonical construction files, so a
download can be submitted to scene-based REST endpoints. Ambiguous documents with
both an explicit construction format and `bricks`, or an unknown format, fail.

LDraw/Studio imports retain their reference hierarchy in private snapshot
manifests, and unchanged downloads preserve it. Geographic import placement wraps
the root in a translated reference rather than discarding nested definitions.
Independent leaf edits can make
that hierarchy stale. The writer checks the complete leaf poses/colors and, if
they no longer match, writes a fresh root referencing the edited leaf parts.
It never restores old template poses over edits. Automatic copy-on-write
submodel editing/re-deduplication is not implemented.

Spatial CBOR blocks and shared-world live piece records remain **materialized
indexes**, with shared immutable part meshes. Shared imports also store an
immutable import-time `source_model` blob referenced by their assembly header;
retention/GC trace that link. It is provenance, not the authority for subsequent
shared piece edits. A bounded shared navigation region is not a full-model export.

`Model::to_mpd()` emits standard CRLF `0 FILE` sections and type-1 references,
with the root first. It exports the assembly, not embedded triangle meshes or
Ergo-only physics/geographic metadata. External `.dat` references need the
corresponding LDraw library. This is not full support for every LDraw META,
TEXMAP, conditional-line or material extension.

## MCP placement and bounds

`place_submodel` accepts inline `ldraw` text (LDR/MPD), optional `matrix` in the
row-major/LDU convention above, and optional inherited `color`. It namespaces
embedded definition names before appending the new occurrence to the working
model. Geometry must already be resolved, be a known parametric part, or be
embedded in the MPD. Use `get_catalog(part=...)` first for an external part.

Placement is atomic under the existing editing checks and does not weld a
submodel into one rigid body. Limits: 128 KiB inline tool text, 5,000 local
working bricks, 200,000 stored references/definitions and expanded leaves,
64 nesting levels, and one million expansion visits (including empty models).
Cycles, missing submodels/meshes, excessive expansion and malformed matrices fail.
Full uploaded files retain the server's existing upload/geometry budgets.

Implementation: `crates/world/src/{assembly,storage,ldraw}.rs`,
`crates/client/src/runtime.rs`, `crates/mcp/src/main.rs`, and
`crates/server/src/{import,store}.rs` and `crates/server/src/shared/`. Tests cover composition, reusable
`.dat` subparts, inheritance, JSON/CBOR and MPD round trips, legacy files,
edited-pose preservation, invalid graphs/matrices and actual MCP rollback.
