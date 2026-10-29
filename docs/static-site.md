# Public project website on GitHub Pages

`site/` is the standalone public guide at **https://www.ergosandbox.com**.
The public sandbox and REST API use **https://ergosandbox.com**; editing requires
authentication.
The site links humans to the sandbox's `/login#connect` flow; it does not issue
tokens, receive credentials, proxy the API, or provide a remote MCP endpoint.

## Pages and content

- `/`: an interactive shared-engine WebGL brick illustration, agent onboarding, visible
  MCP build/start commands, client configuration, direct-API example, and a
  first construction prompt.
- `/privacy/` and `/terms/`: static policy pages describing the hosted service,
  current Google/profile storage, cookies, delegation limits, shared-world
  visibility, third-party providers, and experimental physics limitations.
- `/llms.txt`: a plain-text authentication and building guide for agents.
- `/404.html`: a real Pages error document with a link back to the guide.

Agent instructions follow [access and onboarding](access-and-onboarding.md) and
the [MCP guide](local-construction-mcp.md). The setup installs the prebuilt npm
package with Node, not Rust. The current package is a public GitHub Release
tarball; the npm-registry source can be selected after registry publication.
Google login stays in the human's browser. The agent receives the path to a
downloaded, short-lived token and the operator-selected API origin. The example
prompt keeps its first construction local until the human requests publication.

Keep the policy pages and agent guide synchronized with authentication,
permissions, storage, provider, and retention changes. They describe current
behavior and the actual authentication/storage guarantees.

## Build and preview

Use Node.js 22.12+ (CI uses Node 24), Rust 1.97.1, the WASM target, and the
matching wasm-bindgen CLI:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
npm ci --prefix site --ignore-scripts
npm run build --prefix site
npm test --prefix site
npm run preview --prefix site -- --port 4173
```

Open `http://127.0.0.1:4173`. `npm run dev --prefix site` starts Vite's development
server. The production output is `site/dist/`, which is ignored by Git.

Vite builds separate HTML entries for the home, privacy, terms, and 404 pages.
The build injects the shared header/footer from `site/src/layout.ts`.
`cargo xtask build-site` builds the navigator's `ergo-client` renderer with the
small `site-entry` wrapper and copies the canonical `web/favicon.svg` logo.
Generated bindings/logo in `site/public/engine` and `site/public/favicon.svg` are
ignored. The build uses those generated outputs, not checked-in binaries.
`site/public/brick-outline.svg` is also generated: the inspector, fallback and
decorative brick illustrations project the same `3003.dat` catalog mesh in metric
proportions, with hidden-line removal. CSS preserves its aspect ratio rather than
stretching its height or inventing stud spacing. The logo is a top view with
20-LDU pitch / 12-LDU stud diameter on the 40×40-LDU 2×2 footprint.
The WASM renderer loads dynamically only on the home page; legal pages load no
3D engine. There is no Three.js dependency. Manrope headings/wordmark and DM Sans
body text match the apex interface and are self-hosted.
There are no analytics scripts,
external model downloads, token-entry fields, or persistent demo preferences.
Their font license notices are emitted with the site under `/licenses/`.

The white/slate/amber tokens, typography, logo, and buttons follow the apex UI.
The brick model uses the navigator's catalog geometry, instancing, shaders and
shadow pass, with a cursor-positioned point light and studio fill so it remains
readable at any time and from rear/side views. See [lighting](lighting.md).
Indirect light is scaled to 0.30 and studio fill to 0.35 for stronger cast shadows.
Free mouse movement positions the light; click-drag orbits the actual camera
horizontally and vertically while keeping the light fixed in world space.
Pinch changes the camera distance; wheel scrolling moves the page.
Arrow keys orbit, plus/minus zoom, and Home or **Reset view** restores the view.
The preview has no viewport frame or slogan sticker. Keyboard focus is indicated
on the control-help caption rather than by a blue rectangle around the canvas.
Hover and page scrolling never move the camera. The tree foliage is green.
Rendering stops when assembly/interaction settles.
Offscreen/hidden scenes pause, reduced-motion preferences are respected, and
WebGL failure leaves the onboarding text usable. The illustration does not use
or claim to replace the construction validator.

Build tests check the static routes, legal navigation, MCP origin/token-file
configuration, distribution boundaries, and current documentation semantics.
Browser checks cover real WebGL mouse/scroll controls, tabs, clipboard, 320/390-pixel layouts,
time-independent point lighting, shadow draw passes, reduced motion, WebGL-unavailable fallback,
and readable no-JavaScript content.

## Public package-source environment setting

`ERGO_MCP_PACKAGE` selects the npm package spec displayed in the homepage and
generated `/llms.txt`. It is a **public build setting**, not a credential. The
source default points to the versioned release asset below. The deployed
repository variable stays on the last verified asset until the new release is
published and verified:

```text
https://github.com/ngrislain/ergo/releases/download/v0.5.14/ngrislain-ergo-mcp-0.5.14.tgz
```

For a local override, copy `site/.env.example` to ignored `site/.env`, edit that
one variable, and rebuild. The production build reads the GitHub repository
variable with the same name. Update it only after the release asset is verified:

```sh
gh variable set ERGO_MCP_PACKAGE --repo ngrislain/ergo \
  --body 'https://github.com/ngrislain/ergo/releases/download/v0.5.14/ngrislain-ergo-mcp-0.5.14.tgz'
gh workflow run pages.yml --repo ngrislain/ergo --ref main
```

Use the actual published GitHub Release `.tgz` URL. A raw monorepo Git URL is not
an installable distribution. Credential-bearing source values are rejected.
The configured client launches the installed `ergo-mcp` command.

## GitHub Pages publication

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) builds and tests
`site/` on relevant pull requests and pushes to `main`. Changes to the shared
client/world/xtask, Cargo configuration, or canonical logo also trigger builds.
CI installs the pinned WASM toolchain and runs solar tests. Only `main` push/manual
runs deploy. The artifact contains **only `site/dist`**, not the repository,
`.env`, OpenTofu files, state, or plans. Deployment uses the repository's built-in
token with `pages: write` and `id-token: write`; no publishing secret is needed.

The repository's Pages settings use build type `workflow` and the custom domain
`www.ergosandbox.com`. The included `site/public/CNAME` documents that domain but
does not replace the GitHub Pages API/settings configuration.

To publish manually after the workflow is on `main`:

```sh
gh workflow run pages.yml --repo ngrislain/ergo --ref main
gh run list --repo ngrislain/ergo --workflow pages.yml --limit 5
```

Open the new run's URL to inspect build and deployment results. Website release
tags use `site-vVERSION`, matching `site/package.json`; a site tag does not use
the `v*` trigger for Rust/npm distribution releases.

## DNS and HTTPS

[`infra/scaleway/pages.tf`](../infra/scaleway/pages.tf) manages the `www` CNAME:

```text
www.ergosandbox.com.  CNAME  ngrislain.github.io.
```

The target is the GitHub Pages account hostname, not a repository path.
`github_pages_host` can override it; `manage_dns=false` also omits this record
when DNS is managed elsewhere. The apex ALIAS, server public origin, and Google
OAuth callback remain configured for `ergosandbox.com`.

GitHub provisions the `www` certificate after its DNS check. Enforce HTTPS in the
repository's Pages settings once the certificate is ready. The public URLs for
Google OAuth app branding are:

```text
Homepage: https://www.ergosandbox.com/
Privacy:  https://www.ergosandbox.com/privacy/
Terms:    https://www.ergosandbox.com/terms/
Callback: https://ergosandbox.com/auth/google/callback
```

The callback must be saved under **Authorized redirect URIs** of the actual Web
application OAuth client, not under JavaScript origins. After changing it,
restart login from the sandbox rather than reusing an old Google error URL.
