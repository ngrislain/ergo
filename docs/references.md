# Implementation references

These sources support the techniques actually used by Ergo. Research for
unimplemented work is indexed in [TODO.md](../TODO.md).

## Spatial indexing

Hanan Samet, [The Quadtree and Related Hierarchical Data Structures](https://doi.org/10.1145/356924.356930)
(1984), provides background for hierarchical regional queries. Ergo implements
derived resident octrees and immutable packed octree records, with its own
application layouts and budgets.

## S5. Engine-style instancing and pass reuse

Francesco Carucci, [Inside Geometry Instancing](https://developer.nvidia.com/gpugems/gpugems2/part-i-geometric-complexity/chapter-3-inside-geometry-instancing),
GPU Gems 2, chapter 3 (2005), separates prototype geometry, instance attributes,
batch updates and repeated draw passes. Ergo applies those ideas to reusable
block prototypes, shared color/depth geometry and change-driven uploads using
[WebGL2 instanced drawing](https://developer.mozilla.org/en-US/docs/Web/API/WebGL2RenderingContext/drawArraysInstanced).
The chapter's Direct3D implementation and historical performance numbers are
not Ergo's backend or performance claims.

## Static contacts and client dynamics

David Baraff, [Fast Contact Force Computation for Nonpenetrating Rigid Bodies](https://doi.org/10.1145/192161.192168)
(1994), explains unilateral reaction constraints. Ergo's server uses its own
constructive gravity/compression and constraint-wrench certificates.
[Rapier](https://rapier.rs/docs/) is the implemented local WASM dynamics engine.

## Transactions and encoding

Kung/Robinson, [On Optimistic Methods for Concurrency Control](https://doi.org/10.1145/319566.319567)
(1981), motivates dependency validation and re-execution on conflict. Ergo uses
immutable subtree versions, explicit predicates and a conditional head PUT.
[RFC 8949](https://www.rfc-editor.org/rfc/rfc8949) defines the deterministic CBOR
encoding used for authoritative transaction objects.

## Identity and providers

- [Google OpenID Connect](https://developers.google.com/identity/openid-connect):
  browser identity validation, distinct from downloaded agent delegation.
- [Scaleway Object Storage](https://www.scaleway.com/en/docs/object-storage/):
  the deployed S3-compatible backend.
- [BrickLink's 3001 catalog entry](https://www.bricklink.com/v2/catalog/catalogitem.page?P=3001):
  an example source for catalog mass records; catalog mass is not clutch strength.
- [OSM tile policy](https://operations.osmfoundation.org/policies/tiles/): request
  identification, interactive demand, attribution and source-cache behavior for
  that configured raster provider.
