import React from "react";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { afterEach, expect, it, vi } from "vitest";
import { useModalEnterGuard } from "./useModalEnterGuard";

afterEach(() => vi.restoreAllMocks());

it("button activation and IME confirmation cannot submit the parent modal", () => {
  let now = 1000;
  vi.spyOn(performance, "now").mockImplementation(() => now);
  let guard!: ReturnType<typeof useModalEnterGuard>;
  let view!: ReactTestRenderer;
  function Probe() { guard = useModalEnterGuard(); return null; }
  act(() => { view = create(React.createElement(Probe)); });
  const event = (tagName: string, insideButton = false) => ({ key: "Enter", nativeEvent: {}, target: { tagName, closest: () => insideButton ? {} : null } }) as unknown as Parameters<typeof guard.shouldHandleModalEnter>[0];
  expect(guard.shouldHandleModalEnter(event("BUTTON"))).toBe(false);
  expect(guard.shouldHandleModalEnter(event("SPAN", true))).toBe(false);
  expect(guard.shouldHandleModalEnter(event("TEXTAREA"))).toBe(false);
  expect(guard.shouldHandleModalEnter(event("INPUT"))).toBe(true);
  guard.handleModalCompositionStart();
  expect(guard.shouldHandleModalEnter(event("INPUT"))).toBe(false);
  guard.handleModalCompositionEnd();
  expect(guard.shouldHandleModalEnter(event("INPUT"))).toBe(false);
  now += 200;
  expect(guard.shouldHandleModalEnter(event("INPUT"))).toBe(true);
  act(() => view.unmount());
});

it("does not invent a composition event at the clock origin or after reset", () => {
  vi.spyOn(performance, "now").mockReturnValue(0);
  let guard!: ReturnType<typeof useModalEnterGuard>;
  let view!: ReactTestRenderer;
  function Probe() { guard = useModalEnterGuard(); return null; }
  act(() => { view = create(React.createElement(Probe)); });
  const event = { key: "Enter", nativeEvent: {}, target: { tagName: "INPUT", closest: () => null } } as unknown as Parameters<typeof guard.shouldHandleModalEnter>[0];
  try {
    expect(guard.shouldHandleModalEnter(event)).toBe(true);
    guard.handleModalCompositionStart();
    expect(guard.shouldHandleModalEnter(event)).toBe(false);
    guard.handleModalCompositionEnd();
    expect(guard.shouldHandleModalEnter(event)).toBe(false);
    guard.resetModalCompositionState();
    expect(guard.shouldHandleModalEnter(event)).toBe(true);
  } finally {
    act(() => view.unmount());
  }
});
