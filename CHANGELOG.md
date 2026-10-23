# Changelog

## v0.3.0

- Add authenticated MCP publication, accepted-revision read-back, browser view
  links, edit allowance inspection, and browser-assisted sign-in/setup.
- Add landing/Google SSO pages with MCP/API instructions and short-lived token
  generation, clipboard copy, and credential-file download.
- Add collaborative shared-world access, persistent per-user edit pacing
  (5,000 changed bricks refilled per day by default), and operator-managed
  unlimited paid entitlement. Authorization and physical checks still apply.
- Add bounded shared-world navigation, full-screen exploration, and camera-relative
  nearest-construction discovery.
- Resolve Studio/LDraw parts on demand, retain standard-first recommendations,
  embed the sourced full LDraw color catalog, and accept sourced operator mass/
  nominal-bound records while rejecting unmeasured publication.
- Render studded 1024×1024 ground plates with satellite/aerial-image-derived
  standard colors, JPEG imagery decoding, bounded geometric studs, and distant LOD.
- Add Scaleway/OpenTofu definitions for one scale-to-zero API/web container,
  the existing `ergo` bucket, and `ergosandbox.com` apex DNS/managed HTTPS.
- Add Podman-compatible native cross-compilation for the Linux/AMD64 server,
  packaging, and actual-container API/browser smoke coverage on Apple Silicon.
- Correct local loopback-origin validation and handle container SIGTERM shutdown.

Shared-world browser views are read-only spatial regions; edits use MCP/REST.
Billing-provider automation, exact global nearest-neighbor search, and optical
material simulation remain future work. Live cloud deployment is separate from
the repository's mocked infrastructure and isolated application checks.
