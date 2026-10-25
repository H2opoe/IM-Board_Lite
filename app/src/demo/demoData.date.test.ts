import { afterEach, expect, it, vi } from "vitest";
import { demoDashboard } from "./demoData";

afterEach(() => vi.useRealTimers());

it.each([
  [new Date(2026, 8, 30, 0, 30), "2026-09-30"],
  [new Date(2027, 0, 1, 0, 30), "2027-01-01"],
  [new Date(2024, 1, 29, 0, 30), "2024-02-29"],
] as const)("uses the current local calendar day at %s", (date, expected) => {
  vi.useFakeTimers();
  vi.setSystemTime(date);
  const dashboard = demoDashboard();
  expect(dashboard.day).toBe(expected);
  expect(dashboard.metrics.find(item => item.key === "messages")?.value).toBe(1099);
});
