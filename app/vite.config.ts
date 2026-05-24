import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { releaseConfig } from "./scripts/release-config.mjs";

const releaseLabel = releaseConfig().releaseLabel;

export default defineConfig({
  plugins: [react()],
  define: {
    __IM_BOARD_RELEASE_LABEL__: JSON.stringify(releaseLabel)
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: false
  },
  envPrefix: ["VITE_", "TAURI_"]
});
