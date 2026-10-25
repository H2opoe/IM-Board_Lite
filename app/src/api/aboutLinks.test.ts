import { beforeEach, expect, it, vi } from "vitest";
const state = vi.hoisted(() => ({ desktop: true, invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: state.invoke }));
vi.mock("./tauri", () => ({ get isTauri() { return state.desktop; } }));
import { openAboutLink } from "./aboutLinks";
beforeEach(() => { state.desktop = true; state.invoke.mockReset(); });
it("dispatches both desktop destinations and prevents embedded navigation", async () => {
  for (const target of ["profile", "email"] as const) {
    const event = { preventDefault: vi.fn() };
    await openAboutLink(event, target);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(state.invoke).toHaveBeenLastCalledWith("open_about_link", { target });
  }
});
it("keeps ordinary browser anchor behavior without calling native commands", async () => {
  state.desktop = false;
  const event = { preventDefault: vi.fn() };
  await openAboutLink(event, "profile");
  expect(event.preventDefault).not.toHaveBeenCalled();
  expect(state.invoke).not.toHaveBeenCalled();
});
it("propagates native open failures so the view can show guidance", async () => {
  state.invoke.mockRejectedValue(new Error("no mail handler"));
  await expect(openAboutLink({ preventDefault: vi.fn() }, "email")).rejects.toThrow("no mail handler");
});
