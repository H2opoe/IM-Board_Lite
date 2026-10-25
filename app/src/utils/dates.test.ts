import { expect, it } from "vitest";
import { formatLocalDate, formatRelativeDateTime } from "./dates";

function localClock(date: Date) {
  return date.toLocaleTimeString("en-GB", { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
}

it.each([
  "2026-09-30T03:36:00.000Z",
  "2026-09-30T11:36:00+08:00",
  "2026-09-29T23:36:00-05:30",
  "2026-09-30T03:36:00+00:00",
  "2026-09-30T11:36:00+0800",
  "2026-09-30T03:36:00.125Z",
  "2026-09-29T24:00:00Z",
])("formats the local clock of the absolute ISO instant %s", value => {
  const instant = new Date(value);
  const now = new Date(instant);
  now.setHours(23, 59, 59, 999);
  expect(formatRelativeDateTime(value, now)).toBe(`今天 ${localClock(instant)}`);
});

it.each([
  "2026-09-29T19:36:00Z",
  "2026-09-30T01:36:00+09:00",
  "2026-09-29T23:36:00-05:30",
])("groups %s by the converted local day rather than its source date", value => {
  const instant = new Date(value);
  const nextLocalDay = new Date(instant);
  nextLocalDay.setDate(nextLocalDay.getDate() + 1);
  nextLocalDay.setHours(12, 0, 0, 0);
  expect(formatRelativeDateTime(value, nextLocalDay)).toBe(`昨天 ${localClock(instant)}`);
});

it.each([
  "2026-09-30 11:36:00",
  "2026-09-30T11:36:00",
  "2026-09-30 11:36:00.123456",
  "2026-09-30 11:36",
])("preserves database local wall-clock semantics for %s", value => {
  expect(formatRelativeDateTime(value, new Date(2026, 8, 30, 12))).toBe("今天 11:36");
});

it("treats a database date without a time as local midnight", () => {
  expect(formatRelativeDateTime("2026-09-30", new Date(2026, 8, 30, 12))).toBe("今天 00:00");
});

it.each([
  [new Date(2026, 8, 30, 0, 30), "2026-09-30"],
  [new Date(2027, 0, 1, 0, 30), "2027-01-01"],
  [new Date(2024, 1, 29, 0, 30), "2024-02-29"],
] as const)("keeps the local calendar date of %s", (date, expected) => {
  expect(formatLocalDate(date)).toBe(expected);
});

it("keeps valid leap days and existing non-ISO timezone formats", () => {
  expect(formatRelativeDateTime("2024-02-29 11:36:00", new Date(2024, 1, 29, 12))).toBe("今天 11:36");
  const value = "Wed, 30 Sep 2026 03:36:00 GMT";
  const instant = new Date(value);
  expect(formatRelativeDateTime(value, instant)).toBe(`今天 ${localClock(instant)}`);
});

it.each([
  "2026-02-29T11:36:00Z",
  "2026-02-30 11:36:00",
  "2026-13-30 11:36:00",
  "2026-00-30 11:36:00",
  "2026-09-00 11:36:00",
  "2026-09-30T11:36:00+25:00",
  "2026-09-30T11:36:00+08:60",
  "2026-09-30T11:36:00garbage",
  "2026-09-30 25:36:00",
  "2026-09-30 11:60:00",
  "2026-09-30T24:00:00.001Z",
  "not-a-date",
  "",
])("leaves invalid input visible instead of normalizing it: %s", value => {
  expect(formatRelativeDateTime(value, new Date(2026, 8, 30, 12))).toBe(value);
});
