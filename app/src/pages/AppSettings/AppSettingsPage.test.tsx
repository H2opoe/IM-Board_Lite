import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AppSettingsPage } from "./AppSettingsPage";

const api = vi.hoisted(() => ({ get: vi.fn(), save: vi.fn(), export: vi.fn(), dialog: vi.fn() }));
vi.mock("../../api/appSettingsApi", () => ({ getAppSettings: api.get, saveAppSettings: api.save, exportDiagnosticPackage: api.export }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: api.dialog }));
vi.mock("../../components/AboutModal", () => ({ AboutModal: () => null }));
vi.mock("../../components/DeveloperFeedbackModal", () => ({ DeveloperFeedbackModal: () => null }));
vi.mock("../../components/shared/FloatingNotice", () => ({ FloatingNotice: ({ message }: { message: string }) => <p>{message}</p>, FloatingNoticeStack: ({ children }: { children: React.ReactNode }) => <div>{children}</div> }));
let view: ReactTestRenderer;
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
const saveButton = () => view.root.findAllByType("button").find(button => button.props.className === "primary-button")!;
const exportButton = () => view.root.findAllByType("button").find(button => button.children.includes("导出诊断包"))!;
beforeEach(() => { vi.resetAllMocks(); api.get.mockResolvedValue({ cacheClearTime: "01:00" }); vi.stubGlobal("window", new EventTarget()); });
afterEach(() => { act(() => view?.unmount()); vi.unstubAllGlobals(); });

it("locks edits and all mutation entries while saving and rejects repeated activation", async () => {
  const response = deferred<{ cacheClearTime: string }>(); api.save.mockReturnValue(response.promise);
  await act(async () => { view = create(<AppSettingsPage />); });
  const save = saveButton().props.onClick;
  const exportDiagnostics = exportButton().props.onClick;
  let pending!: Promise<void>; act(() => { pending = save(); void save(); void exportDiagnostics(); });
  expect(api.save).toHaveBeenCalledTimes(1);
  expect(api.dialog).not.toHaveBeenCalled();
  const input = view.root.findByType("input");
  expect(input.props.disabled).toBe(true);
  act(() => input.props.onChange({ target: { value: "02:00" } }));
  expect(view.root.findByType("input").props.value).toBe("01:00");
  await act(async () => { response.resolve({ cacheClearTime: "01:00" }); await pending; });
  expect(view.root.findByType("input").props.value).toBe("01:00");
  expect(saveButton().props.disabled).toBe(false);
});

it("claims the export lock before opening a native save dialog and releases it on cancellation", async () => {
  const dialog = deferred<string | null>(); api.dialog.mockReturnValue(dialog.promise);
  await act(async () => { view = create(<AppSettingsPage />); });
  const exportDiagnostics = exportButton().props.onClick; const save = saveButton().props.onClick;
  let pending!: Promise<void>; act(() => { pending = exportDiagnostics(); void exportDiagnostics(); void save(); });
  expect(api.dialog).toHaveBeenCalledTimes(1);
  expect(api.save).not.toHaveBeenCalled();
  expect(view.root.findByType("input").props.disabled).toBe(true);
  await act(async () => { dialog.resolve(null); await pending; });
  expect(api.export).not.toHaveBeenCalled();
  expect(saveButton().props.disabled).toBe(false);
});

it("does not begin an export if the dialog returns after settings unmounts", async () => {
  const dialog = deferred<string | null>(); api.dialog.mockReturnValue(dialog.promise);
  await act(async () => { view = create(<AppSettingsPage />); });
  let pending!: Promise<void>; act(() => { pending = exportButton().props.onClick(); });
  act(() => view.unmount());
  await act(async () => { dialog.resolve("fixture.zip"); await pending; });
  expect(api.export).not.toHaveBeenCalled();
});

it("shows save errors and lets the user retry", async () => {
  api.save.mockRejectedValueOnce(new Error("fixture save failed")).mockResolvedValueOnce({ cacheClearTime: "01:00" });
  await act(async () => { view = create(<AppSettingsPage />); });
  await act(async () => { await saveButton().props.onClick(); });
  expect(view.root.findAllByType("p").some(node => node.children.includes("fixture save failed"))).toBe(true);
  expect(saveButton().props.disabled).toBe(false);
  await act(async () => { await saveButton().props.onClick(); });
  expect(api.save).toHaveBeenCalledTimes(2);
});

it("ignores an older settings load after that mount has ended", async () => {
  const old = deferred<{ cacheClearTime: string }>(); api.get.mockReturnValueOnce(old.promise).mockResolvedValueOnce({ cacheClearTime: "03:00" });
  await act(async () => { view = create(<AppSettingsPage />); });
  act(() => view.unmount());
  await act(async () => { view = create(<AppSettingsPage />); });
  await act(async () => { old.resolve({ cacheClearTime: "00:00" }); await old.promise; });
  expect(view.root.findByType("input").props.value).toBe("03:00");
});
