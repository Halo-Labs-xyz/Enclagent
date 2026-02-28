#!/usr/bin/env node
/**
 * Bundle Privy + React for launchpad. Fixes keccak_256 ESM resolution.
 * Run: npm run bundle-privy  (or: node scripts/bundle-privy.mjs)
 */
import * as esbuild from "esbuild";
import { fileURLToPath } from "url";
import { dirname, join } from "path";

const __dirname = dirname(fileURLToPath(import.meta.url));
const root = join(__dirname, "..");
const out = join(root, "src/channels/web/static/privy-bundle.js");

await esbuild.build({
  entryPoints: [join(__dirname, "privy-bundle-entry.js")],
  bundle: true,
  format: "esm",
  platform: "browser",
  target: ["es2020"],
  outfile: out,
  minify: true,
  sourcemap: false,
  define: { "process.env.NODE_ENV": '"production"' },
});

console.log("Bundled Privy to", out);
