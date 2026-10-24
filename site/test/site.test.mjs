import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";
import test from "node:test";

const dist = resolve(import.meta.dirname, "../dist");
const text = (path) => readFile(resolve(dist, path), "utf8");

test("all public pages have legal navigation and deployable static content", async () => {
  for (const path of [
    "index.html",
    "privacy/index.html",
    "terms/index.html",
    "404.html",
  ]) {
    const html = await text(path);
    assert.match(html, /href="\/privacy\/"/);
    assert.match(html, /href="\/terms\/"/);
    assert.match(html, /<main[^>]*id="main"|<main id="main"/);
    assert.doesNotMatch(html, /<!-- site:/);
    assert.doesNotMatch(html, /<script[^>]+src="https?:/);
    assert.doesNotMatch(html, /fonts\.(googleapis|gstatic)\.com/);
  }
  assert.equal((await text("CNAME")).trim(), "www.ergosandbox.com");
  await text(".nojekyll");
  assert.match(await text("licenses/outfit.txt"), /SIL OPEN FONT LICENSE/);
  assert.match(await text("licenses/manrope.txt"), /SIL OPEN FONT LICENSE/);
});

test("agent setup uses a human-issued token and the canonical API origin", async () => {
  const html = await text("index.html");
  const json = html.match(/<code id="mcp-config">([\s\S]*?)<\/code>/)?.[1];
  assert.ok(json, "MCP configuration must be readable without JavaScript");
  const configuration = JSON.parse(json);
  const server = configuration.mcpServers.ergo;
  assert.equal(server.env.ERGO_PUBLISH_URL, "https://ergosandbox.com");
  assert.equal(
    server.env.ERGO_PUBLISH_TOKEN_FILE,
    "/absolute/path/ergo-mcp-token",
  );
  assert.ok(server.command.endsWith("/target/release/ergo-mcp"));
  assert.match(html, /id="build-server"/);
  assert.match(html, /id="start-server"/);
  assert.match(html, /Copy command/);
  assert.match(html, /https:\/\/ergosandbox\.com\/login#connect/);
  assert.doesNotMatch(html, /npx -y @ngrislain/);
  const guide = await text("llms.txt");
  assert.match(guide, /no public remote MCP endpoint/);
  assert.match(guide, /not automatically retry/);
  assert.match(guide, /authenticated_editing=false/);
  assert.match(guide, /X-Ergo-Request: 1/);
});

test("privacy and terms describe current credential and sharing semantics", async () => {
  const privacy = (await text("privacy/index.html")).replace(/\s+/g, " ");
  const terms = (await text("terms/index.html")).replace(/\s+/g, " ");
  assert.match(privacy, /does not persist Google access or refresh tokens/);
  assert.match(privacy, /Logging out does not individually revoke/);
  assert.match(privacy, /not currently implemented/);
  assert.match(terms, /not professional engineering advice/);
  assert.match(terms, /authenticated_editing: false/);
});

test("the Pages artifact contains no dotenv, state, or infrastructure files", async () => {
  async function walk(path = "") {
    for (const entry of await readdir(resolve(dist, path), {
      withFileTypes: true,
    })) {
      const name = path ? `${path}/${entry.name}` : entry.name;
      assert.doesNotMatch(
        name,
        /(^|\/)(\.env(?:\.|$)|infra(?:\/|$))|\.tfvars(?:\.|$)|\.tfstate(?:\.|$)|\.tfplan$/,
      );
      if (entry.isDirectory()) await walk(name);
    }
  }
  await walk();
});

test("landing uses the navigator's engine and canonical logo", async () => {
  const logo = await readFile(
    resolve(import.meta.dirname, "../../web/favicon.svg"),
    "utf8",
  );
  assert.equal(await text("favicon.svg"), logo);
  const bindings = await text("engine/ergo_client.js");
  assert.match(bindings, /class LandingPreview/);
  const wasm = await readFile(resolve(dist, "engine/ergo_client_bg.wasm"));
  assert.deepEqual([...wasm.subarray(0, 4)], [0, 97, 115, 109]);
  const packageJson = JSON.parse(
    await readFile(resolve(import.meta.dirname, "../package.json"), "utf8"),
  );
  assert.equal(packageJson.dependencies.three, undefined);
  assert.match(await text("index.html"), /img src="\/favicon.svg"/);
});
