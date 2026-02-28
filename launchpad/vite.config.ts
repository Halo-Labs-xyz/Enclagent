import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";
import { readdirSync, rmSync } from "fs";

function cleanLaunchpadAssets() {
  return {
    name: "clean-launchpad-assets",
    apply: "build" as const,
    buildStart() {
      const outDir = resolve(__dirname, "../src/channels/web/static");
      for (const file of readdirSync(outDir)) {
        if (/^launchpad(?:-[A-Za-z0-9_.-]+)?\.js$/.test(file) || file === "launchpad.html") {
          rmSync(resolve(outDir, file), { force: true });
        }
      }
    },
  };
}

export default defineConfig({
  plugins: [cleanLaunchpadAssets(), react()],
  build: {
    modulePreload: false,
    outDir: resolve(__dirname, "../src/channels/web/static"),
    emptyOutDir: false,
    rollupOptions: {
      input: resolve(__dirname, "launchpad.html"),
      output: {
        entryFileNames: "launchpad.js",
        chunkFileNames: "launchpad-[name].js",
        inlineDynamicImports: false,
        assetFileNames: (info) =>
          info.name?.endsWith(".css") ? "launchpad-app.css" : "launchpad.[ext]",
        manualChunks: (id) => {
          if (
            id.includes("/react-dom/") ||
            id.includes("/react/") ||
            id.includes("/scheduler/")
          ) {
            return undefined;
          }
          if (id.includes("node_modules")) return "privy";
          return undefined;
        },
      },
    },
    sourcemap: false,
    minify: "esbuild",
    target: "es2020",
  },
  base: "/",
});
