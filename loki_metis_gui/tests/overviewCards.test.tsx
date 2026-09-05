import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";

import type { WindowUsageDto } from "../src/api/usage";
import { LocalWindowSummary } from "../src/pages/OverviewCards";
import { TestProviders } from "./testUtils";

/** 构造概览窗口摘要测试所需的最小本机事实。 */
function emptyWindowUsage(): WindowUsageDto {
  return {
    window: "today",
    fact: {
      completeness: "complete",
      confidence: "exact",
      freshness: "fresh",
      observedAtEpochMs: 0,
      provider: "rolloutJsonl",
      scope: "deviceObserved",
      sourceVersion: null,
      value: {
        cacheReadBasisPoints: null,
        cachedReadCallCount: 0,
        callCount: 0,
        confidence: "exact",
        crossRootDuplicateSourceCount: 0,
        duplicateSourceCount: 0,
        rootCount: 0,
        sourceCount: 0,
        threadCount: 0,
        tokens: {
          cacheWriteInputTokens: null,
          cachedInputTokens: null,
          inputTokens: 0,
          outputTokens: 0,
          reasoningOutputTokens: null,
          totalIsDerived: false,
          totalTokens: 0,
        },
      },
    },
  };
}

describe("overview local window summary", () => {
  /** 覆盖卡不再提供跳转到调用页的浏览全部调用按钮。 */
  test("coverage_card_does_not_offer_browse_all_calls", () => {
    render(
      <TestProviders>
        <LocalWindowSummary windowUsage={emptyWindowUsage()} />
      </TestProviders>,
    );
    expect(screen.getByText("Cache and data coverage")).toBeVisible();
    expect(screen.queryByRole("link", { name: /Browse all calls/i })).not.toBeInTheDocument();
    expect(screen.queryByText(/Browse all calls/i)).not.toBeInTheDocument();
  });
});
