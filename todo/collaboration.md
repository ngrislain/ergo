# Collaboration and scale-out plans

These items are not implemented.

- Cross-head/shard atomic commits and an explicit distributed coordination design.
- Model migrations with certified unloaded-region mechanical boundaries.
- Hierarchical leases only if a workflow needs them; the current protocol is
  optimistic validation plus conditional root publication.
- Idempotent model-creation recovery receipts and stronger multi-client workflows.
- Capacity studies against actual hot-shard workloads before enabling group
  publication broadly; paging alone is not a throughput guarantee.
