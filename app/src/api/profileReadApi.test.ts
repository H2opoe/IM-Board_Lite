import { beforeEach, expect, it, vi } from "vitest";
import { testProfileRead } from "./profileReadApi";
import { READ_RESULT_MESSAGES } from "./readResult";
import { PLATFORM_BINDING_OPTIONS } from "../constants/platforms";
import type { ImProfile } from "../features/profiles/model/types";
const bridge = vi.hoisted(() => ({ run: vi.fn() }));
vi.mock("./bridgeApi", () => ({ runBridgeCommand: bridge.run }));
vi.mock("./tauri", () => ({ isTauri: true, requireTauri: vi.fn() }));
const profile = { id:"fixture", platform:"dingtalk", configJson:{remark:"unrelated name"},label:"fixture",enabled:true,status:"normal",sortOrder:0,createdAt:"",updatedAt:"" } as ImProfile;
beforeEach(() => vi.resetAllMocks());
it("reads a time window directly and deduplicates both direct and group conversations", async () => {
 bridge.run.mockResolvedValue({data:[{chatId:"direct",isGroup:false},{chatId:"group",isGroup:true},{chatId:"group",isGroup:true}],warnings:[]});
 expect(await testProfileRead(profile)).toBe("测试读取成功：今天消息3条；会话2个；群聊1个。");
 expect(bridge.run).toHaveBeenCalledTimes(1);
 expect(bridge.run).toHaveBeenCalledWith(profile,"search-messages",expect.objectContaining({start_time:expect.any(String),end_time:expect.any(String)}));
});
it("treats a confirmed empty read as normal success", async () => {
 bridge.run.mockResolvedValue({data:[],warnings:[]});
 const result = await testProfileRead(profile);
 expect(result).toBe(READ_RESULT_MESSAGES.empty);
});
it("does not turn partial warnings or permission failures into a successful empty result", async () => {
 bridge.run.mockResolvedValue({data:[],warnings:["fixture partial"]});
 await expect(testProfileRead(profile)).rejects.toThrow("fixture partial");
 bridge.run.mockRejectedValue(new Error("fixture permission denied"));
 await expect(testProfileRead(profile)).rejects.toThrow("fixture permission denied");
});

it("Wecom automatically enumerates the full visible directory without keyword entry",async()=>{
 const wecom={...profile,platform:"wecom" as const,configJson:{contactSearchKeywords:["obsolete manual filter"]}};
 bridge.run.mockImplementation(async (_profile,command)=>command==="list-contacts" ? {data:[{chatId:"u1"},{chatId:"u2"}]} : {data:[{chatId:"u1",isGroup:false},{chatId:"group",isGroup:true}]});
 expect(await testProfileRead(wecom)).toBe("测试读取成功：通讯录2人；今天会话2个；群聊1个。");
 expect(bridge.run).toHaveBeenNthCalledWith(1,wecom,"list-contacts",{});
});
it("directory failures must not be reported as successful groups-only coverage",async()=>{
 const wecom={...profile,platform:"wecom" as const,configJson:{}};
 bridge.run.mockRejectedValue(new Error("fixture directory unavailable"));
 await expect(testProfileRead(wecom)).rejects.toThrow("fixture directory unavailable");
});

it.each(PLATFORM_BINDING_OPTIONS)("$label: a successful empty read is quiet and uses the common success text", async ({ id }) => {
  bridge.run.mockResolvedValue({ ok: true, data: [], warnings: [] });
  expect(await testProfileRead({ ...profile, platform: id })).toBe(READ_RESULT_MESSAGES.empty);
});
it.each(PLATFORM_BINDING_OPTIONS)("$label: missing collections, partial reads, and API errors remain failures", async ({ id }) => {
  const account = { ...profile, platform: id };
  for (const data of [{}, null, { items: [], complete: false }, { items: [], failedCount: 1 }]) {
    bridge.run.mockResolvedValue({ ok: true, data, warnings: [] });
    await expect(testProfileRead(account)).rejects.toThrow();
  }
  bridge.run.mockResolvedValue({ ok: false, data: [], error: { message: "permission denied" } });
  await expect(testProfileRead(account)).rejects.toThrow("permission denied");
  bridge.run.mockResolvedValue({ ok: true, data: [], warnings: ["partial read"] });
  await expect(testProfileRead(account)).rejects.toThrow("partial read");
});
