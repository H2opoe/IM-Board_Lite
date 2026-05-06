import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import "./styles/base.css";
import "./styles/app-shell.css";
import "./styles/notices.css";
import "./styles/dashboard.css";
import "./styles/profiles.css";
import "./styles/settings.css";
import "./styles/modals.css";
import "./styles/drawer.css";
import "./styles/theme-core.css";
import "./styles/theme-overrides.css";
import "./styles/theme-light.css";
import "./styles/page-overrides.css";
import "./styles/responsive.css";

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
