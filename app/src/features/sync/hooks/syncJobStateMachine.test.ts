import { describe, expect, it } from "vitest";

import type { SyncProgress } from "../model/types";
import {
  isCancellableSyncState,
  nextStateForProgress,
  shouldRefreshDashboardForProgress
} from "./syncJobStateMachine";

function progress(overrides: Partial<SyncProgress> = {}): SyncProgress {
  return {
    profileId: "aggregate",
    profileLabel: "全部账号",
    phase: "analysis",
    message: "正在分析",
    current: 1,
    total: 2,
    shouldRefreshDashboard: false,
    ...overrides
  };
}

describe("syncJobStateMachine", () => {
  it("allows cancellation only while work is running", () => {
    expect(isCancellableSyncState("syncing")).toBe(true);
    expect(isCancellableSyncState("analyzing")).toBe(true);
    expect(isCancellableSyncState("done")).toBe(false);
    expect(isCancellableSyncState("failed")).toBe(false);
  });

  it("refreshes retry-analysis only after the summary checkpoint", () => {
    expect(
      shouldRefreshDashboardForProgress(
        "retry_analysis",
        progress({ phase: "analysis", shouldRefreshDashboard: true })
      )
    ).toBe(false);
    expect(
      shouldRefreshDashboardForProgress(
        "retry_analysis",
        progress({ phase: "summary_done", shouldRefreshDashboard: true })
      )
    ).toBe(true);
    expect(nextStateForProgress(progress())).toBe("analyzing");
  });
});
