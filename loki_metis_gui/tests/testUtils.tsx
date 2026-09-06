import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createStore, Provider as JotaiProvider } from "jotai";
import type { ReactElement, ReactNode } from "react";
import { I18nextProvider } from "react-i18next";

import type { MonitorCapabilities } from "../src/api/monitor";
import { AppThemeProvider } from "../src/components/AppThemeProvider";
import { appI18n } from "../src/i18n";

/** 与 Hook 后端协议一致的完整 Agent 目录。 */
export const allMonitorAiToolsFixture: MonitorCapabilities["aiTools"] = [
  { tool: "codex", name: "Codex" },
  { tool: "claudeCode", name: "Claude Code" },
  { tool: "cursor", name: "Cursor" },
  { tool: "openCode", name: "OpenCode" },
  { tool: "workBuddy", name: "WorkBuddy" },
  { tool: "hermes", name: "Hermes" },
  { tool: "openClaw", name: "OpenClaw" },
  { tool: "codeBuddy", name: "CodeBuddy" },
  { tool: "qwenCode", name: "Qwen Code" },
  { tool: "kimiCode", name: "Kimi Code" },
  { tool: "qoder", name: "Qoder" },
  { tool: "geminiCli", name: "Gemini CLI" },
  { tool: "gitHubCopilot", name: "GitHub Copilot" },
  { tool: "grok", name: "Grok" },
];

/** 为组件测试挂载与生产一致的最小稳定 Provider 集合。 */
export function TestProviders({ children }: { children: ReactNode }): ReactElement {
  const queryClient = new QueryClient({
    defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
  });
  const store = createStore();
  return (
    <I18nextProvider i18n={appI18n}>
      <JotaiProvider store={store}>
        <QueryClientProvider client={queryClient}>
          <AppThemeProvider>{children}</AppThemeProvider>
        </QueryClientProvider>
      </JotaiProvider>
    </I18nextProvider>
  );
}

/** get_monitor_capabilities 的共享夹具；用例只覆写自己关心的字段。 */
export function monitorCapabilitiesFixture(
  overrides: Partial<MonitorCapabilities> = {},
): MonitorCapabilities {
  return {
    aiTools: [
      { tool: "codex", name: "Codex" },
      { tool: "claudeCode", name: "Claude Code" },
      { tool: "grok", name: "Grok Build" },
      { tool: "workBuddy", name: "WorkBuddy" },
    ],
    hookBehaviors: ["idle", "running", "asking", "error"],
    profileSlot: { default: 1, min: 1, max: 12 },
    imageUploadAccept: {
      mimeTypes: ["image/jpeg", "image/png", "image/gif"],
      extensions: [".jpg", ".jpeg", ".png", ".gif"],
    },
    ...overrides,
  };
}
