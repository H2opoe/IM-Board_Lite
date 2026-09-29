import { describe, expect, it } from "vitest";

import type { SyncResult } from "../model/types";
import { shouldShowSyncWarning, syncCompletionMessage } from "./syncMessages";

function result(overrides: Partial<SyncResult> = {}): SyncResult {
  return {
    profileId: "aggregate",
    insertedMessages: 3,
    analyzedMessages: 2,
    syncStatus: "synced",
    aiStatus: "ready",
    warnings: [],
    startedAt: "2026-08-08T00:00:00Z",
    finishedAt: "2026-08-08T00:00:01Z",
    ...overrides
  };
}

describe("syncCompletionMessage", () => {
  it("does not describe a total failure as success", () => {
    expect(syncCompletionMessage("incremental", result({ syncStatus: "failed" }))).toContain(
      "没有账号同步成功"
    );
  });

  it("explains partial success without advancing failed accounts", () => {
    const message = syncCompletionMessage("incremental", result({ syncStatus: "partial" }));
    expect(message).toContain("部分账号同步成功");
    expect(message).toContain("失败账号未更新成功时间");
  });
});

describe("shouldShowSyncWarning", () => {
  it("hides harmless paging noise but keeps permission failures", () => {
    expect(shouldShowSyncWarning("[page 2] fetched 20 messages")).toBe(false);
    expect(shouldShowSyncWarning("飞书：缺少消息读取权限，请重新授权")).toBe(true);
  });
});
