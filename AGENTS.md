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

## Migrate forward; do not maintain legacy systems

- When a format, library, identifier scheme or organization changes, migrate
  every existing construct, stored construction, fixture and generated asset to
  the new form in the same change (or with an explicit operator migration step).
- Do not keep legacy code paths, compatibility shims, parallel old/new systems,
  fallback readers or duplicate organizations alive "just in case". Remove the
  old system once its data is migrated.
- Prefer one authoritative pipeline per concern (for example one part library
  and one synthesis procedure) over layered special cases.

### S3 migration is a release requirement

- Before releasing an S3 storage or index change, inventory all affected data,
  not just the data touched by a smoke test. Prefer an eager, resumable backfill
  and verify completeness before deploying code that depends on the new form.
  Check that canonical data and retained revisions are preserved as required.
- Use lazy migration on read only when eager migration is impractical. Make it
  explicit, idempotent and safe under concurrent access. Track what remains,
  verify completion, and remove the legacy path after cutover. Reading the old
  form without converting it does not count as migration.
- If nothing persisted changed, verify the existing format or index coverage
  rather than rewriting data. If verification or a required migration fails,
  stop the rollout and report what remains. Follow the
  [storage operations runbook](docs/storage-operations.md).
