import { useCallback, useEffect, useRef, useState } from "react";
import { setThemeDockIcon } from "../api/appSettingsApi";

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
  const [themeChoice, setThemeChoice] = useState<ThemeChoice>(loadThemeChoice);
  const [effectiveThemeMode, setEffectiveThemeMode] = useState<ThemeMode>(() => resolveThemeMode(loadThemeChoice()));
  const themeTransitionTimer = useRef<number | null>(null);

  useEffect(() => {
    const applyTheme = () => {
      const effectiveTheme = resolveThemeMode(themeChoice);
      setEffectiveThemeMode(effectiveTheme);
      document.documentElement.dataset.theme = effectiveTheme;
      document.documentElement.dataset.themeChoice = themeChoice;
      void setThemeDockIcon(effectiveTheme);
    };
    window.localStorage.setItem(THEME_STORAGE_KEY, themeChoice);
    applyTheme();
    if (themeChoice !== "auto") return undefined;
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    mediaQuery.addEventListener("change", applyTheme);
    return () => mediaQuery.removeEventListener("change", applyTheme);
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
