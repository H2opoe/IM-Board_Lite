import { act, create } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useProfilesStore } from "./useProfilesStore";
import type { ImProfile } from "../model/types";

const mocks = vi.hoisted(() => ({ list: vi.fn() }));
vi.mock("../api/profilesApi", () => ({ listProfiles: mocks.list }));
let store: ReturnType<typeof useProfilesStore>;
let view: ReturnType<typeof create>;
const profile = (id: string): ImProfile => ({ id, platform: "feishu", label: id, enabled: true, configJson: {}, status: "normal", sortOrder: 0, createdAt: "fixture", updatedAt: "fixture" });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: Error) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function Harness() { store = useProfilesStore(); return null; }
beforeEach(() => { vi.resetAllMocks(); vi.stubGlobal("window", new EventTarget()); });
afterEach(() => { act(() => view?.unmount()); vi.unstubAllGlobals(); });

it("does not restore a deleted account when the older initial request completes last", async () => {
  const initial = deferred<ImProfile[]>(); mocks.list.mockReturnValueOnce(initial.promise);
  await act(async () => { view = create(<Harness />); });
  mocks.list.mockResolvedValueOnce([]);
  await act(async () => { await store.refreshProfiles(); });
  await act(async () => { initial.resolve([profile("deleted")]); await initial.promise; });
  expect(store.profiles).toEqual([]);
});

it("retains the displayed list on refresh failure and propagates the error for retry", async () => {
  mocks.list.mockResolvedValueOnce([profile("saved")]);
  await act(async () => { view = create(<Harness />); });
  const stale = deferred<ImProfile[]>(); mocks.list.mockReturnValueOnce(stale.promise);
  let earlier!: Promise<void>;
  act(() => { earlier = store.refreshProfiles(); });
  mocks.list.mockRejectedValueOnce(new Error("fixture"));
  await act(async () => { await expect(store.refreshProfiles()).rejects.toThrow("fixture"); });
  await act(async () => { stale.resolve([profile("stale")]); await earlier; });
  expect(store.profiles.map(p => p.id)).toEqual(["saved"]);
  mocks.list.mockResolvedValueOnce([profile("latest")]);
  await act(async () => { await store.refreshProfiles(); });
  expect(store.profiles.map(p => p.id)).toEqual(["latest"]);
});

it("does not issue another request from a callback retained after unmount", async () => {
  mocks.list.mockResolvedValue([]);
  await act(async () => { view = create(<Harness />); });
  const refresh = store.refreshProfiles;
  act(() => view.unmount());
  const calls = mocks.list.mock.calls.length;
  await refresh();
  window.dispatchEvent(new Event("im-board-profiles-changed"));
  expect(mocks.list).toHaveBeenCalledTimes(calls);
});

it("exposes initial load errors and clears them after an explicit retry", async () => {
  mocks.list.mockRejectedValueOnce(new Error("fixture initial load failed"));
  await act(async () => { view = create(<Harness />); });
  expect(store.profilesError).toBe("fixture initial load failed");
  expect(store.isRefreshing).toBe(false);
  mocks.list.mockResolvedValueOnce([profile("retried")]);
  await act(async () => { await store.retryProfiles(); });
  expect(store.profiles.map(p => p.id)).toEqual(["retried"]);
  expect(store.profilesError).toBe("");
});

it("prevents repeated retry clicks while showing the existing account list", async () => {
  mocks.list.mockResolvedValueOnce([profile("saved")]);
  await act(async () => { view = create(<Harness />); });
  const pending = deferred<ImProfile[]>(); mocks.list.mockReturnValueOnce(pending.promise);
  let first!: Promise<void>; act(() => { first = store.retryProfiles(); });
  expect(store.isRefreshing).toBe(true);
  await store.retryProfiles();
  expect(mocks.list).toHaveBeenCalledTimes(2);
  expect(store.profiles.map(p => p.id)).toEqual(["saved"]);
  await act(async () => { pending.resolve([profile("latest")]); await first; });
  expect(store.isRefreshing).toBe(false);
});

it("does not publish an error from a request superseded by a successful refresh", async () => {
  const old = deferred<ImProfile[]>(); mocks.list.mockReturnValueOnce(old.promise);
  await act(async () => { view = create(<Harness />); });
  mocks.list.mockResolvedValueOnce([profile("latest")]);
  await act(async () => { await store.refreshProfiles(); });
  await act(async () => { old.reject(new Error("stale failure")); });
  expect(store.profilesError).toBe("");
  expect(store.profiles.map(p => p.id)).toEqual(["latest"]);
});
