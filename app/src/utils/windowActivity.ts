const OUTSIDE_MOUSE_IGNORE_AFTER_FOCUS_MS = 400;

let isWindowActive = typeof document === "undefined" ? true : document.hasFocus();
let lastWindowActivatedAt = 0;

function now() {
  return typeof performance === "undefined" ? Date.now() : performance.now();
}

if (typeof window !== "undefined") {
  window.addEventListener("focus", () => {
    isWindowActive = true;
    lastWindowActivatedAt = now();
  });

  window.addEventListener("blur", () => {
    isWindowActive = false;
  });
}

export function shouldHandleOutsideMouseDetection() {
  if (typeof document === "undefined") return true;
  if (!isWindowActive || !document.hasFocus()) return false;

  // 窗口从非激活状态切回时，首个鼠标按下通常只是激活窗口，不应被当作弹窗外点击。
  if (lastWindowActivatedAt > 0 && now() - lastWindowActivatedAt < OUTSIDE_MOUSE_IGNORE_AFTER_FOCUS_MS) {
    return false;
  }

  return true;
}
