import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { releaseConfig } from "./scripts/release-config.mjs";

const releaseLabel = releaseConfig().releaseLabel;

const isWindows = process.platform === "win32";

export default defineConfig({
  plugins: [react()],
  define: {
    __IM_BOARD_RELEASE_LABEL__: JSON.stringify(releaseLabel)
  },
  clearScreen: false,
  resolve: {
    // Windows 共享盘/映射盘下 Vite 可能把真实路径和盘符路径拼接错，保留符号链接路径可避免入口文件解析漂移。
    preserveSymlinks: isWindows
  },
  server: {
    port: 1420,
    strictPort: false,
    watch: isWindows
      ? {
          // 映射盘上的构建产物和依赖备份目录容易触发 fs.watch/scandir 异常，Windows 下改用轮询并明确忽略。
          usePolling: true,
          ignored: [
            "**/src-tauri/target*/**",
            "**/node_modules/**",
            "**/node_modules.broken-*/**",
            "**/runtime/**"
          ]
        }
      : undefined
  },
  envPrefix: ["VITE_", "TAURI_"]
});
