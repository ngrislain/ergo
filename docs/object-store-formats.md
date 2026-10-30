# S3 keys and current file formats

Paths below are relative to `s3://ergo/ergo/`, except `_ops/ergo/` at bucket root.
The layout is summarized in [object-store organization](object-store-layout.md).
Only current persisted formats are supported by the application.

## Encoding and version domains

- `.json`: UTF-8 JSON. `.cbor`: CBOR, not compressed JSON.
- `CID`: lowercase 64-hex SHA-256 of the **stored bytes**. A CID is not an access
  grant. S3 ETags are opaque conditional-write tokens, not content hashes.
- Transaction objects use RFC 8949 core deterministic CBOR via
  `crates/transactions/src/codec.rs`: bytewise encoded-key ordering, finite floats
  and normalized negative zero. Cache CBOR may use ordinary `ciborium` encoding;
  hashing always uses the actual stored bytes.
- Public coordinate `schema: 2` is east/north/up construction space. X/Y units are
  8 mm and Z units are 9.6 mm. Bounds are `{min:[x,y,z],max:[x,y,z]}`.
- Transaction `format: 3`, spatial layouts `2` (legacy) and `3` (packed reads), and the named Ergo model/part/
  construction formats are independent version domains.
- Logical octree paths are arrays of octants 0–7 (maximum depth 192), not S3 keys.
  Sets/ordered sequences encode as arrays; CBOR integer map keys remain integers.
- S3 object metadata is not a decoder: writers do not uniformly set Content-Type
  or Content-Encoding. Select the type from the key and the typed reference/schema.
  Plain `.cbor` resources below are uncompressed unless an envelope explicitly
  contains gzip bytes. HTTP JSON/CBOR responses and HTTP gzip are separate transport
  choices; a `.cbor` object is not implicitly a gzip stream or base64 document.

## Resource references: parts, constructs and occurrences

The [canonical library](part-library.md) is compiled from checked-in UTF-8 JSON:
`sources.lock.json` pins official LDraw geometry/dependencies and BrickLink catalogue
metadata; `constructs.json` defines immutable common models. It is shipped with the
client/server, not copied into each S3 construction as another set of part meshes.

- A standard **part ID** such as `3003.dat` selects one canonical description.
  Official identity aliases resolve to that record. A **library revision** pins
  its source/metadata, baker and full vertex fingerprint; it is not a world CID.
- A **common construct ID** such as `classic-car-v1` selects reusable component
  placements. A native model stores `{kind:"construct",id:"classic-car-v1"}` plus
  transform/colour, rather than embedding the car's full definition per copy.
- A placed **occurrence** has an independent local ID and global Birth CID.
  Materialized Piece records and spatial references still track its component
  bricks for permissions, editing and physical validation. A prefab reference
  does not collapse a minifigure or car into one unvalidated rigid body.
- Mesh vertices are referenced by immutable **mesh CIDs**. Source/prototype
  sharing and per-occurrence identity are separate; incompatible historical
  resources must not be merged just because their filenames or boxes are similar.

The lock uses sorted-key, two-space-indented UTF-8 JSON with one final LF for its
revision inputs. `source_revision` hashes `{catalogue,files}`; `revision` hashes
`{baker,source_revision,vertex_hash,mesh_hash}`. Source-file hashes use the documented UTF-8/LF
normalization. `vertex_hash` hashes every internal float32 component in little-endian
order, normalizing signed zero before the public-axis wire conversion. Mesh CIDs
instead hash stored CBOR. Distribution `parts.pack` is a concatenation of per-part
gzip-CBOR members indexed by `{offset,bytes,mesh_hash}` in the lock; it is not a new
accepted-world object family. The runtime expands a requested entry into the ordinary
native part cache described below.

## `world/refs/main.cbor`: Head

CBOR `{format:3,commit:CID,history:{root:CID|null,retained_from:u64}}`.
The acceptance index is required. Its root indexes `{revision,sequence}` records
by both revision CID and sequence. The current commit is selected with a
conditional head PUT after every referenced immutable object has been staged.

## `world/maintenance.cbor`: Control

CBOR `{epoch:u64,exclusive:string|null,pins:[string...]}`. Conditional updates
coordinate durable non-expiring reader/writer pins and exclusive maintenance.
Cancelled/uncertain operations remain fenced; only explicit, settled operator
recovery removes abandoned pins/barriers.

## `world/objects/<CID>.cbor`: typed immutable objects

The filename does not identify a record type. Verify its hash and decode according
to the reference being followed. The current object families are:

### Transaction layer

- **Commit:** `{format:3,transaction_id,sequence,parent,base,root,edits,actor,
  message,metadata,blobs}`. Optional CIDs are null. An edit is
  `{path,generation,value}`; null value removes a subtree. `root` is materialized
  accepted state; viewing needs no full-history replay. `metadata` links the audit.
- **Node:** externally tagged `Branch {generation,children:[8 optional CIDs]}`,
  `Extension {path,child}`, `Leaf {generation,value}`, or `Deleted {generation}`.
  Generations are replacement fences. Deleted is a logical tombstone.
- **RevisionEntry:** `{revision:CID,sequence:u64}`. The head's separate acceptance
  index avoids a Commit self-hash cycle.

### Application tree paths and records

The leading octant selects a family. Signed block addresses use 64 sign-biased,
interleaved coordinate bits with octant `x_bit + 2*y_bit + 4*z_bit`. Entity IDs use
all 256 hash bits, padded by two bits and split into 86 octants.

- **`[0]` Policy:** `{owner,grants,keep_last,authenticated_editing}`. Grants contain
  `{user,regions}`. Retention is optional; permissions come from this policy and
  authenticated server identity, not free-form author strings.
- **`[2]` Metadata:** `{id?,name,geo,model,layout:2|3}`. Geo is
  `{latitude,longitude,arcseconds_per_block}`. Model links the frozen physical model.
  The record contains neither a world-sized piece list nor a global piece counter.
- **`[3]+entity-ID` Piece:** `{id,assembly,local_id,geometry,mesh,replicas}`. The stable
  ID is the Birth CID, not this state payload's CID. Geometry is
  `{part,color,rgb?,position,transform,step}`; its 3×3 matrix is flat column-major
  public coordinates. Mesh is a world-namespace geometry CID. Replicas are block
  coordinates containing references to the same piece.
- **`[1]+block-address` spatial rows:** adaptive local children `0` hold references;
  children `1/q` descend by octant, up to ten levels. **SpatialRef** is
  `{coord,piece,bounds}` using render bounds. There is one reference per piece/
  intersected storage block, not one in every fine cell it crosses.
- **`[5]+block-address` BlockHeader:** `{coord,paged:true,packed?:bool}`. Missing
  `packed` means false. Layout 3 may supply a complete packed read projection.
  This sparse allocated
  block directory is not a preallocated world grid.
- **`[3,2]+block-address+shard` SpatialPage:**
  `{format:"ergo-spatial-page-1",coord,shard,pieces:{BirthCID:Piece}}`.
  Entity IDs begin with padded octants 0/1, so child 2 cannot collide with canonical
  Piece paths. The first byte of the birth hash selects one of 256 shards, encoded
  in three octants. Pages contain inline complete Piece metadata and mesh CIDs,
  not duplicated mesh vertices. Each page is limited to 1 MiB and 1,024 records.
  `packed:true` means complete coverage for that block in this accepted snapshot.
  Edit transactions maintain pages atomically, including colour-only edits and
  cross-block replica changes. Page overflow or a projection update exceeding its
  separate 64 MiB read-and-update budget disables the block projection and removes its pages;
  it does not impose a world-size limit or bypass canonical validation.
- **`[4]+assembly-ID` paged rules:** child `0` is
  **RulesHeader** `{id,name,meshes,paged:true,source_model?,creation?,piece_count?}`;
  creation is retained across edits and new headers store the exact current count.
  Meshes map part IDs
  to CIDs. Child `1` has `{local_id,piece}` membership rows, child `2` peer-CID
  adjacency, child `3` endpoint-bound `{a,b,spec}` joints, child `4` boolean anchor
  markers, child `5` group headers/members and child `6` authored joint specs.
  Numeric row keys use eleven octants. Group headers are `{id,name,pivot_world}`;
  members are tagged piece/group records. `shared/paged.rs` defines suffixes.
- **`[6]+joint-ID` GlobalJoint:** `{id,a,b,spec}` with global birth endpoints.
  **`[7]+piece-ID`** has incident joint-ID rows. Specs carry public anchors,
  fixed/revolute kind, optional axis/limits, measured force/torque thresholds and
  their source. These rows permit bounded cross-assembly dependency discovery.

### Linked blobs

- **Assembly birth:** new records use
  `["assembly-birth-v2",nonce,actor,name,creation]`, where creation has
  `{created_by,created_by_name?,created_at?}`. `created_at` is server-recorded Unix
  milliseconds. Existing `["assembly-birth-v1",nonce,actor,name]` identities remain
  valid and retain their creator, but contain no creation timestamp. Their dates
  are not fabricated from later edits/object timestamps. The hash remains the
  assembly identity after renames/replacements.
- **Birth:** `{nonce,created_by,created_by_name?,assembly,initial,mesh}`. Initial is
  Geometry. Names are server-resolved attribution snapshots; permissions use stable
  actor IDs. This is not a personal cryptographic signature by a display name.
- **Joint birth:** `["joint-birth-v1",nonce,actor,input]`; input contains global
  endpoints, public anchor and kind.
- **PhysicalModel:** `{algorithm,measurements,tolerances}`. Measurements contain
  connector entries and optional per-part records. See [calibration](physical-calibration.md)
  for sourced SI fields; the client cannot override this frozen server-owned model.
- **Audit:** `{actor,actor_name?,base,policy_hash,model,changed_pieces,
  affected_pieces,watched_regions,validation,loaded_pieces,loaded_cbor_bytes}`.
  Validation may be null for non-geometric operations.
- **Native Model:** `{format:"ergo-model-1",schema:2,root,definitions}`. Instances
  have part/local-definition/versioned-construct targets, optional inherited colors
  and native row-major 4×4 float64 transforms. Referenced common constructs come
  from the versioned library; local reusable definitions are stored once per model.
  See [construction format](construction-format.md). Current Piece states remain
  live geometry authority; source hierarchy cannot restore stale edited poses.
- **Mesh:** the Ergo geometry record described below. World meshes are retained
  independently of cache objects. Quality/source annotations and credits are
  provenance, not a choice of external storage decoder.

The collector follows typed tree/history links and application blob links in
`shared/retention.rs`. Hash-looking strings in comments are not implicit links.
Packed-page entries additionally retain the embedded Piece's Birth, assembly and
mesh links. Old accepted roots keep their original projection versions.

## `cache/read-views/v1/<hash>.cbor`: discardable read packet

Discardable `cache/read-views/v1/*.cbor` objects use a separate compressed read
packet, not a canonical world record. Its envelope has `format`, `key`
(world/revision/cell/limits), `logical_read_bytes`, `decoded_bytes`, `checksum` and
gzip bytes. Authorization/revision checks and bounded decompression precede use;
see [loading caches](loading-and-cache.md). Removing this cache does not remove
accepted history or geometry.

## `read-cache/metadata-v1/<world>/<00..15>.json`: catalogue cache

Uncompressed UTF-8 JSON `{format:1,etag,revision,checksum,text}`. `text` is an encoded
JSON response string, not a nested response object. Its SHA-256 checksum covers its
UTF-8 bytes; both the envelope and decoded response must identify the expected
accepted revision. Sixteen hash-selected slots are reused per world. Response text
is at most 512 KiB and the stored envelope at most 1,052,672 bytes. These are
discardable projections, never world-GC roots or publication input. Current
authorization precedes reuse, including HTTP 304 responses.

## `assets/parts/<part>.json`: Ergo part description

UTF-8 JSON `{format:"ergo-part-1",schema:2,part,mesh_hash,bounds,body_bounds,
catalog_mass,credits,library_revision?}`. Catalog mass is optional `{kg,source}`. Credits contain
`{file,source_url,author,license}`. The hash selects **application-namespace**
`objects/<mesh_hash>.cbor`, not `world/objects/`.
Only the canonical part ID gets a current description; aliases select it rather
than creating another entry. A stale library revision rebuilds the cache from
the verified native pack before updating the description. Existing accepted world
meshes remain frozen until an ordinary validated retrofit is accepted.

## `objects/<mesh_hash>.cbor`: Ergo Mesh

CBOR `{schema:2,part,quality,vertices,bounds,body_bounds,credits,physical?}`.
Vertices are an array of floats, not packed binary: nine values per vertex
`position.xyz,normal.xyz,color.rgb`, three vertices per triangle. Positions/normals
use public Z-up coordinates; negative face colors inherit the instance color.
Render bounds include details/studs; nominal collision body bounds exclude
decorative studs for known parts. Physical metadata is a sourced measurement,
not an inference from triangle volume.

The description is published after its geometry; cache reads verify hash, schema
and part identity. Native client LODs and instance lists are derived, not additional
persisted formats. Source inputs occupy only a bounded transient import cache.

## `ground/rasters/<provider-hash>/<z>/<x>/<y>.cbor`: Ergo raster

CBOR `{format:"ergo-raster-1",fetched_at,etag,modified,media_type,image}`. The
provider hash is SHA-256 of the XYZ URL template. The image is the original encoded
PNG/JPEG as a CBOR byte string, not base64 or decoded pixels. Times are Unix seconds;
ETag/Last-Modified support conditional refresh. The normal freshness interval is
seven days.

## `ground/blocks/<signature>/<east>_<north>.cbor`: GroundBlock

CBOR `{schema:2,coord,bricks,color,sample_rgb,source,attribution,attribution_url,
generated_at,source_tiles,fallback_reason}`. Public coord is `[east,north,0]` and
bricks is empty. One palette color covers the virtual baseplate. Sample RGB is an
optional 24-bit integer. Source/attribution explain the derived color.

The configuration signature covers geographic origin/resolution, provider/template,
attribution, enabled state, palette and the averaging algorithm. It is not a hash
of the payload. Successful cells have a seven-day TTL; fallback cells retry after
60 seconds. A single `color` is required; old color-grid records are not accepted.

## Private account JSON

- **`users/<stable-user-id>.json`:** `{id,name,email,picture,created_at,last_login}`.
  Picture is an optional URL; times are Unix seconds. The profile remains private.
- **`user-positions/<actor-hash>.json`:**
  `{format:"ergo-user-position-1",schema:2,context,updated_at_ms}`. Context contains
  `{world_id,position_world,view_target_world,navigation,client_session,sequence}`.
  Camera/target are public construction coordinates; navigation is orbit/fly/walk.
  The server assigns the timestamp. Conditional writes reject delayed lower
  sequence numbers from the same browser session. The authenticated GET returns
  this user's context plus a ground-level point/transform for new-model placement.
- **`edit-pacing/<actor-hash>.json`:**
  `{credits,updated,bricks,seconds,reservations}`. Credits are brick-change × seconds
  units; available changes are credits/seconds, refill adds bricks per second,
  capped at bricks×seconds. Reservation receipts map IDs to `{bricks,at}`.
- **`account-plans/<actor-hash>.json`:** `{unlimited:boolean}`. Missing means false.
  Only the operator sets the plan; unlimited enables god mode with no pacing quota
  or authoring brick-count cap. It does not bypass physical acceptance.

Actor hashes are SHA-256 of the authenticated ID, not construction IDs. Position
GET/PUT has no arbitrary user-ID parameter. These records are outside world GC.

## Development/private-snapshot compatibility records

These paths are supported by `store.rs`/`catalog.rs`; the production common-world
configuration disables creating new private snapshots:

- **`worlds/<owner>/<uuid>/manifest.json`:** uncompressed UTF-8 `WorldManifest`
  JSON with `schema`, UUID/name/owner/source, total count/bounds/geo, block
  `{coord,key,brick_count}` references, part-to-mesh `geometry` CID map, proxy-part
  list, notes, joints/anchors/groups, validation, ground and optional native model.
  Its objects must exist before publishing the manifest. Owner-scoped lookup
  checks the authenticated principal; manifest discovery is not authorization.
- **`objects/<CID>.cbor` Block:** ordinary native CBOR `{schema,coord,bricks}`.
  The manifest selects this type; the same namespace also holds native Mesh
  objects. Bounds, transforms and part/colour values use schema-2 coordinates.
- **`_private_catalog/<owner-hash>/...`:** a transactional repository with
  `refs/main.cbor`, `maintenance.cbor` and immutable `objects/<CID>.cbor`. Summary
  leaves contain `{id,name,owner,brick_count,bounds,geo}`. The head/index/node
  encodings are the same deterministic transaction types described above.
- **`shared/_catalog/...`:** optional multi-world discovery transaction repositories
  for actor, geographic-directory and geographic-tile indexes. Advertisement leaves
  are `{world,coord?}`; directory leaves are quadkey strings. Each advertised result
  rechecks current world policy and actual allocated data. In the deployed one-world
  configuration these discovery writes are disabled.

With no common-world prefix override, individual shared-world repositories use
`shared/<uuid>` instead of `world`; their typed records and encodings are identical.

## Private operator files

`s3://ergo/_ops/ergo/production.env` is plaintext dotenv recovery configuration;
it may contain secrets. Migration receipts under `_ops/ergo/migrations/*.json`
record verified heads/counts/checksums and conversion results, without credential
or profile bodies. They are not runtime scenes.

Local working files, client binding journals, generated PNGs and WASM bundles are
not automatically uploaded to S3. Public world visibility does not grant anonymous
bucket access. Age, ETag, or absence from a camera view does not prove that an
object is safe to delete; use coordinated schema-aware collection.
