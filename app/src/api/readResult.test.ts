import { expect, it } from "vitest";
import fixtures from "../../shared/read-result-fixtures.json";
import { READ_RESULT_MESSAGES, requireCompleteRead } from "./readResult";

it.each(fixtures)("shared read contract: $name", ({ data, expected }) => {
  const read = () => requireCompleteRead({ ok: true, data });
  if (expected === "complete") expect(read()).toBeInstanceOf(Array);
  else expect(read).toThrow(READ_RESULT_MESSAGES[expected as "invalid" | "incomplete" | "api"]);
});
it("keeps a structured permission failure even when an empty array is present", () => {
  expect(() => requireCompleteRead({ ok: false, data: [], error: { message: "missing search scope" } })).toThrow("missing search scope");
});
it("reports warnings consistently and ignores blank diagnostics", () => {
  expect(() => requireCompleteRead({ data: [], warnings: ["partial read"] })).toThrow("partial read");
  expect(requireCompleteRead({ data: [], warnings: [" "] })).toEqual([]);
});
