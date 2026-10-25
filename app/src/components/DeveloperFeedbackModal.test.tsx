import { act, create } from "react-test-renderer";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DeveloperFeedbackModal } from "./DeveloperFeedbackModal";

let listeners: Map<string, Set<(event: { key: string }) => void>>;
let view: ReturnType<typeof create>;
beforeEach(() => {
  listeners = new Map();
  vi.stubGlobal("window", {
    setTimeout: vi.fn(() => 1),
    clearTimeout: vi.fn(),
    addEventListener: (event: string, listener: (event: { key: string }) => void) => {
      if (!listeners.has(event)) listeners.set(event, new Set());
      listeners.get(event)!.add(listener);
    },
    removeEventListener: (event: string, listener: (event: { key: string }) => void) => listeners.get(event)?.delete(listener)
  });
});
afterEach(() => { if (view) act(() => view.unmount()); vi.unstubAllGlobals(); });
function press(key: string) { act(() => { for (const listener of listeners.get("keydown") ?? []) listener({ key }); }); }

it("closes feedback on Escape while other keys leave it open", () => {
  const close = vi.fn();
  act(() => { view = create(<DeveloperFeedbackModal onClose={close} />); });
  press("Enter"); expect(close).not.toHaveBeenCalled();
  press("Escape"); expect(close).toHaveBeenCalledTimes(1);
});
it("uses the latest close callback without adding duplicate key listeners", () => {
  const previous = vi.fn(), current = vi.fn();
  act(() => { view = create(<DeveloperFeedbackModal onClose={previous} />); });
  act(() => view.update(<DeveloperFeedbackModal onClose={current} />));
  expect(listeners.get("keydown")?.size).toBe(1);
  press("Escape"); expect(previous).not.toHaveBeenCalled(); expect(current).toHaveBeenCalledTimes(1);
});
it("removes its Escape listener on unmount", () => {
  const close = vi.fn();
  act(() => { view = create(<DeveloperFeedbackModal onClose={close} />); });
  act(() => view.unmount());
  expect(listeners.get("keydown")?.size).toBe(0);
  press("Escape"); expect(close).not.toHaveBeenCalled();
});

it("shows the public contact QR and copies the developer contact", async () => {
  const writeText = vi.fn().mockResolvedValue(undefined);
  vi.stubGlobal("navigator", { clipboard: { writeText } });
  act(() => { view = create(<DeveloperFeedbackModal onClose={vi.fn()} />); });
  expect(view.root.findAllByType("svg").some((node) => node.props.className === "developer-feedback-qr")).toBe(true);
  const button = view.root.findAllByType("button").find((node) => node.props.className === "secondary-button")!;
  await act(async () => { await button.props.onClick(); });
  expect(writeText).toHaveBeenCalledWith("DAISUNFILMS");
});
