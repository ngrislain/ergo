export const DEFAULT_MCP_PACKAGE =
  "https://github.com/ngrislain/ergo/releases/download/v0.3.1/ngrislain-ergo-mcp-0.3.1.tgz";

export function packageSource(value = "") {
  const spec = value.trim() || DEFAULT_MCP_PACKAGE;
  if (/^@ngrislain\/ergo-mcp(?:@[a-zA-Z0-9._~^*+-]+)?$/.test(spec)) return spec;
  try {
    const url = new URL(spec);
    if (
      url.protocol === "https:" &&
      url.hostname === "github.com" &&
      !url.username &&
      !url.password &&
      !url.search &&
      !url.hash &&
      /^\/[^/]+\/[^/]+\/releases\/download\/[^/]+\/[^/]+\.tgz$/.test(
        url.pathname,
      )
    )
      return spec;
  } catch {}
  throw new Error(
    "ERGO_MCP_PACKAGE must be a public GitHub Release .tgz URL or @ngrislain/ergo-mcp package spec.",
  );
}

export function packageTokens(text, spec, html = false) {
  const shell = "'" + spec.replaceAll("'", "'\\''") + "'";
  const escape = (value) =>
    html
      ? value
          .replaceAll("&", "&amp;")
          .replaceAll("<", "&lt;")
          .replaceAll(">", "&gt;")
          .replaceAll('"', "&quot;")
          .replaceAll("'", "&#39;")
      : value;
  return text
    .replaceAll("%ERGO_MCP_PACKAGE_SHELL%", escape(shell))
    .replaceAll(
      "%ERGO_MCP_PACKAGE_PROVIDER%",
      spec.startsWith("https://") ? "GitHub Releases" : "the npm registry",
    )
    .replaceAll("%ERGO_MCP_PACKAGE%", escape(spec));
}
