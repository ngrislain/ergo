# Rendering and memory plans

These items are not implemented. Current rendering is documented in
[the display pipeline](../docs/3d-display-indexing.md).

- Per-page octrees, incremental updates and a small top-level page index.
- Dense storage-cell subdivision into independently streamed render pages.
- View-directed prefetch, eviction hysteresis, cancellation and decoded-byte budgets.
- Worker-based decoding/index construction and bounded simulation communication.
- Hi-Z occlusion and conservative interior-face elimination for suitable blocks.
- Error-bounded simplification for arbitrary part silhouettes and progressive meshes.
- Optional WebGPU profiling after measuring the existing WASM/WebGL2 baseline.
- True persistent byte-budgeted LRU, geometry reuse across authorized contexts,
  context-loss recovery, renderer parity and mobile benchmarks.
- Profile decode, index, upload, draw, triangles and memory separately before
  selecting further optimizations. Persist an index only when saved construction
  time outweighs transfer and invalidation costs.
