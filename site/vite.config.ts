import { defineConfig } from "vite";

export default defineConfig({
  build: {
    // Every asset as a file: the content security policy allows no data: fonts or scripts.
    assetsInlineLimit: 0,
    // Two pages, resolved from the site's root: the landing page and the mining guide.
    rollupOptions: {
      input: { main: "index.html", mine: "mine.html" },
    },
  },
});
