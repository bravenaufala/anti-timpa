import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port and fails if it is not available.
// See: https://tauri.app/start/frontend/vite/
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],

  // Prevent Vite from obscuring Rust errors.
  clearScreen: false,

  server: {
    // `TAURI_DEV_HOST` is set when developing against a physical device.
    host: host || "127.0.0.1",
    port: 1420,
    // Fail loudly instead of silently picking another port, because the
    // Tauri devUrl in tauri.conf.json is pinned to 1420.
    strictPort: true,
    // HMR websocket settings. Only used when TAURI_DEV_HOST is present.
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Rust sources are watched by cargo, not Vite.
      ignored: ["**/src-tauri/**"],
    },
  },

  // Produce output compatible with the WebView engines Tauri targets.
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
});
