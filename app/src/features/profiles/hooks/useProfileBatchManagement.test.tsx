import { act, create } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useProfileBatchManagement } from "./useProfileBatchManagement";
import type { ImProfile } from "../model/types";
const mocks = vi.hoisted(() => ({ save: vi.fn(), remove: vi.fn(), refresh: vi.fn(), notice: vi.fn(), reset: vi.fn() }));
vi.mock("../api/profilesApi", () => ({ upsertProfile: mocks.save, deleteProfile: mocks.remove }));
const profiles: ImProfile[] = ["one", "two", "three"].map(id => ({ id, platform: "wecom", label: id, enabled: true, configJson: {}, status: "normal", sortOrder: 0, createdAt: "fixture", updatedAt: "version" }));
let controller: ReturnType<typeof useProfileBatchManagement>;
let view: ReturnType<typeof create>;
function Harness() {
  controller = useProfileBatchManagement({ isBatchManaging: true, orderedProfiles: profiles, draggedProfileId: null, onProfilesChange: mocks.refresh, resetProfileDragState: mocks.reset, setIsBatchManaging: vi.fn(), setProfileNotice: mocks.notice, clearPendingDelete: vi.fn() });
  return null;
}
beforeEach(async () => {
  vi.resetAllMocks(); mocks.refresh.mockResolvedValue(undefined);
  await act(async () => { view = create(<Harness />); });
  act(() => controller.toggleAllProfilesSelected());
});
afterEach(() => act(() => view.unmount()));
it("waits for every status result, blocks duplicate submissions, and refreshes partial success", async () => {
  let resolve!: () => void;
  mocks.save.mockResolvedValueOnce(profiles[0]).mockRejectedValueOnce(new Error("conflict")).mockReturnValueOnce(new Promise<void>(yes => { resolve = yes; }));
  let task!: Promise<void>;
  await act(async () => { task = controller.setSelectedProfilesEnabled(false); });
  expect(mocks.refresh).not.toHaveBeenCalled();
  await act(async () => { await controller.setSelectedProfilesEnabled(false); await controller.handleBulkDeleteProfiles(); });
  expect(mocks.save).toHaveBeenCalledTimes(3);
  expect(mocks.remove).not.toHaveBeenCalled();
  await act(async () => { resolve(); await task; });
  expect(mocks.refresh).toHaveBeenCalledTimes(1);
  expect(mocks.notice).toHaveBeenLastCalledWith(expect.stringContaining("完成 2 个，未完成 1 个"), true);
  expect(mocks.save.mock.calls[0][0].updatedAt).toBe("version");
});
it("continues deletion after a failure and retries only failed accounts", async () => {
  mocks.remove.mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error("busy")).mockResolvedValueOnce(undefined);
  act(() => { void controller.handleBulkDeleteProfiles(); });
  await act(async () => { await controller.handleBulkDeleteProfiles(); });
  expect(mocks.remove.mock.calls).toEqual([["one"], ["two"], ["three"]]);
  expect(controller.pendingBulkDeleteIds).toEqual(["two"]);
  expect([...controller.selectedProfileIds]).toEqual(["two"]);
  expect(mocks.refresh).toHaveBeenCalledTimes(1);
  expect(mocks.notice).toHaveBeenLastCalledWith(expect.stringContaining("完成 2 个，未完成 1 个"), true);
  mocks.remove.mockResolvedValue(undefined);
  await act(async () => { await controller.handleBulkDeleteProfiles(); });
  expect(mocks.remove.mock.calls).toEqual([["one"], ["two"], ["three"], ["two"]]);
  expect(controller.pendingBulkDeleteIds).toEqual([]);
});
it("distinguishes successful writes from a failed list refresh", async () => {
  mocks.refresh.mockRejectedValue(new Error("offline"));
  await act(async () => { await controller.setSelectedProfilesEnabled(false); });
  expect(mocks.save).toHaveBeenCalledTimes(3);
  expect(mocks.notice).toHaveBeenLastCalledWith(expect.stringContaining("刷新账号列表失败"), true);
  expect(mocks.notice.mock.lastCall?.[0]).toContain("3");
});
it("does not execute confirmed deletion twice while the first call is pending", async () => {
  let resolve!: () => void;
  mocks.remove.mockReturnValueOnce(new Promise<void>(yes => { resolve = yes; }));
  act(() => { void controller.handleBulkDeleteProfiles(); });
  let task: Promise<void> | undefined;
  await act(async () => { task = controller.handleBulkDeleteProfiles(); });
  await act(async () => { await controller.handleBulkDeleteProfiles(); });
  expect(mocks.remove).toHaveBeenCalledTimes(1);
  await act(async () => { resolve(); await task; });
  expect(mocks.remove).toHaveBeenCalledTimes(3);
  expect(mocks.refresh).toHaveBeenCalledTimes(1);
});
