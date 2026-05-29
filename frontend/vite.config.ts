import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The Rust backend serves the built bundle from `/`; keep absolute asset paths
// so deep SPA routes still resolve assets correctly.
export default defineConfig({
  plugins: [react()],
  base: "/",
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    port: 5173,
    proxy: {
      "/api": { target: "http://127.0.0.1:8080", changeOrigin: true },
      "/health": { target: "http://127.0.0.1:8080", changeOrigin: true },
    },
  },
});
