import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useDashboardActions } from "./useDashboardActions";
import type { ActionItem, DashboardData } from "../model/types";

const api = vi.hoisted(() => ({ mark: vi.fn(), dashboard: vi.fn() }));
vi.mock("../api/dashboardApi", () => ({ markActionItem: api.mark, getDashboard: api.dashboard }));
let view: ReactTestRenderer;
let actions: ReturnType<typeof useDashboardActions>;
const change = vi.fn();
const item = { id: "fixture-action" } as ActionItem;
const data = (marker: string) => ({ day: marker }) as DashboardData;
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
function Harness({ profile = "A" }: { profile?: string }) { actions = useDashboardActions(profile, change); return null; }
beforeEach(() => { vi.resetAllMocks(); api.mark.mockResolvedValue(undefined); });
afterEach(() => act(() => view?.unmount()));

it("publishes the current account after complete and reopen", async () => {
  api.dashboard.mockResolvedValue(data("current"));
  act(() => { view = create(<Harness />); });
  await act(async () => { await actions.complete(item); await actions.reopen(item); });
  expect(api.mark.mock.calls).toEqual([[item.id, "done"], [item.id, "open"]]);
  expect(api.dashboard.mock.calls).toEqual([["A"], ["A"]]);
  expect(change).toHaveBeenCalledTimes(2);
});

it("keeps an already sent mark but skips an old account refresh after navigation", async () => {
  const mark = deferred<void>(); api.mark.mockReturnValue(mark.promise);
  act(() => { view = create(<Harness />); });
  let pending!: Promise<void>; act(() => { pending = actions.complete(item); });
  act(() => view.update(<Harness profile="B" />));
  await act(async () => { mark.resolve(); await pending; });
  expect(api.mark).toHaveBeenCalledTimes(1);
  expect(api.dashboard).not.toHaveBeenCalled();
  expect(change).not.toHaveBeenCalled();
});

it("discards a dashboard response from the previous account", async () => {
  const snapshot = deferred<DashboardData>(); api.dashboard.mockReturnValue(snapshot.promise);
  act(() => { view = create(<Harness />); });
  let pending!: Promise<void>; await act(async () => { pending = actions.complete(item); });
  act(() => view.update(<Harness profile="B" />));
  await act(async () => { snapshot.resolve(data("old")); await pending; });
  expect(change).not.toHaveBeenCalled();
});

it("does not refresh or write UI state after unmount", async () => {
  const mark = deferred<void>(); api.mark.mockReturnValue(mark.promise);
  act(() => { view = create(<Harness />); });
  let pending!: Promise<void>; act(() => { pending = actions.complete(item); });
  act(() => view.unmount());
  await act(async () => { mark.resolve(); await pending; });
  expect(api.dashboard).not.toHaveBeenCalled();
  expect(change).not.toHaveBeenCalled();
});

it("the newest snapshot wins when different action reads complete in reverse order", async () => {
  const old = deferred<DashboardData>(); api.dashboard.mockReturnValueOnce(old.promise).mockResolvedValueOnce(data("latest"));
  act(() => { view = create(<Harness />); });
  let earlier!: Promise<void>; await act(async () => { earlier = actions.complete(item); });
  await act(async () => { await actions.complete({ id: "second" } as ActionItem); });
  await act(async () => { old.resolve(data("old")); await earlier; });
  expect(api.mark).toHaveBeenCalledTimes(2);
  expect(change.mock.calls).toEqual([[data("latest")]]);
});

it("a callback retained from the previous account cannot send another mark", async () => {
  act(() => { view = create(<Harness />); });
  const oldComplete = actions.complete;
  act(() => view.update(<Harness profile="B" />));
  await oldComplete(item);
  expect(api.mark).not.toHaveBeenCalled();
});

it("exposes backend errors to void consumers and retries without replacing current data", async () => {
  api.mark.mockRejectedValueOnce(new Error("fixture failure")).mockResolvedValueOnce(undefined);
  api.dashboard.mockResolvedValue(data("retried"));
  act(() => { view = create(<Harness />); });
  await act(async () => { await actions.complete(item); });
  expect(actions.actionError).toBe("fixture failure");
  expect(change).not.toHaveBeenCalled();
  await act(async () => { await actions.retryAction(); });
  expect(actions.actionError).toBe("");
  expect(change).toHaveBeenCalledWith(data("retried"));
});

it("refreshes again when a slower different action finishes its mark last", async () => {
  const firstMark = deferred<void>();
  api.mark.mockReturnValueOnce(firstMark.promise).mockResolvedValueOnce(undefined);
  api.dashboard.mockResolvedValueOnce(data("second-only")).mockResolvedValueOnce(data("both-completed"));
  act(() => { view = create(<Harness />); });
  let first!: Promise<void>; act(() => { first = actions.complete(item); });
  await act(async () => { await actions.complete({ id: "second" } as ActionItem); });
  await act(async () => { firstMark.resolve(); await first; });
  expect(api.dashboard).toHaveBeenCalledTimes(2);
  expect(change.mock.calls).toEqual([[data("second-only")], [data("both-completed")]]);
});

it("retries only reading after a successful mark whose dashboard refresh failed", async () => {
  api.dashboard.mockRejectedValueOnce(new Error("fixture read failed")).mockResolvedValueOnce(data("retried-read"));
  act(() => { view = create(<Harness />); });
  await act(async () => { await actions.complete(item); });
  expect(actions.actionError).toBe("fixture read failed");
  await act(async () => { await actions.retryAction(); });
  expect(api.mark).toHaveBeenCalledTimes(1);
  expect(api.dashboard).toHaveBeenCalledTimes(2);
  expect(change).toHaveBeenCalledWith(data("retried-read"));
});

it("a previous account failure does not publish an error or retry on the new account", async () => {
  let reject!: (error: Error) => void;
  api.mark.mockReturnValue(new Promise<void>((_, no) => { reject = no; }));
  act(() => { view = create(<Harness />); });
  let first!: Promise<void>; act(() => { first = actions.complete(item); });
  act(() => view.update(<Harness profile="B" />));
  await act(async () => { reject(new Error("old account failure")); await first; });
  expect(actions.actionError).toBe("");
  await actions.retryAction();
  expect(api.mark).toHaveBeenCalledTimes(1);
});
