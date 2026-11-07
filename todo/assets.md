# Asset and import plans

These items are not implemented.

- Derived (`auto-v1`) part profiles beyond studs and sockets:
  - Technic bores/pins/axles, bars, clips and hinges from their LDraw primitives;
  - sideways studs;
  - compound collision boxes and bearing regions for non-box moulds.
- A thin-shell mass model, so that stickers, cloth, sails and string (excluded
  for lack of enclosed volume) can enter the library.
- Better volume estimates for thin mouldings that official geometry models as
  solid (for example baseplates).
- Run the first production `parts-sync` with a Rebrickable key. Until then BrickLink
  numbers are neither deduplicated nor listed. Then delete the retired
  `assets/parts/` S3 prefix.
- Read-only model inspiration/catalogue adapters with provenance, licensing,
  pagination and provider-specific authentication; complete models and parts are
  different discovery products.
- Exact wheel/axle/pin/hinge collision and connector metadata with measurements.
- Additional material, texture, face-orientation and specialty geometry support.
- Representative import coverage for archive/encryption/custom-part variations.
- Incremental reusable-definition editing/re-deduplication.
- Explicit positioning previews, exact XYZ/GPS/file-pose choice, drag/rotate and a
  Place in Sandbox confirmation step for imports.
- Packed/compressed binary pages only after profiling the current CBOR payloads.
