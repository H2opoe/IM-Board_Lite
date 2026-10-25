import { afterEach, expect, it, vi } from "vitest";

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });
it("focus activation clicks are ignored, later deliberate clicks close outside UI", async () => {
  vi.resetModules();
  let time = 1000;
  let focused = true;
  const listeners = new Map<string, () => void>();
  vi.stubGlobal("window", { addEventListener: (event: string, listener: () => void) => listeners.set(event, listener) });
  vi.stubGlobal("document", { hasFocus: () => focused });
  vi.spyOn(performance, "now").mockImplementation(() => time);
  const { shouldHandleOutsideMouseDetection } = await import("./windowActivity");
  expect(shouldHandleOutsideMouseDetection()).toBe(true);
  focused = false; listeners.get("blur")!();
  expect(shouldHandleOutsideMouseDetection()).toBe(false);
  focused = true; listeners.get("focus")!();
  expect(shouldHandleOutsideMouseDetection()).toBe(false);
  time += 399;
  expect(shouldHandleOutsideMouseDetection()).toBe(false);
  time += 2;
  expect(shouldHandleOutsideMouseDetection()).toBe(true);
});
