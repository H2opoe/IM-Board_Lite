import { act, create } from "react-test-renderer";
import type { ReactTestRenderer } from "react-test-renderer";
import { afterEach, expect, it, vi } from "vitest";
import type { ActionItem } from "../../features/dashboard/model/types";
import { ActionListCard } from "./ActionListCard";

vi.mock("./dashboardAnimations", () => ({
  CountUpNumber: ({ value }: { value: number }) => value,
  dashboardIntroClassName: (value: string) => value,
  useDashboardViewportIntro: () => ({ ref: () => {}, isIntroAnimating: false, hasStarted: true })
}));

let view: ReactTestRenderer | undefined;
afterEach(() => { act(() => { view?.unmount(); }); view = undefined; });

function item(id: string, overrides: Partial<ActionItem> = {}): ActionItem {
  return {
    id, itemType: "task", status: "done", priority: "medium", title: id, description: "合成排序示例",
    profileId: "fixture", platform: "wecom", platformLabel: "企业微信", platformRemark: "示例", sourceLabel: "示例",
    chatId: "fixture-chat", chatName: "示例", evidenceSummary: "示例", contextIncomplete: false, carryOver: false,
    sourceMessageAt: "2026-09-30 11:00:00", lastUpdatedAt: "2026-09-30 03:40:00", ...overrides
  };
}

async function renderOrder(items: ActionItem[]) {
  await act(async () => {
    view = create(<ActionListCard title="排序示例" aiStatus="ready" items={items} onComplete={() => {}} onReopen={() => {}} onOpenSource={() => {}} onConfigureAi={() => {}} />);
  });
  return view!.root.findAllByProps({ className: "action-main" }).map(row => row.findByType("strong").children.join(""));
}

it("puts the later SQLite UTC update first when its completion time is missing", async () => {
  expect(await renderOrder([
    item("modern", { completedAt: "2026-09-30T03:40:00+00:00" }),
    item("legacy", { lastUpdatedAt: "2026-09-30 03:45:00" })
  ])).toEqual(["legacy", "modern"]);
});

it("keeps the earlier SQLite UTC fallback behind an explicitly completed item", async () => {
  expect(await renderOrder([
    item("legacy", { lastUpdatedAt: "2026-09-30 03:35:00" }),
    item("modern", { completedAt: "2026-09-30T03:40:00Z" })
  ])).toEqual(["modern", "legacy"]);
});

it("orders explicit completion offsets across a daylight-saving clock rollback", async () => {
  expect(await renderOrder([
    item("before", { completedAt: "2026-11-01T01:50:00-04:00" }),
    item("after", { completedAt: "2026-11-01T01:10:00-05:00" })
  ])).toEqual(["after", "before"]);
});

it("preserves milliseconds when a completion and UTC fallback share a second", async () => {
  expect(await renderOrder([
    item("fallback", { lastUpdatedAt: "2026-09-30 03:40:00" }),
    item("precise", { completedAt: "2026-09-30T03:40:00.500Z" })
  ])).toEqual(["precise", "fallback"]);
});

it("retains the existing local parsing of unverified naive completedAt values", async () => {
  const earlierLocal = new Date(2026, 8, 30, 11, 35).toISOString();
  expect(await renderOrder([
    item("earlier", { completedAt: earlierLocal }),
    item("naive-completed", { completedAt: "2026-09-30 11:36:00" })
  ])).toEqual(["naive-completed", "earlier"]);
});

it("keeps source times local and preserves open/done/ignored ordering", async () => {
  expect(await renderOrder([
    item("ignored-old", { status: "ignored", sourceMessageAt: "2026-09-30 11:35:00" }),
    item("open-new", { status: "open", sourceMessageAt: "2026-09-30 11:40:00" }),
    item("done", { completedAt: "2026-09-30T03:40:00Z" }),
    item("ignored-new", { status: "ignored", sourceMessageAt: "2026-09-30 11:40:00" }),
    item("open-old", { status: "open", sourceMessageAt: "2026-09-30 11:35:00" })
  ])).toEqual(["open-old", "open-new", "done", "ignored-new", "ignored-old"]);
});

it("accepts the ISO UTC update fallback used by UI example interactions", async () => {
  expect(await renderOrder([
    item("earlier", { completedAt: "2026-09-30T03:40:00Z" }),
    item("example", { lastUpdatedAt: "2026-09-30T03:45:00.000Z" })
  ])).toEqual(["example", "earlier"]);
});

it("retains the existing zero fallback for invalid completion timestamps", async () => {
  expect(await renderOrder([
    item("invalid", { completedAt: "not-a-date", lastUpdatedAt: "2026-09-30 03:50:00" }),
    item("valid", { completedAt: "2026-09-30T03:40:00Z" })
  ])).toEqual(["valid", "invalid"]);
});
