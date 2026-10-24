# S3 keys and file formats

This is the wire-format companion to [the bucket layout and migration record](object-store-layout.md).
It describes the current implementation, including compatibility formats that the
single hosted Sandbox no longer creates. S3 keys are prefixes, not directories.

## Namespaces, encodings and coordinates

Production uses bucket `ergo`, application prefix `ergo`, and world prefix `world`.
Thus a key written as `world/refs/main.cbor` below means
`s3://ergo/ergo/world/refs/main.cbor`. `_ops/ergo/production.env` is an exception:
it is at bucket root, outside the application prefix.

- `.json` files contain UTF-8 JSON; `.cbor` files contain CBOR, not compressed JSON.
- `CID` means the lowercase, 64-hex-character SHA-256 of the **stored bytes**.
  A CID is an integrity/content identifier, not authorization or an encryption key.
  An S3 ETag is an opaque conditional-write token, not a substitute for this hash.
- Transaction objects use `canonical_cbor` in `crates/transactions/src/codec.rs`:
  RFC 8949 core deterministic map ordering, finite floats and normalized negative
  zero. Asset/terrain caches use ordinary `ciborium` serialization. Do not assume
  re-encoding one with the other preserves its hash.
- Maps have field names unless otherwise noted; sequences/sets serialize as arrays.
  `BTreeMap<u32,...>` keys remain integers in CBOR. Optional values are null unless
  their Serde declaration explicitly omits them. Transaction paths are arrays of
  octants 0–7, at most 192 entries, not slash-separated S3 keys.
- Coordinate schema **2** means public/persisted X east, Y north, Z up. Positions
  use construction units; transforms are flat **column-major 3×3** matrices.
  Bounds are `{min:[x,y,z],max:[x,y,z]}`. The renderer's Y-up coordinates are not
  written directly. One block is 1024³ units; the metric conversion is 8, 8, 9.6 mm
  per public X, Y, Z unit. The separate source-model format uses LDU and -Y up.
- Transaction `format:3`, application metadata `layout:2`, physical model
  `algorithm:1`, and geometry `schema:2` are **different version domains**.

## Authoritative world: `world/`

### `refs/main.cbor` — mutable branch head

CBOR map `{format,commit,history}`. Current `format` is 3; `commit` is the accepted
Commit CID. `history` is `{root,retained_from}`: an optional acceptance-index root
CID and the oldest retained sequence number. Legacy heads may have no history.
Other branches, if explicitly created by library/operator tooling, use
`refs/<branch>.cbor`; the hosted application uses `main`.

Only the transaction library updates this key, using conditional create/ETag
compare-and-swap **after** its referenced immutable objects have been written.
Uploading an object alone does not make it an accepted edit.

### `maintenance.cbor` — mutable coordination state

CBOR map `{epoch,exclusive,pins}`: an unsigned epoch, optional exclusive maintenance
token, and array of durable reader/writer pin IDs. Conditional updates coordinate
readers, writers, migration and GC. Pins do not expire automatically. A crash can
leave a pin/barrier behind; it must not be removed while a former holder might
still publish. See `crates/transactions/src/maintenance.rs`.

### `objects/<CID>.cbor` — immutable mixed record store

Every transaction node, commit, application record and linked blob uses this
pattern. **The filename does not identify the record type.** Decode according to
the reference being followed, not by guessing that every CBOR object is a mesh.
Verify SHA-256 before decoding; follow the head, tree and schema links below.

#### Transaction records

- **Commit** (`transactions/src/lib.rs`): `{format,transaction_id,sequence,parent,
  base,root,edits,actor,message,metadata,blobs}`. Parent/base/root/metadata are
  optional CIDs; `blobs` is an array of additional retained CIDs. Each edit is
  `{path,generation,value}`; null `value` deletes that subtree. `root` selects the
  current tree; replaying all historical edits is not necessary to read it.
  `metadata` links the application audit. Library actor/message strings alone do
  not authenticate anyone; the REST layer supplies the verified identity.
- **Tree node** (`transactions/src/tree.rs`): a Serde externally tagged enum:
  `{"Branch":{"generation":null,"children":[...8 optional CIDs...]}}`,
  `{"Extension":{"path":[...octants...],"child":CID}}`,
  `{"Leaf":{"generation":text,"value":CID}}`, or
  `{"Deleted":{"generation":text}}`. Branch generations are optional replacement
  fences; a deleted node is a logical tombstone, not an object-deletion request.
- **Acceptance entry** (`transactions/src/history.rs`): `{revision,sequence}`.
  The head's history root indexes the same entry by CID and by sequence. The
  index is outside Commit to avoid a self-hash cycle.

#### Application tree addresses and leaf payloads

These are **logical octree paths inside the root**, not extra bucket folders.
The leading octant selects the record family (`shared/address.rs`). A block key
uses 64 interleaved bits from signed i64 coordinates biased by the sign bit, with
octant `x_bit + 2*y_bit + 4*z_bit`. An entity key uses the full 256-bit ID padded
with two leading zero bits and split into 86 octants.

- **`[0]`: Policy.** `{owner,grants,keep_last,authenticated_editing}`. A grant is
  `{user,regions}`; regions are octree prefixes. `keep_last` is an optional retained
  revision count. This is the authorization policy, not a client visibility flag.
- **`[2]`: Metadata.** `{id?,name,geo,model,layout}`. `geo` is
  `{latitude,longitude,arcseconds_per_block}`; `model` links the frozen physical
  model. Current layout is 2; absent layout is read as legacy layout 1. There is no
  world-sized piece list or global piece counter in this record.
- **`[3] + entity-ID`: Piece.** `{id,assembly,local_id,geometry,mesh,replicas}`.
  `id` is the stable Birth CID, **not the CID of this mutable-state payload**.
  `geometry` is `{part,color,rgb?,position,transform,step}`; `mesh` is the mesh CID.
  `replicas` lists public block coordinates containing spatial references. Moving
  a piece changes this payload's content hash while keeping its birth ID.
- **`[1] + block-address`: spatial references.** Current layout 2 uses adaptive
  local nodes: child `0` holds own references keyed by full piece ID; child `1/q`
  descends to octant `q`, up to ten local levels. A **SpatialRef** is
  `{coord,piece,bounds}`. Bounds are full render bounds; one reference is placed
  per piece/intersected storage block, not in every fine cell it crosses.
  Legacy layout 1 instead stores **SpatialBlock** `{coord,pieces}` as one leaf.
- **`[5] + block-address`: allocated-block directory.** **BlockHeader** is
  `{coord,paged}`. This is a sparse directory for bounded enumeration, not a
  preallocated world grid. Current `paged` is true.
- **`[4] + assembly-ID`: paged assembly rules.** Child `0` stores **RulesHeader**
  `{id,name,meshes,paged,source_model?}`; `meshes` maps part filenames to mesh CIDs.
  Child `1` has membership rows `{local_id,piece}`. Child `2` has peer-CID adjacency
  rows, child `3` endpoint-bound joints `{a,b,spec}`, child `4` boolean ground-anchor
  markers, child `5` group headers/membership, and child `6` authored joint specs.
  Local numeric row keys use eleven octants; endpoint/group suffixes are defined
  in `shared/paged.rs`. Group headers are `{id,name,pivot_world}`; members are the
  tagged piece/group records from `world/src/groups.rs`.
  Legacy monolithic **Rules** is `{id,name,pieces,meshes,joints,fixed_bricks,
  groups,source_model?}`. The reader reconstructs equivalent bounded authoring
  rules from current rows; ordinary edits need not load every assembly member.
- **`[6] + joint-ID`: cross-assembly GlobalJoint.** `{id,a,b,spec}`; `a` and `b`
  are stable global piece IDs. **`[7] + piece-ID`** contains incident joint-ID rows.
  A joint spec carries local endpoints, public anchor and fixed/revolute kind,
  optional force/torque thresholds and their measurement source. The global
  endpoints bind the spec to actual pieces (`shared/joints.rs`).

#### Blobs linked by those records

- **Assembly birth:** CBOR array `["assembly-birth-v1",nonce,actor,name]`. Its
  hash identifies the assembly independently of later names or geometry.
- **Piece Birth:** `{nonce,created_by,created_by_name?,assembly,initial,mesh}`.
  `initial` has the same Geometry fields as Piece. Author names are server-resolved
  display snapshots; permissions use stable actor IDs. This is attribution, not a
  user's personal digital signature.
- **Joint birth:** `["joint-birth-v1",nonce,actor,input]`, where input has global
  `a`, `b`, public anchor and tagged kind. Its hash remains the joint identity.
- **PhysicalModel:** `{algorithm,measurements,tolerances}`. `measurements` has
  connector `entries` and optional per-part `parts`; tolerances include position,
  rotation, force and torque thresholds. These are frozen server-owned records.
  Their detailed SI fields are in [physical calibration](physical-calibration.md).
- **Audit:** `{actor,actor_name?,base,policy_hash,model,changed_pieces,
  affected_pieces,watched_regions,validation,loaded_pieces,loaded_cbor_bytes}`.
  It links policy/model/birth records and records the actual bounded validation
  domain; validation may be null for non-geometric operations.
- **Source Model:** `{root,subparts}` from `world/src/assembly.rs`. Each subpart
  is an array of `{file,color,matrix}` references. Color 16 inherits; `matrix` is
  **row-major 4×4**, column-vector, affine **LDU/-Y-up**, with last row `[0,0,0,1]`.
  This is deliberately different from the 3×3 public piece transform. Live Piece
  records are the accepted geometry authority; source hierarchy is authoring
  provenance, not permission to resurrect old poses. See [LDraw storage](ldraw-storage.md).
- **Mesh:** the geometry schema described below. World meshes live in the world
  object namespace even when the part asset cache also contains that geometry.

GC must trace these schema-defined links as well as tree/history links.
`shared/retention.rs::blob_links` implements the application tracer. A hash-looking
string in a free-form comment is not automatically a reference.

## Shared part cache: `assets/` and `objects/`

### `assets/ldraw/source/<normalized-name>.json`

UTF-8 JSON `{text,url}`: complete LDraw source text and provenance location.
Normalized names may contain safe subpaths such as `s/` or `48/`. File headers
supply author/license credits; `url` can identify a local library path. These are
on-demand cache entries, not an assertion that the entire Studio/BrickLink library
has been uploaded. They currently have no separate schema field or refresh TTL.

### `assets/v2/parts/<part>.json`

UTF-8 JSON `{schema,part,mesh_hash,credits}`. Schema is 2, `part` is the canonical
LDraw filename (including `.dat`), and `mesh_hash` selects
**`objects/<mesh_hash>.cbor` in the application namespace**, not `world/objects/`.
Each credit is `{file,source_url,author,license}`. Tile aliases such as `3068.dat`
canonicalize to `3068b.dat` before this lookup.

### `objects/<mesh_hash>.cbor` — cached Mesh

CBOR map `{schema,part,quality,vertices,bounds,body_bounds,credits,physical?}` from
`world/src/geometry.rs`. `vertices` is an array of floats, **not packed binary**:
each vertex has nine values `position.xyz, normal.xyz, color.rgb`; three vertices
form a triangle. Positions/normals are public Z-up on the wire. Negative face
colors inherit the instance color. Bounds include visible studs; nominal body
bounds exclude decorative studs for known catalog bricks. Optional `physical`
contains a sourced part measurement, not a measurement inferred from triangle volume.

`quality` distinguishes `ldraw`, `parametric`, `proxy`, `body_lod`, and generated
ground geometry where applicable. The persistent part cache writes baked LDraw
meshes; fallback results can be returned without becoming a successful baked
cache index. Render LODs/instance lists are currently derived in the client;
**there is no persisted multi-LOD asset format or bulk-library completeness guarantee**.

The resolver writes the mesh before the index. Both cache layers are active;
deleting `objects/` breaks retained part indexes. These meshes are not covered
by the world repository's reachability GC.

## Terrain cache: `ground/`

### `ground/source/<provider-hash>/<z>/<x>/<y>.cbor`

CBOR **Tile** `{fetched_at,etag,modified,png}`. The provider hash is SHA-256 of the
URL template, and z/x/y are Web Mercator tile coordinates. `fetched_at` is Unix
seconds; ETag/Last-Modified values support conditional HTTP refresh.
Despite its historical name, `png` is a CBOR **byte string containing the original
encoded PNG or JPEG image**. It is neither base64 nor decoded RGB pixels. Source
tiles normally remain fresh for seven days. See `assets/src/ground.rs`.

### `ground/blocks/<signature>/<east>_<north>.cbor`

CBOR **GroundBlock** `{schema,coord,bricks,color,sample_rgb,source,attribution,
attribution_url,generated_at,source_tiles,fallback_reason}`. Schema is 2; public
`coord` is `[east,north,0]`; `bricks` is empty. One palette `color` covers the whole
virtual baseplate; `sample_rgb` is an optional 24-bit RGB integer. Times are Unix
seconds. Source tile descriptions/attribution explain where the sampled color
came from; fallback_reason is null on success.

The signature hashes the rounded geographic origin/resolution, provider/template,
attribution, enabled state, palette and `ground-average-v3` algorithm identifier.
These filenames are **configuration keys**, not hashes of the CBOR payload.
Generated cells normally have a seven-day TTL; fallback cells retry after 60 s.
Legacy records with a `colors` grid are read and averaged to one palette color.
Terrain caches do not contain authored construction pieces or accepted history.

## Private account records

### `users/<stable-user-id>.json`

UTF-8 JSON User `{id,name,email,picture,created_at,last_login}`. Picture is an
optional URL; times are Unix seconds. The profile supports sign-in and
server-resolved display names. It is private, unlike a name copied into public
birth/audit attribution. Source: `server/src/auth.rs` and `store.rs`.

### `edit-pacing/<actor-hash>.json`

UTF-8 JSON Ledger `{credits,updated,bricks,seconds,reservations}`. The filename is
SHA-256 of the actor ID, not a construction hash. Credits are integer
brick-change × seconds units: available changes are `credits/seconds`; refill adds
`bricks` credits per elapsed second, capped at `bricks*seconds`. Reservations map
unique receipt IDs to `{bricks,at}`. Conditional writes make concurrent charging
and reservation refunds safe; this is operational state, not accepted world history.

### `account-plans/<actor-hash>.json`

UTF-8 JSON `{unlimited:boolean}`. An absent file means false. Only the operator
sets entitlement; it bypasses pacing, not authentication or physical admission.
This key family is supported even when no such record exists in an inventory.
Both pacing/plan formats are defined in `shared/pacing.rs`.

## Operator backup: `_ops/ergo/production.env`

Private **plaintext dotenv**, not JSON/CBOR and not an application database.
It is a recovery copy of deployment configuration and may contain credentials
and signing secrets. Keep its body/access private. It is outside world GC and
is not loaded automatically as an application scene. The application has no
separate S3 session-cookie, OAuth-token or downloaded MCP-token record family;
those flows use signed tokens/local downloads, not a token database in the bucket.

## Legacy and isolated-development formats

Production cleanup retired these namespaces. Compatibility code and isolated
fixtures can still use them; their existence in source is not a supported private
world workflow on the hosted service.

- `shared/<world-UUID>/{refs,objects,maintenance...}`: the same transaction/file
  formats above under one repository per world. The hosted repository is now
  `world/`, with one public identity.
- `shared/_catalog/...`: transaction repositories for user/quad discovery.
  Advertisement leaf `{world,coord}` references a world and optional block.
  Single-world configuration skips writing these derived advertisements.
- `worlds/<owner>/<UUID>/manifest.json`: UTF-8 **WorldManifest** containing
  `{schema,id,name,source,brick_count,bounds,geo,blocks,geometry,proxy_parts,notes,
  joints,fixed_bricks,validation,owner,ground,groups,model?}`. Blocks have
  `{coord,key,brick_count}`; geometry maps part IDs to application-object mesh CIDs.
  Block objects in `objects/` are `{schema,coord,bricks}` with full serialized
  Brick records (IDs, part/color/RGB, pose, body/render bounds, step), replicated
  across intersected cells and deduplicated on read.
- `_private_catalog/<owner-hash>/...`: the same generic transaction repository
  format, with Summary leaf `{id,name,owner,brick_count,bounds,geo}`.
- Historical transaction test/benchmark prefixes contain generic transaction
  heads/nodes/blobs, not application world roots. No active production data is
  inferred solely from those names.

Local `.ergo.json`/MPD working copies, `.shared.json` client journals, WASM
bundles and generated PNGs are **not** automatically S3 objects. The canonical
construction-file format is documented separately in [LDraw storage](ldraw-storage.md).

## Read/retention rules

To inspect a construction, start at the mutable head, verify/decode its immutable
Commit, then traverse typed tree leaves and mesh/source-model links. Public read
APIs expose authorized application data; they do not grant anonymous bucket access.
Keep the backup, profiles, account records and active cache roots separate from
world history. Delete world objects only through the coordinated, schema-aware
collector; filename age, ETag, or absence from the current camera view proves
neither obsolescence nor unreachability.
