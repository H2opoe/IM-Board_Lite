import type { ActionItem, DashboardData, ImProfile, Platform, SourceStat } from "../types";

const now = new Date();
const iso = (hour: number, minute = 0) => {
  const date = new Date(now);
  date.setHours(hour, minute, 0, 0);
  return date.toISOString();
};

export const demoProfiles: ImProfile[] = [
  demoProfile("demo_wechat_growth", "wechat", "微信", "增长运营", 0),
  demoProfile("demo_wecom_retail", "wecom", "企业微信", "渠道客户", 1),
  demoProfile("demo_feishu_rd", "feishu", "飞书", "研发协同", 2),
  demoProfile("demo_dingtalk_delivery", "dingtalk", "钉钉", "交付项目", 3)
];

const sources = {
  wechat: source("demo_wechat_growth", "wechat", "微信", "增长运营", 286, ["新品直播筹备群", "投放素材快审"]),
  wecom: source("demo_wecom_retail", "wecom", "企业微信", "渠道客户", 344, ["华南门店试点群", "重点客户回访"]),
  feishu: source("demo_feishu_rd", "feishu", "飞书", "研发协同", 271, ["桌面端性能专项", "本地模型验收"]),
  dingtalk: source("demo_dingtalk_delivery", "dingtalk", "钉钉", "交付项目", 198, ["西区交付战情室", "合同回款推进"])
};

const demoActions: ActionItem[] = [
  action("reply", "high", "确认华南门店试点折扣边界", "渠道客户希望明早前拿到折扣和赠品口径，方便门店培训统一话术。", "demo_wecom_retail", "wecom", "企业微信", "渠道客户", "华南门店试点群", "客户已接受首批20家门店试点，但要求你确认满赠活动能否叠加老客券。", iso(16, 12)),
  action("reply", "high", "回复本地模型验收是否延后", "研发同事发现低配机器首次加载偏慢，需要你决定是否影响本周演示范围。", "demo_feishu_rd", "feishu", "飞书", "研发协同", "本地模型验收", "性能数据已补齐，争议点集中在是否把离线推理放进默认演示路径。", iso(15, 48)),
  action("reply", "medium", "确认直播间优惠券标题", "运营给出两版优惠券标题，投放同事等待最终版本进素材包。", "demo_wechat_growth", "wechat", "微信", "增长运营", "新品直播筹备群", "当前更推荐「开播前锁定专属价」，但需要你确认是否符合品牌语气。", iso(11, 36)),
  action("task", "high", "整理西区交付风险清单", "交付项目有三家客户卡在验收环境，今晚需要一版可同步管理层的风险摘要。", "demo_dingtalk_delivery", "dingtalk", "钉钉", "交付项目", "西区交付战情室", "项目经理已标出环境、数据迁移和客户排期三个阻塞点，等你合并成日报。", iso(17, 5)),
  action("task", "medium", "复核桌面端启动耗时截图", "研发已上传三组启动耗时截图，需要你挑出适合对外演示的版本。", "demo_feishu_rd", "feishu", "飞书", "研发协同", "桌面端性能专项", "截图覆盖冷启动、热启动和后台唤起，建议优先展示热启动优化前后对比。", iso(14, 22)),
  action("task", "medium", "补齐重点客户回访纪要", "客户成功已完成两轮回访，还缺下一步负责人和时间点。", "demo_wecom_retail", "wecom", "企业微信", "渠道客户", "重点客户回访", "纪要里已有满意度和阻塞问题，待你补上回访后的行动项归属。", iso(10, 58)),
  action("task", "low", "归档投放素材快审结论", "素材群已经确认三张主图和两条短视频，结论需要同步到项目文档。", "demo_wechat_growth", "wechat", "微信", "增长运营", "投放素材快审", "主图B和短视频2被选为首轮投放素材，设计同事已上传最终包。", iso(9, 42), "done")
];

export function demoDashboard(profileId = "aggregate"): DashboardData {
  const sourceList = profileId === "aggregate" ? Object.values(sources) : Object.values(sources).filter((item) => item.profileId === profileId);
  const sourceIds = new Set(sourceList.map((item) => item.profileId));
  const actions = demoActions.filter((item) => profileId === "aggregate" || sourceIds.has(item.profileId));
  const messageTotal = sourceList.reduce((sum, item) => sum + item.count, 0);
  const openReplies = actions.filter((item) => item.itemType === "reply" && item.status === "open").length;
  const openTasks = actions.filter((item) => item.itemType === "task" && item.status === "open").length;

  return {
    day: new Date().toISOString().slice(0, 10),
    metrics: [
      { key: "messages", label: "今天消息数", value: messageTotal, sources: sourceList },
      { key: "replies", label: "待我回复", value: openReplies, sources: sourceList },
      { key: "tasks", label: "待办事项", value: openTasks, sources: sourceList },
      { key: "chats", label: "对话/群聊数", value: profileId === "aggregate" ? 42 : 12, sources: sourceList }
    ],
    replies: actions.filter((item) => item.itemType === "reply"),
    tasks: actions.filter((item) => item.itemType === "task"),
    topics: [
      { title: "华南门店试点", summary: "渠道客户围绕试点门店、折扣边界、培训话术和赠品规则密集确认。", count: 42, sourceChats: [{ chatName: "华南门店试点群", isGroup: true }, { chatName: "重点客户回访", isGroup: false }], sources: [sources.wecom] },
      { title: "本地模型验收", summary: "研发协同讨论离线推理、首次加载、低配机器体验和默认演示路径。", count: 35, sourceChats: [{ chatName: "本地模型验收", isGroup: true }, { chatName: "桌面端性能专项", isGroup: true }], sources: [sources.feishu] },
      { title: "新品直播准备", summary: "增长运营确认直播间标题、优惠券口径、素材版本和投放节奏。", count: 33, sourceChats: [{ chatName: "新品直播筹备群", isGroup: true }, { chatName: "投放素材快审", isGroup: true }], sources: [sources.wechat] },
      { title: "西区交付风险", summary: "交付项目集中暴露验收环境、客户排期和数据迁移三类阻塞。", count: 27, sourceChats: [{ chatName: "西区交付战情室", isGroup: true }], sources: [sources.dingtalk] },
      { title: "合同回款推进", summary: "财务和项目经理同步合同节点、付款材料和月底前回款概率。", count: 16, sourceChats: [{ chatName: "合同回款推进", isGroup: true }], sources: [sources.dingtalk] }
    ].filter((topic) => topic.sources?.some((sourceItem) => sourceIds.has(sourceItem.profileId)) ?? true),
    chatRank: [
      { chat: "华南门店试点群", count: 96, sourceLabel: "企业微信（渠道客户）" },
      { chat: "本地模型验收", count: 82, sourceLabel: "飞书（研发协同）" },
      { chat: "新品直播筹备群", count: 74, sourceLabel: "微信（增长运营）" },
      { chat: "西区交付战情室", count: 61, sourceLabel: "钉钉（交付项目）" },
      { chat: "桌面端性能专项", count: 49, sourceLabel: "飞书（研发协同）" },
      { chat: "重点客户回访", count: 43, sourceLabel: "企业微信（渠道客户）" }
    ].filter((row) => profileId === "aggregate" || row.sourceLabel?.includes(profileRemark(profileId))),
    speakerTop: [
      { speaker: "顾南星", count: 121, sourceLabel: "企业微信（渠道客户）" },
      { speaker: "陆明澈", count: 104, sourceLabel: "飞书（研发协同）" },
      { speaker: "许知微", count: 92, sourceLabel: "微信（增长运营）" },
      { speaker: "程一舟", count: 84, sourceLabel: "钉钉（交付项目）" },
      { speaker: "孟清和", count: 73, sourceLabel: "飞书（研发协同）" },
      { speaker: "唐亦然", count: 66, sourceLabel: "企业微信（渠道客户）" }
    ].filter((row) => profileId === "aggregate" || row.sourceLabel?.includes(profileRemark(profileId))),
    hourlyActivity: Array.from({ length: 24 }, (_, hour) => ({
      hour: String(hour).padStart(2, "0"),
      count: [0, 0, 0, 0, 1, 3, 9, 24, 52, 88, 104, 92, 55, 68, 97, 136, 154, 112, 71, 39, 22, 12, 5, 1][hour] ?? 0,
      sources: sourceList
    })),
    messageTypes: [
      { type: "文本", count: Math.round(messageTotal * 0.69), sources: sourceList },
      { type: "图片", count: Math.round(messageTotal * 0.13), sources: sourceList },
      { type: "文件", count: Math.round(messageTotal * 0.09), sources: sourceList },
      { type: "链接", count: Math.round(messageTotal * 0.06), sources: sourceList },
      { type: "语音", count: Math.round(messageTotal * 0.03), sources: sourceList }
    ],
    keywords: [
      { text: "试点门店", weight: 96, count: 48, sources: [sources.wecom] },
      { text: "本地模型", weight: 90, count: 41, sources: [sources.feishu] },
      { text: "优惠券", weight: 84, count: 36, sources: [sources.wechat] },
      { text: "交付风险", weight: 78, count: 31, sources: [sources.dingtalk] },
      { text: "启动耗时", weight: 70, count: 25, sources: [sources.feishu] },
      { text: "培训话术", weight: 64, count: 22, sources: [sources.wecom] },
      { text: "直播素材", weight: 58, count: 19, sources: [sources.wechat] },
      { text: "验收环境", weight: 50, count: 15, sources: [sources.dingtalk] },
      { text: "回款材料", weight: 42, count: 11, sources: [sources.dingtalk] }
    ].filter((keyword) => keyword.sources?.some((sourceItem) => sourceIds.has(sourceItem.profileId)) ?? true),
    aiStatus: "ready",
    syncStatus: "synced"
  };
}

export function updateDemoAction(actionId: string, status: ActionItem["status"]) {
  const item = demoActions.find((actionItem) => actionItem.id === actionId);
  if (item) item.status = status;
}

function demoProfile(id: string, platform: Platform, label: string, remark: string, sortOrder: number): ImProfile {
  const createdAt = iso(8);
  return {
    id,
    platform,
    label,
    enabled: true,
    configJson: { remark },
    status: "normal",
    sortOrder,
    createdAt,
    updatedAt: createdAt
  };
}

function source(profileId: string, platform: Platform, platformLabel: string, remark: string, count: number, chats: string[]): SourceStat {
  return {
    profileId,
    platform,
    platformLabel,
    remark,
    label: `${platformLabel}（${remark}）`,
    count,
    chats
  };
}

function action(
  itemType: ActionItem["itemType"],
  priority: ActionItem["priority"],
  title: string,
  description: string,
  profileId: string,
  platform: Platform,
  platformLabel: string,
  platformRemark: string,
  chatName: string,
  evidenceSummary: string,
  lastUpdatedAt: string,
  status: ActionItem["status"] = "open"
): ActionItem {
  return {
    id: `demo_${itemType}_${profileId}_${title}`,
    itemType,
    status,
    priority,
    title,
    description,
    suggestedReply: "",
    profileId,
    platform,
    platformLabel,
    platformRemark,
    sourceLabel: `${platformLabel}（${platformRemark}）`,
    chatId: `demo_chat_${chatName}`,
    chatName,
    evidenceSummary,
    contextIncomplete: false,
    carryOver: false,
    sourceMessageAt: lastUpdatedAt,
    lastUpdatedAt,
    completedAt: status === "done" ? lastUpdatedAt : undefined
  };
}

function profileRemark(profileId: string) {
  return demoProfiles.find((profile) => profile.id === profileId)?.configJson.remark as string;
}
