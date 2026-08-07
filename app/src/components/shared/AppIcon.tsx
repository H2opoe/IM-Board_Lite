import darkIcon from "../../assets/app-icons/app-icon-dark.png";
import lightIcon from "../../assets/app-icons/app-icon-light.png";

interface Props {
  className?: string;
  variant?: "auto" | "dark" | "light";
}

function currentThemeMode() {
  const theme = document.documentElement.dataset.theme;
  if (theme === "dark" || theme === "light") return theme;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function AppIcon({ className = "", variant = "auto" }: Props) {
  const themeMode = variant === "auto" ? currentThemeMode() : variant;
  const iconSrc = themeMode === "dark" ? lightIcon : darkIcon;
  return <img className={`app-icon ${className}`.trim()} src={iconSrc} alt="IM-Board" />;
}
