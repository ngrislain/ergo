# Ergo public website

The public shared-renderer WebGL landing page and static agent guide for
**https://www.ergosandbox.com**. The authenticated sandbox is at
**https://ergosandbox.com**.

Consumer setup is an npm installation, selected by public `ERGO_MCP_PACKAGE`.
It currently installs the prebuilt GitHub Release tarball; the same setting can
later point at the npm registry. The commands below are for **site maintainers**.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci --ignore-scripts
npm run build
npm test
npm run preview -- --port 4173
```

The home page is an accessible HTML guide with a progressively enhanced WebGL
illustration. `/privacy/`, `/terms/`, and `/llms.txt` work as static documents.
The illustration uses the navigator's Rust/WASM engine and catalog geometry;
`cargo xtask build-site` generates its ignored bindings and copies the shared
`web/favicon.svg` logo. Geographic UTC sun lighting, shadows, ambient/specular
appearance, and the automatic night flashlight are common to both sites.
Fonts and geometry are self-hosted; the site collects no credentials or analytics.

The visual system follows the application's white/slate/amber tokens, DM Sans
body typography, canonical four-stud logo, and Manrope headings/wordmark.
The scene's night light stays fixed when the camera orbits. The main visual is
a brick-built doorway, with a numbered build-manual layout for the actual
human-to-agent handoff.

See [the full website guide](../docs/static-site.md) for GitHub Pages automation,
DNS, HTTPS, testing, content ownership, and OAuth branding URLs.
