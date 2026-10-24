import { resolve } from "node:path";
import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import { footer, header } from "./src/layout.ts";

export default defineConfig({
  base: "/",
  plugins: [
    {
      name: "ergo-shared-layout",
      transformIndexHtml(html) {
        return html
          .replace("<!-- site:header -->", header)
          .replace("<!-- site:footer -->", footer);
      },
      generateBundle() {
        for (const family of ["outfit", "manrope"]) {
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
});
