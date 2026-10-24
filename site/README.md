# Ergo public website

The public Three.js landing page and static agent guide for
**https://www.ergosandbox.com**. The authenticated sandbox is at
**https://ergosandbox.com**.

```sh
npm ci --ignore-scripts
npm run build
npm test
npm run preview -- --port 4173
```

The home page is an accessible HTML guide with a progressively enhanced WebGL
illustration. `/privacy/`, `/terms/`, and `/llms.txt` work as static documents.
Fonts and geometry are self-hosted; the site collects no credentials or analytics.

The visual system uses cool building-mat blue (`#edf3ff`), dark ink (`#182a3a`),
plastic blue (`#315fed`), yellow (`#ffd23f`), red (`#ee6748`), and mint (`#a3c6ac`).
Self-hosted Outfit provides the rounded, geometric typography. The main visual is
a brick-built doorway, with a numbered build-manual layout for the actual
human-to-agent handoff.

See [the full website guide](../docs/static-site.md) for GitHub Pages automation,
DNS, HTTPS, testing, content ownership, and OAuth branding URLs.
