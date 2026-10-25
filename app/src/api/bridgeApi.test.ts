import { beforeEach, expect, it, vi } from "vitest";
import { runBridgeCommand } from "./bridgeApi";
import type { ImProfile } from "../features/profiles/model/types";

const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("./tauri", () => ({ isTauri: true, requireTauri: vi.fn() }));
beforeEach(() => vi.resetAllMocks());

it.each(["wecom", "feishu", "dingtalk"] as const)("%s timeout retains its actual error and generic timeout guidance", async platform => {
  native.invoke.mockResolvedValue({ ok: false, data: null, error: { code: "BRIDGE_TIMEOUT", message: "fixture company timeout" }, warnings: ["fixture warning"] });
  await expect(runBridgeCommand({ id: "fixture", platform } as ImProfile, "auth-status", {}))
    .rejects.toThrow("桥接进程执行超时\nfixture company timeout\nfixture warning");
  expect(native.invoke).toHaveBeenCalledWith("run_bridge_command", { request: { platform, command: "auth-status", profile: { id: "fixture", platform }, args: {} } });
});

it("does not duplicate a label already present in the backend error", async () => {
  native.invoke.mockResolvedValue({ ok: false, error: { code: "BRIDGE_TIMEOUT", message: "桥接进程执行超时：fixture" } });
  await expect(runBridgeCommand({ platform: "feishu" } as ImProfile, "auth-status", {})).rejects.toThrow(/^桥接进程执行超时：fixture$/);
});
