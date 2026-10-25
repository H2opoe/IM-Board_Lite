import { act, create } from "react-test-renderer";
import { afterEach, expect, it, vi } from "vitest";
import { AppShell } from "./AppShell";

afterEach(() => vi.unstubAllGlobals());
it("shows account load failures with an explicit retry entry", () => {
  vi.stubGlobal("window", new EventTarget());
  const retry = vi.fn();
  const props = { profiles: [], profilesError: "fixture account load failed", isRefreshingProfiles: false, onReloadProfiles: retry, activeProfileId: "aggregate", activeView: "dashboard" as const, drawerItem: null, onSelectProfile: vi.fn(), onSelectView: vi.fn(), onCloseDrawer: vi.fn(), themeChoice: "dark" as const, effectiveThemeMode: "dark" as const, onThemeChange: vi.fn(), children: null };
  let view!: ReturnType<typeof create>;
  act(() => { view = create(<AppShell {...props} />); });
  try {
    const alert = view.root.findByProps({ role: "alert" });
    expect(alert.findByType("p").children).toEqual(["fixture account load failed"]);
    const button = alert.findByType("button");
    expect(button.children).toEqual(["重新加载账号"]);
    act(() => button.props.onClick());
    expect(retry).toHaveBeenCalledTimes(1);
    act(() => view.update(<AppShell {...props} isRefreshingProfiles />));
    expect(view.root.findByProps({ role: "alert" }).findByType("button").props.disabled).toBe(true);
    act(() => view.update(<AppShell {...props} profilesError="" />));
    expect(view.root.findAllByProps({ role: "alert" })).toHaveLength(0);
  } finally { act(() => view.unmount()); }
});
