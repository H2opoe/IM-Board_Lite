const DEMO_MODE_STORAGE_KEY = "im-board-demo-mode";

export function isDemoMode() {
  if (import.meta.env.VITE_IM_BOARD_DEMO === "1") return true;
  const params = new URLSearchParams(window.location.search);
  if (params.get("demo") === "1") {
    window.localStorage.setItem(DEMO_MODE_STORAGE_KEY, "1");
    return true;
  }
  if (params.get("demo") === "0") {
    window.localStorage.removeItem(DEMO_MODE_STORAGE_KEY);
    return false;
  }
  return window.localStorage.getItem(DEMO_MODE_STORAGE_KEY) === "1";
}
