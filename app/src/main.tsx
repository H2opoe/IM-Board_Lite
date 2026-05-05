import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "./styles.css";

type AppPlatform = "macos" | "windows" | "other";

function detectAppPlatform(): AppPlatform {
  const platformHint = `${window.navigator.platform} ${window.navigator.userAgent}`.toLowerCase();
  if (platformHint.includes("win")) return "windows";
  if (platformHint.includes("mac")) return "macos";
  return "other";
}

document.documentElement.dataset.platform = detectAppPlatform();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
