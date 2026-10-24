# Ergo public website

The public shared-renderer WebGL landing page and static agent guide for
**https://www.ergosandbox.com**. The authenticated sandbox is at
**https://ergosandbox.com**.

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

The visual system uses cool building-mat blue (`#edf3ff`), dark ink (`#182a3a`),
plastic blue (`#315fed`), yellow (`#ffd23f`), red (`#ee6748`), and mint (`#a3c6ac`).
Self-hosted Outfit provides the body typography; the canonical four-stud logo and
Manrope wordmark match the authenticated sandbox. The main visual is
a brick-built doorway, with a numbered build-manual layout for the actual
human-to-agent handoff.

See [the full website guide](../docs/static-site.md) for GitHub Pages automation,
DNS, HTTPS, testing, content ownership, and OAuth branding URLs.
