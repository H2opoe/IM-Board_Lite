import { useCallback, useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setNativeTheme, setThemeDockIcon } from "../api/appSettingsApi";
import { isTauri } from "../api/tauri";

export type ThemeChoice = "auto" | "light" | "dark";
export type ThemeMode = "light" | "dark";

const DEFAULT_THEME_CHOICE: ThemeChoice = "dark";
const THEME_STORAGE_KEY = "im-board-theme-mode";
const THEME_TRANSITION_MS = 320;

function systemThemeMode(): ThemeMode {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function resolveThemeMode(choice: ThemeChoice): ThemeMode {
  return choice === "auto" ? systemThemeMode() : choice;
}

function loadThemeChoice(): ThemeChoice {
  const requested = new URLSearchParams(window.location.search).get("theme");
  if (requested === "auto" || requested === "dark" || requested === "light") return requested;
  const stored = window.localStorage.getItem(THEME_STORAGE_KEY);
  return stored === "auto" || stored === "dark" || stored === "light" ? stored : DEFAULT_THEME_CHOICE;
}

export function useThemeController() {
  const initialThemeChoice = useRef<ThemeChoice>(loadThemeChoice());
  const [themeChoice, setThemeChoice] = useState<ThemeChoice>(initialThemeChoice.current);
  const [effectiveThemeMode, setEffectiveThemeMode] = useState<ThemeMode>(() => resolveThemeMode(initialThemeChoice.current));
  const themeTransitionTimer = useRef<number | null>(null);

  useEffect(() => {
    let cancelled = false;
    let unlistenThemeChanged: UnlistenFn | undefined;
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    const applyTheme = (nativeTheme?: ThemeMode | null) => {
      const effectiveTheme = nativeTheme ?? resolveThemeMode(themeChoice);
      setEffectiveThemeMode(effectiveTheme);
      document.documentElement.dataset.theme = effectiveTheme;
      document.documentElement.dataset.themeChoice = themeChoice;
      void setThemeDockIcon(effectiveTheme);
    };
    const applySystemTheme = () => applyTheme();

    window.localStorage.setItem(THEME_STORAGE_KEY, themeChoice);
    applyTheme();

    // 自动模式必须把原生窗口主题交还给系统，否则系统外观变化后 WebView 不一定会继续收到主题更新。
    void setNativeTheme(themeChoice === "auto" ? null : themeChoice);

    if (themeChoice === "auto") {
      if (isTauri) {
        void getCurrentWindow()
          .onThemeChanged(({ payload }) => {
            if (!cancelled) applyTheme(payload);
          })
          .then((unlisten) => {
            if (cancelled) {
              unlisten();
              return;
            }
            unlistenThemeChanged = unlisten;
          });
      }
      mediaQuery.addEventListener("change", applySystemTheme);
    }

    return () => {
      cancelled = true;
      unlistenThemeChanged?.();
      if (themeChoice === "auto") mediaQuery.removeEventListener("change", applySystemTheme);
    };
  }, [themeChoice]);

  useEffect(() => {
    return () => {
      if (themeTransitionTimer.current !== null) {
        window.clearTimeout(themeTransitionTimer.current);
      }
      delete document.documentElement.dataset.themeTransition;
    };
  }, []);

  const handleThemeChange = useCallback(
    (nextThemeChoice: ThemeChoice) => {
      if (nextThemeChoice === themeChoice) return;

      if (themeTransitionTimer.current !== null) {
        window.clearTimeout(themeTransitionTimer.current);
      }
      document.documentElement.dataset.themeTransition = "running";
      setThemeChoice(nextThemeChoice);
      themeTransitionTimer.current = window.setTimeout(() => {
        delete document.documentElement.dataset.themeTransition;
        themeTransitionTimer.current = null;
      }, THEME_TRANSITION_MS);
    },
    [themeChoice]
  );

  return {
    themeChoice,
    effectiveThemeMode,
    handleThemeChange
  };
}
