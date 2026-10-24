import { resolve } from "node:path";
import { readFileSync } from "node:fs";
import { defineConfig, loadEnv } from "vite";
import { footer, header } from "./src/layout.ts";
import { packageSource, packageTokens } from "./src/package-source.mjs";

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, import.meta.dirname, "ERGO_MCP_PACKAGE");
  const spec = packageSource(
    process.env.ERGO_MCP_PACKAGE || env.ERGO_MCP_PACKAGE,
  );
  return {
    define: {
      "import.meta.env.VITE_ENGINE_VERSION": JSON.stringify(
        JSON.parse(
          readFileSync(resolve(import.meta.dirname, "package.json"), "utf8"),
        ).version,
      ),
    },
    base: "/",
    plugins: [
      {
        name: "ergo-shared-layout",
        transformIndexHtml(html) {
          return packageTokens(
            html
              .replace("<!-- site:header -->", header)
              .replace("<!-- site:footer -->", footer),
            spec,
            true,
          );
        },
        generateBundle() {
          for (const family of ["dm-sans", "manrope"]) {
            this.emitFile({
              type: "asset",
              fileName: `licenses/${family}.txt`,
              source: readFileSync(
                resolve(
                  import.meta.dirname,
                  `node_modules/@fontsource-variable/${family}/LICENSE`,
                ),
                "utf8",
              ),
            });
          }
          this.emitFile({
            type: "asset",
            fileName: "llms.txt",
            source: packageTokens(
              readFileSync(
                resolve(import.meta.dirname, "src/llms-template.txt"),
                "utf8",
              ),
              spec,
            ),
          });
        },
      },
    ],
    build: {
      rolldownOptions: {
        input: {
          home: resolve(import.meta.dirname, "index.html"),
          privacy: resolve(import.meta.dirname, "privacy/index.html"),
          terms: resolve(import.meta.dirname, "terms/index.html"),
          notFound: resolve(import.meta.dirname, "404.html"),
        },
      },
    },
  };
});
