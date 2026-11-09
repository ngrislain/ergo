import fs from "node:fs"
import path from "node:path"
import { fileURLToPath } from "node:url"
import { Plugin, Skill } from "@opencode/plugin"

const here = path.dirname(fileURLToPath(import.meta.url))
const skillPath = path.join(here, "skills", "build", "SKILL.md")

// Prefer the version-matched bridge installed as this package's npm
// dependency. Locate node_modules/@ngrislain/ergo-mcp from this package's
// own directory instead of the module resolver, because plugin hosts may
// restrict module resolution to their own SDK. Checkout-local loads without
// installed dependencies fall back to a globally installed ergo-mcp on PATH.
// Sign-in stays in the human's browser; per-origin sign-in state is stored
// privately by the bridge and shared by every bridge copy.
function bridgeCommand(): string[] {
  const own = JSON.parse(fs.readFileSync(path.join(here, "package.json"), "utf8"))
  let dir = here
  for (;;) {
    const manifestPath = path.join(dir, "node_modules", "@ngrislain", "ergo-mcp", "package.json")
    if (fs.existsSync(manifestPath)) {
      const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"))
      const entry = path.resolve(path.dirname(manifestPath), manifest.bin["ergo-mcp"])
      if (manifest.version === own.version && fs.existsSync(entry)) return ["node", entry, "--headless"]
    }
    const parent = path.dirname(dir)
    if (parent === dir) return ["ergo-mcp", "--headless"]
    dir = parent
  }
}

function loadSkill() {
  const markdown = fs.readFileSync(skillPath, "utf8")
  const frontmatter = /^---\n([\s\S]*?)\n---\n/.exec(markdown)
  if (!frontmatter) throw new Error(`Missing frontmatter in ${skillPath}`)
  const field = (name: string) => {
    const match = new RegExp(`^${name}: (.+)$`, "m").exec(frontmatter[1])
    if (!match) throw new Error(`Missing ${name} in ${skillPath} frontmatter`)
    return match[1]
  }
  return {
    name: field("name"),
    description: field("description"),
    content: markdown.slice(frontmatter[0].length),
  }
}

export default Plugin.define({
  id: "ergo",
  async setup(ctx) {
    await ctx.mcp.transform((editor) => {
      editor.set("ergo", {
        type: "local",
        command: bridgeCommand(),
        environment: { ERGO_PUBLISH_URL: "https://ergosandbox.com" },
        timeout: { startup: 660000, catalog: 30000, execution: 3900000 },
      })
    })
    const skill = loadSkill()
    await ctx.skill.transform((editor) => {
      editor.add({
        id: Skill.ID.make("ergo-build"),
        name: Skill.Name.make(skill.name),
        description: skill.description,
        path: Skill.Info.fields.path.make(skillPath),
        content: skill.content,
      })
    })
  },
})
