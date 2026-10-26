import { defineConfig } from "vite";

export default defineConfig({
  // Every asset as a file: the content security policy allows no data: fonts or scripts.
  build: { assetsInlineLimit: 0 },
});
