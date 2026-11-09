import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent"

const here = path.dirname(fileURLToPath(import.meta.url))
const packageRoot = path.resolve(here, "..")

// Prefer the version-matched bridge installed as this package's npm
// dependency. Locate node_modules/@ngrislain/ergo-mcp from the package root
// instead of the module resolver, so the logic stays identical across
// package-manager layouts. Local path installs and missing dependencies fall
// back to a globally installed ergo-mcp on PATH. Sign-in stays in the human's
// browser; per-origin sign-in state is stored privately by the bridge and
// shared by every bridge copy.
function bridge() {
  const own = JSON.parse(fs.readFileSync(path.join(packageRoot, "package.json"), "utf8"))
  let dir = packageRoot
  for (;;) {
    const manifestPath = path.join(dir, "node_modules", "@ngrislain", "ergo-mcp", "package.json")
    if (fs.existsSync(manifestPath)) {
      const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"))
      const entry = path.resolve(path.dirname(manifestPath), manifest.bin["ergo-mcp"])
      if (manifest.version === own.version && fs.existsSync(entry))
        return { command: "node", args: [entry, "--headless"] }
    }
    const parent = path.dirname(dir)
    if (parent === dir) return { command: "ergo-mcp", args: ["--headless"] }
    dir = parent
  }
}

export default function (pi: ExtensionAPI) {
  const server = bridge()
  pi.registerMcpServer("ergo", {
    command: server.command,
    args: server.args,
    env: { ERGO_PUBLISH_URL: "https://ergosandbox.com" },
    description:
      "Find LEGO set inspiration and build in the shared Ergo world with human-approved browser sign-in.",
    // Per-request seconds; large construction reads and queued WASM jobs can run
    // close to an hour, matching the 3,900,000 ms budget documented for other hosts.
    timeout: 3900,
  })
}
