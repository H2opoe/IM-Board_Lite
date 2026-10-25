import { act, create } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useOfficialCliBindController } from "./useOfficialCliBindController";
import type { ImProfile } from "../model/types";

const mocks = vi.hoisted(() => ({ deploy: vi.fn(), version: vi.fn(), update: vi.fn(), copy: vi.fn(), verify: vi.fn(), identity: vi.fn(), duplicate: vi.fn(), save: vi.fn(), release: vi.fn() }));
vi.mock("../../../api/bridgeApi", () => ({ deployPlatformBridge: mocks.deploy, checkPlatformCliUpdate: mocks.version, updatePlatformCli: mocks.update }));
vi.mock("../../../api/profileReadApi", () => ({ verifyOfficialCliAuthorization: mocks.verify }));
vi.mock("../api/profilesApi", () => ({ upsertProfile: mocks.save, releaseProfileDraft: mocks.release }));
vi.mock("../model/accountIdentity", () => ({ readBoundAccountIdentity: mocks.identity, findDuplicateAccountIdentity: mocks.duplicate, withAccountIdentity: (profile: ImProfile) => profile }));

let controller: ReturnType<typeof useOfficialCliBindController>;
let view: ReturnType<typeof create>;
let request: number;
const message = vi.fn();
const stop = vi.fn();
const watch = vi.fn();
const changed = vi.fn();
const busy = vi.fn();
const copied = vi.fn();
const resetCopied = vi.fn();
const tasks: Promise<void>[] = [];
const profile = (id: string): ImProfile => ({ id, platform: "wecom", label: id, enabled: true, configJson: {}, status: "normal", sortOrder: 0, createdAt: "fixture", updatedAt: "fixture" });
function deferred<T>() { let resolve!: (value: T) => void; let reject!: (error: Error) => void; const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; }
function Harness() {
  controller = useOfficialCliBindController({
    orderedProfiles: [], onProfilesChange: changed, onDuplicateProfileReplaced: vi.fn(),
    cancelDeploymentRequest: () => { request += 1; }, isDeploymentRequestActive: id => id === request,
    nextDeploymentRequest: () => ++request, preservePersistedProfileState: (_, next) => next,
    resetCliDeploymentProgress: vi.fn(), resetCopiedCommandStatus: resetCopied, markCopiedCommand: copied,
    runAfterModalPaint: task => { tasks.push(Promise.resolve(task())); }, setFormMessage: message,
    setUpdatingCliPlatform: busy, shouldHandleModalEnter: () => true, watchDeploymentProgressFor: watch
  });
  return null;
}
async function open(id: string) { await act(async () => { controller.openOfficialCliSetup("wecom", profile(id)); }); }
beforeEach(async () => {
  vi.resetAllMocks(); mocks.release.mockResolvedValue(undefined); request = 0; tasks.length = 0;
  mocks.copy.mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText: mocks.copy } });
  mocks.deploy.mockResolvedValue({ platform: "wecom", cliPath: "/fixture/cli", configDir: "/fixture/config", command: "fixture", commandShell: "fixture", source: "fixture", currentVersion: "1" });
  mocks.version.mockResolvedValue({ currentVersion: "1", latestVersion: "1" });
  mocks.update.mockResolvedValue({ currentVersion: "2", latestVersion: "2", updateAvailable: false });
  mocks.verify.mockResolvedValue(undefined); mocks.identity.mockResolvedValue({ accountId: "fixture" });
  mocks.duplicate.mockResolvedValue(null); mocks.save.mockImplementation(async value => value);
  watch.mockResolvedValue(stop); changed.mockResolvedValue(undefined);
  await act(async () => { view = create(<Harness />); });
});
afterEach(async () => { act(() => view.unmount()); await Promise.allSettled(tasks); vi.unstubAllGlobals(); });

it("does not persist a cancelled authorization result or close the replacement session", async () => {
  const authorization = deferred<void>(); mocks.verify.mockReturnValueOnce(authorization.promise);
  await open("old"); let saving!: Promise<void>;
  act(() => { saving = controller.saveOfficialCliProfile(); });
  act(() => controller.closeOfficialCliSetup()); await open("new");
  await act(async () => { authorization.resolve(); await saving; });
  expect(mocks.save).not.toHaveBeenCalled(); expect(controller.officialCliFlow.profile?.id).toBe("new");
});
it("starts only one save for repeated clicks before React rerenders", async () => {
  const authorization = deferred<void>(); mocks.verify.mockReturnValue(authorization.promise);
  await open("one"); let first!: Promise<void>; let second!: Promise<void>;
  act(() => { first = controller.saveOfficialCliProfile(); second = controller.saveOfficialCliProfile(); });
  expect(mocks.verify).toHaveBeenCalledTimes(1);
  await act(async () => { authorization.resolve(); await Promise.all([first, second]); });
  expect(mocks.save).toHaveBeenCalledTimes(1); expect(changed).toHaveBeenCalledTimes(1);
});
it("ignores a late save completion without clearing the new window", async () => {
  const write = deferred<ImProfile>(); mocks.save.mockReturnValueOnce(write.promise);
  await open("old"); let saving!: Promise<void>;
  await act(async () => { saving = controller.saveOfficialCliProfile(); });
  expect(mocks.save).toHaveBeenCalledTimes(1);
  act(() => controller.closeOfficialCliSetup()); await open("new");
  await act(async () => { write.resolve(profile("old")); await saving; });
  expect(controller.officialCliFlow.profile?.id).toBe("new"); expect(changed).toHaveBeenCalledTimes(1);
});
it("handles subscription failure without leaving deployment stuck", async () => {
  watch.mockRejectedValueOnce(new Error("fixture subscription failure"));
  await open("one"); await Promise.all(tasks);
  expect(controller.officialCliFlow.isDeployingBridge).toBe(false);
  expect(mocks.deploy).not.toHaveBeenCalled(); expect(message).toHaveBeenCalledWith(expect.stringContaining("fixture subscription failure"));
});
it("releases a late subscription after cancellation before deploying", async () => {
  const registration = deferred<() => void>(); watch.mockReturnValueOnce(registration.promise);
  await open("old"); act(() => controller.closeOfficialCliSetup());
  await act(async () => { registration.resolve(stop); await Promise.all(tasks); });
  expect(stop).toHaveBeenCalledTimes(1); expect(mocks.deploy).not.toHaveBeenCalled();
});
it("does not apply late deployment completion after unmount", async () => {
  const deployment = deferred<unknown>(); mocks.deploy.mockReturnValueOnce(deployment.promise);
  await open("old"); act(() => view.unmount());
  await act(async () => { deployment.resolve({ cliPath: "/fixture/late" }); await Promise.all(tasks); });
  expect(mocks.version).not.toHaveBeenCalled(); expect(stop).toHaveBeenCalledTimes(1);
});

it("releases replaced, closed and unmounted drafts without touching a newer session", async () => {
  await open("first"); await open("second");
  expect(mocks.release.mock.calls).toEqual([["first"]]);
  act(() => controller.closeOfficialCliSetup()); act(() => controller.closeOfficialCliSetup());
  expect(mocks.release.mock.calls).toEqual([["first"],["second"]]);
  await open("third"); act(() => view.unmount());
  expect(mocks.release.mock.calls).toEqual([["first"],["second"],["third"]]);
});

const deploymentFor = (id: string) => ({ platform: "wecom", cliPath: `/fixture/${id}/cli`, configDir: `/fixture/${id}/config`, command: `authorize-${id}`, commandShell: "fixture", source: "fixture", currentVersion: "2" });

it("ignores an old update result before redeploying after another account opens", async () => {
  const update = deferred<unknown>(); mocks.update.mockReturnValueOnce(update.promise);
  await open("old"); let updating!: Promise<void>;
  act(() => { updating = controller.updateOfficialCli("wecom"); });
  act(() => controller.closeOfficialCliSetup()); await open("new");
  const newDeployment = controller.officialCliFlow.deployment;
  const deployCount = mocks.deploy.mock.calls.length;
  await act(async () => { update.resolve({ currentVersion: "2", latestVersion: "2", updateAvailable: false }); await updating; });
  expect(mocks.deploy).toHaveBeenCalledTimes(deployCount);
  expect(controller.officialCliFlow.profile?.id).toBe("new");
  expect(controller.officialCliFlow.deployment).toBe(newDeployment);
});

it.each(["new", "old"])("does not apply a late redeployment to the replacement %s account session", async (replacementId) => {
  await open("old"); const deployment = deferred<unknown>(); mocks.deploy.mockReturnValueOnce(deployment.promise);
  let updating!: Promise<void>;
  await act(async () => { updating = controller.updateOfficialCli("wecom"); });
  expect(mocks.deploy).toHaveBeenLastCalledWith("wecom", "old");
  act(() => controller.closeOfficialCliSetup()); await open(replacementId);
  const newDeployment = controller.officialCliFlow.deployment;
  const busyWrites = busy.mock.calls.length;
  await act(async () => { deployment.resolve(deploymentFor("old-updated")); await updating; });
  expect(controller.officialCliFlow.profile?.id).toBe(replacementId);
  expect(controller.officialCliFlow.deployment).toBe(newDeployment);
  expect(busy).toHaveBeenCalledTimes(busyWrites);
});

it.each(["update", "redeploy"])("ignores a late %s failure and preserves a newer update's busy state", async (phase) => {
  await open("old"); const oldWork = deferred<unknown>();
  if (phase === "update") mocks.update.mockReturnValueOnce(oldWork.promise);
  else mocks.deploy.mockReturnValueOnce(oldWork.promise);
  let oldUpdating!: Promise<void>;
  await act(async () => { oldUpdating = controller.updateOfficialCli("wecom"); });
  act(() => controller.closeOfficialCliSetup()); await open("new");
  const newWork = deferred<unknown>(); mocks.update.mockReturnValueOnce(newWork.promise);
  let newUpdating!: Promise<void>;
  act(() => { newUpdating = controller.updateOfficialCli("wecom"); });
  expect(busy).toHaveBeenLastCalledWith("wecom");
  message.mockClear(); const busyWrites = busy.mock.calls.length;
  await act(async () => { oldWork.reject(new Error("old failure")); await oldUpdating; });
  expect(message).not.toHaveBeenCalled(); expect(busy).toHaveBeenCalledTimes(busyWrites);
  expect(busy).toHaveBeenLastCalledWith("wecom");
  mocks.deploy.mockResolvedValueOnce(deploymentFor("new-updated"));
  await act(async () => { newWork.resolve({ currentVersion: "2", latestVersion: "2", updateAvailable: false }); await newUpdating; });
  expect(controller.officialCliFlow.deployment?.command).toBe("authorize-new-updated");
  expect(busy).toHaveBeenLastCalledWith(null);
});

it("starts only one update for repeated clicks before React rerenders", async () => {
  await open("one"); const update = deferred<unknown>(); mocks.update.mockReturnValueOnce(update.promise);
  let first!: Promise<void>; let second!: Promise<void>;
  act(() => { first = controller.updateOfficialCli("wecom"); second = controller.updateOfficialCli("wecom"); });
  expect(mocks.update).toHaveBeenCalledTimes(1);
  await act(async () => { update.resolve({ currentVersion: "2", latestVersion: "2", updateAvailable: false }); await Promise.all([first, second]); });
  expect(mocks.deploy).toHaveBeenCalledTimes(2);
});

it("reports a current update failure and permits a retry", async () => {
  await open("one"); mocks.update.mockRejectedValueOnce(new Error("current failure"));
  await act(async () => { await controller.updateOfficialCli("wecom"); });
  expect(message).toHaveBeenLastCalledWith(expect.stringContaining("current failure"));
  expect(busy).toHaveBeenLastCalledWith(null);
  mocks.deploy.mockResolvedValueOnce(deploymentFor("one-updated"));
  await act(async () => { await controller.updateOfficialCli("wecom"); });
  expect(mocks.update).toHaveBeenCalledTimes(2);
  expect(controller.officialCliFlow.deployment?.command).toBe("authorize-one-updated");
});

it("does not redeploy or mutate page notices after unmount during an update", async () => {
  await open("one"); const update = deferred<unknown>(); mocks.update.mockReturnValueOnce(update.promise);
  let updating!: Promise<void>;
  act(() => { updating = controller.updateOfficialCli("wecom"); }); act(() => view.unmount());
  message.mockClear(); const busyWrites = busy.mock.calls.length;
  await act(async () => { update.resolve({ currentVersion: "2", latestVersion: "2", updateAvailable: false }); await updating; });
  expect(mocks.deploy).toHaveBeenCalledTimes(1); expect(message).not.toHaveBeenCalled(); expect(busy).toHaveBeenCalledTimes(busyWrites);
});

it.each(["new", "old"])("ignores clipboard completion after closing and opening the %s account session", async (replacementId) => {
  mocks.deploy.mockImplementation(async (_platform, id) => deploymentFor(id));
  await open("old"); const write = deferred<void>(); mocks.copy.mockReturnValueOnce(write.promise);
  let copying!: Promise<void>;
  act(() => { copying = controller.copyOfficialCliCommand(); });
  expect(mocks.copy).toHaveBeenCalledWith("authorize-old");
  act(() => controller.closeOfficialCliSetup()); await open(replacementId);
  copied.mockClear(); resetCopied.mockClear(); message.mockClear();
  await act(async () => { write.resolve(); await copying; });
  expect(copied).not.toHaveBeenCalled(); expect(resetCopied).not.toHaveBeenCalled(); expect(message).not.toHaveBeenCalled();
  expect(controller.officialCliFlow.profile?.id).toBe(replacementId);
  expect(mocks.copy).toHaveBeenCalledTimes(1);
});

it("leaves a closed binding window unchanged when its clipboard write settles", async () => {
  await open("old"); const write = deferred<void>(); mocks.copy.mockReturnValueOnce(write.promise);
  let copying!: Promise<void>;
  act(() => { copying = controller.copyOfficialCliCommand(); });
  act(() => controller.closeOfficialCliSetup()); copied.mockClear(); resetCopied.mockClear(); message.mockClear();
  await act(async () => { write.resolve(); await copying; });
  expect(controller.officialCliFlow.profile).toBeNull();
  expect(copied).not.toHaveBeenCalled(); expect(resetCopied).not.toHaveBeenCalled(); expect(message).not.toHaveBeenCalled();
});

it.each(["success", "failure"])("allows a new session to copy independently of an older pending %s", async (oldOutcome) => {
  mocks.deploy.mockImplementation(async (_platform, id) => deploymentFor(id));
  await open("old"); const oldWrite = deferred<void>(); const newWrite = deferred<void>();
  mocks.copy.mockReturnValueOnce(oldWrite.promise).mockReturnValueOnce(newWrite.promise);
  let oldCopying!: Promise<void>; let newCopying!: Promise<void>;
  act(() => { oldCopying = controller.copyOfficialCliCommand(); });
  act(() => controller.closeOfficialCliSetup()); await open("new");
  act(() => { newCopying = controller.copyOfficialCliCommand(); });
  expect(mocks.copy.mock.calls).toEqual([["authorize-old"], ["authorize-new"]]);
  copied.mockClear(); resetCopied.mockClear(); message.mockClear();
  await act(async () => { newWrite.resolve(); await newCopying; });
  expect(copied).toHaveBeenCalledExactlyOnceWith("wecom");
  expect(message).toHaveBeenCalledExactlyOnceWith("");
  copied.mockClear(); resetCopied.mockClear(); message.mockClear();
  await act(async () => {
    if (oldOutcome === "success") oldWrite.resolve(); else oldWrite.reject(new Error("old clipboard failure"));
    await oldCopying;
  });
  expect(copied).not.toHaveBeenCalled(); expect(resetCopied).not.toHaveBeenCalled(); expect(message).not.toHaveBeenCalled();
});

it("does not show an old clipboard failure in a newly opened session", async () => {
  await open("old"); const write = deferred<void>(); mocks.copy.mockReturnValueOnce(write.promise);
  let copying!: Promise<void>;
  act(() => { copying = controller.copyOfficialCliCommand(); });
  act(() => controller.closeOfficialCliSetup()); await open("new");
  copied.mockClear(); resetCopied.mockClear(); message.mockClear();
  await act(async () => { write.reject(new Error("old clipboard failure")); await copying; });
  expect(copied).not.toHaveBeenCalled(); expect(resetCopied).not.toHaveBeenCalled(); expect(message).not.toHaveBeenCalled();
});

it("keeps the latest copy result when an earlier write in the same session fails", async () => {
  await open("one"); const oldWrite = deferred<void>(); mocks.copy.mockReturnValueOnce(oldWrite.promise);
  let oldCopying!: Promise<void>;
  act(() => { oldCopying = controller.copyOfficialCliCommand(); });
  await act(async () => { await controller.copyOfficialCliCommand(); });
  expect(copied).toHaveBeenCalledExactlyOnceWith("wecom");
  copied.mockClear(); resetCopied.mockClear(); message.mockClear();
  await act(async () => { oldWrite.reject(new Error("earlier copy failed")); await oldCopying; });
  expect(copied).not.toHaveBeenCalled(); expect(resetCopied).not.toHaveBeenCalled(); expect(message).not.toHaveBeenCalled();
  expect(mocks.copy).toHaveBeenCalledTimes(2);
});

it("reports a current clipboard failure and allows another copy attempt", async () => {
  await open("one"); mocks.copy.mockRejectedValueOnce(new Error("current clipboard failure"));
  resetCopied.mockClear(); copied.mockClear(); message.mockClear();
  await act(async () => { await controller.copyOfficialCliCommand(); });
  expect(resetCopied).toHaveBeenCalledTimes(1);
  expect(message).toHaveBeenLastCalledWith(expect.stringContaining("current clipboard failure"));
  await act(async () => { await controller.copyOfficialCliCommand(); });
  expect(mocks.copy).toHaveBeenCalledTimes(2);
  expect(copied).toHaveBeenCalledExactlyOnceWith("wecom");
  expect(message).toHaveBeenLastCalledWith("");
});

it("shows the current unsupported clipboard error without making a native write", async () => {
  await open("one"); vi.stubGlobal("navigator", { clipboard: undefined });
  await act(async () => { await controller.copyOfficialCliCommand(); });
  expect(mocks.copy).not.toHaveBeenCalled(); expect(copied).not.toHaveBeenCalled();
  expect(message).toHaveBeenLastCalledWith(expect.stringContaining("不支持自动复制"));
});

it("does not apply clipboard completion after unmount", async () => {
  await open("one"); const write = deferred<void>(); mocks.copy.mockReturnValueOnce(write.promise);
  let copying!: Promise<void>;
  act(() => { copying = controller.copyOfficialCliCommand(); }); act(() => view.unmount());
  copied.mockClear(); resetCopied.mockClear(); message.mockClear();
  await act(async () => { write.resolve(); await copying; });
  expect(copied).not.toHaveBeenCalled(); expect(resetCopied).not.toHaveBeenCalled(); expect(message).not.toHaveBeenCalled();
});

