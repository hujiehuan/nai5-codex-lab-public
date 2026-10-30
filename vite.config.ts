import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { mockPlugin } from "./mock/server.mjs";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(({mode}) => ({
  plugins: [vue(), ...(mode === 'mock' ? [mockPlugin()] : [])],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 5174 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
}));
