/** 用量看板设置页的中英文资源；保持页面与两个配置卡片的键结构一致。 */
export const dashboardUsageSettingsZhCN = {
  page: {
    title: "用量设置",
    description: "管理用量数据的后台扫描节奏与派生数据保留范围。",
    codexDescription: "管理 Codex 本机索引与清理范围；本产品不保存会话正文。",
    claudeDescription:
      "掌握 Claude Code 本机 transcript 索引与清理范围；本产品不保存会话正文。",
    loading: "正在读取隐私设置",
  },
  retentionDays: {
    title: "自动清理",
    badge: "默认 90 天",
    label: "自动清理多少天前的数据",
    error: "自动清理天数必须是 1 至 3650 天。",
    save: "保存自动清理天数",
    successTitle: "自动清理天数已保存",
    successBody: "已保存 {{days}} 天；下次打开应用时后台清理更早的派生用量。",
    errorTitle: "无法保存自动清理天数",
  },
  scanInterval: {
    title: "扫描间隔",
    badge: "默认 5 分钟",
    label: "扫描间隔（分钟）",
    error: "扫描间隔必须是 1 至 1440 分钟。",
    save: "保存扫描间隔",
    successTitle: "扫描间隔已保存",
    successBody: "已保存 {{minutes}} 分钟扫描间隔；各客户端共用同一触发器。",
    errorTitle: "无法保存扫描间隔",
  },
};

/** 用量看板设置页的英文资源；键名与中文资源逐项对应。 */
export const dashboardUsageSettingsEnUS = {
  page: {
    title: "Usage settings",
    description:
      "Manage the background scan cadence and retention window for derived usage data.",
    codexDescription:
      "Manage the Codex local index and deletion scope. This app does not save conversation content.",
    claudeDescription:
      "Control the local Claude Code transcript index and deletion scope. Conversation content is not saved.",
    loading: "Loading privacy settings",
  },
  retentionDays: {
    title: "Automatic cleanup",
    badge: "Default: 90 days",
    label: "Delete derived usage older than (days)",
    error: "Automatic cleanup must be from 1 to 3,650 days.",
    save: "Save cleanup window",
    successTitle: "Cleanup window saved",
    successBody:
      "{{days}} days were saved. The next launch will clean older derived usage in the background.",
    errorTitle: "Unable to save the cleanup window",
  },
  scanInterval: {
    title: "Scan interval",
    badge: "Default: 5 minutes",
    label: "Scan interval (minutes)",
    error: "The scan interval must be from 1 to 1,440 minutes.",
    save: "Save scan interval",
    successTitle: "Scan interval saved",
    successBody:
      "The {{minutes}}-minute scan interval was saved. All clients share this trigger.",
    errorTitle: "Unable to save the scan interval",
  },
};
