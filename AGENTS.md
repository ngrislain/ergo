# Project guidance

## Keep documentation synchronized with the code

- Update the relevant `docs/*.md` files in the same change whenever behavior,
  architecture, algorithms, APIs, configuration, storage formats, permissions,
  limits, or guarantees change.
- Keep equations, worked examples, source-file references, and SVG figures in
  `docs/figures/` consistent with the implementation.
- Clearly distinguish implemented behavior from proposals and future work;
  never document a planned feature as an existing guarantee.
- Update `docs/README.md` and the root `README.md` when navigation, setup, or
  supported workflows change.
- Before finishing a change, check the affected documentation for stale names,
  constants, endpoints, assumptions, and links. Code changes are not complete
  until their documentation is accurate.
