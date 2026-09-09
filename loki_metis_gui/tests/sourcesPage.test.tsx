import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { QueryClient } from "@tanstack/react-query";
import { createStore } from "jotai";
import { beforeEach, describe, expect, test, vi } from "vitest";

import type {
  RootCandidateDto,
  RootDiscoveryStatusDto,
  ScanStatusDto,
  SourceRootDto,
} from "../src/api/usage";
import { SourcesPage } from "../src/pages/SourcesPage";
import { agentClientAtom } from "../src/state/agent-client";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => undefined),
}));

const invokeMock = vi.mocked(invoke);

/** 构造数据源页无需读取路径的空闲扫描快照。 */
function scanStatus(): ScanStatusDto {
  return {
    callsIndexed: 0,
    canCancel: false,
    currentScopeLabel: "Idle",
    filesVisited: 0,
    finishedAtEpochMs: null,
    kind: "quick",
    message: "Idle",
    progressBasisPoints: 0,
    scanId: null,
    startedAtEpochMs: null,
    state: "idle",
  };
}

/** 构造可切换生命周期的数据源发现快照。 */
function discoveryStatus(
  state: RootDiscoveryStatusDto["state"] = "idle",
): RootDiscoveryStatusDto {
  return {
    candidatesFound: 0,
    directoriesChecked: 0,
    errorCode: null,
    fallbackPerformed: false,
    fileNamesChecked: 0,
    ioErrors: 0,
    permissionDenied: 0,
    platform: "macOs",
    scope: "userPriority",
    skipped: 0,
    state,
    strategy: "macOsSpotlight",
    systemIndexAvailable: true,
    volumesCompleted: 0,
    volumesTotal: 1,
  };
}

/** 描述单个数据源恢复用例需要注入的读取或写入失败。 */
interface SourcesMockOptions {
  candidateAddHandler?: (attempt: number) => Promise<unknown>;
  candidates?: RootCandidateDto[];
  candidatesFailOnce?: boolean;
  discoveryFailOnce?: boolean;
  discoveryFailsAfterFirst?: boolean;
  discoveryState?: RootDiscoveryStatusDto["state"];
  failingMutation?: string;
  manualAddOutcome?: "alreadyRegistered" | "cancelled" | "registered";
  manualAddPromise?: Promise<unknown>;
  roots?: SourceRootDto[];
  scanFailOnceAfterFirst?: boolean;
  scanFailsAfterFirst?: boolean;
}

/** 装配数据源页的权威读取，并按用例注入单一失败。 */
function mockSourcesApi(options: SourcesMockOptions = {}) {
  let candidateReads = 0;
  let candidateAdds = 0;
  let discoveryReads = 0;
  let scanReads = 0;
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "get_usage_overview") {
      return {
        implementationMessage: null,
        localRecords: null,
        productDefinitionRequired: false,
      };
    }
    if (command === "get_sources") {
      return {
        coverage: {
          permissionDeniedCount: 0,
          rootsDiscovered: 0,
          rootsScanned: 0,
          skippedCount: 0,
          state: "complete",
          warningCount: 0,
        },
        roots: options.roots ?? [],
        scan: scanStatus(),
      };
    }
    if (command === "get_local_scan_status") {
      scanReads += 1;
      if (
        (options.scanFailOnceAfterFirst && scanReads === 2) ||
        (options.scanFailsAfterFirst && scanReads > 1)
      ) {
        throw new Error("scan refresh failed");
      }
      return scanStatus();
    }
    if (command === "get_root_discovery_status") {
      discoveryReads += 1;
      if (
        (options.discoveryFailOnce && discoveryReads === 1) ||
        (options.discoveryFailsAfterFirst && discoveryReads > 1)
      ) {
        throw new Error("discovery read failed");
      }
      return discoveryStatus(options.discoveryState);
    }
    if (command === "list_root_candidates") {
      candidateReads += 1;
      if (options.candidatesFailOnce && candidateReads === 1) {
        throw new Error("candidate read failed");
      }
      return options.candidates ?? [];
    }
    if (options.failingMutation && command === options.failingMutation) {
      throw new Error("mutation failed");
    }
    if (command === "start_root_discovery") return discoveryStatus("running");
    if (command === "cancel_root_discovery") return discoveryStatus("idle");
    if (command === "manual_add_source_root") {
      if (options.manualAddPromise) return options.manualAddPromise;
      const outcome = options.manualAddOutcome ?? "cancelled";
      return {
        changed: outcome === "registered",
        discovery: null,
        messageCode:
          outcome === "registered"
            ? "sourceRegistered"
            : outcome === "alreadyRegistered"
              ? "sourceAlreadyRegistered"
              : "sourceAddCancelled",
        outcome,
      };
    }
    if (command === "add_root_candidate") {
      candidateAdds += 1;
      if (options.candidateAddHandler) {
        return options.candidateAddHandler(candidateAdds);
      }
      return {
        added: true,
        backgroundState: "indexing",
        client: "codex",
        rootId: "candidate-root",
      };
    }
    if (command === "refresh_local_indexes") return [scanStatus()];
    throw new Error(`unexpected command: ${command}`);
  });
  return {
    candidateReads: () => candidateReads,
    candidateAdds: () => candidateAdds,
    discoveryReads: () => discoveryReads,
    scanReads: () => scanReads,
  };
}

describe("sources page recovery", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  /** 候选查询单独失败时，Retry 必须同时重读候选和发现状态。 */
  test("retries the failed candidates query instead of only discovery status", async () => {
    const reads = mockSourcesApi({ candidatesFailOnce: true });
    render(
      <TestProviders>
        <SourcesPage />
      </TestProviders>,
    );

    await userEvent.click(await screen.findByRole("button", { name: "Retry" }));

    expect(await screen.findByText("Data source discovery")).toBeVisible();
    expect(reads.candidateReads()).toBe(2);
    expect(reads.discoveryReads()).toBe(2);
  });

  /** 任一下游权威查询初读失败时，已返回的候选也不得在错误页背后自动登记。 */
  test("does not register candidates while an authoritative initial query failed", async () => {
    mockSourcesApi({
      candidates: [
        {
          absolutePath: "/tmp/codex-candidate",
          client: "codex",
          evidence: "codexRollout",
          id: "candidate-with-failed-discovery",
          strategy: "macOsSpotlight",
        },
      ],
      discoveryFailOnce: true,
    });
    render(
      <TestProviders>
        <SourcesPage />
      </TestProviders>,
    );

    expect(await screen.findByRole("button", { name: "Retry" })).toBeVisible();
    await act(async () => {
      await Promise.resolve();
    });

    expect(invokeMock).not.toHaveBeenCalledWith("add_root_candidate", expect.anything());
  });

  /** 各类用户写操作失败都必须在页面上留下可见且可恢复的错误。 */
  test.each([
    ["start", "start_root_discovery", "Quick scan", "idle"],
    ["cancel", "cancel_root_discovery", "Cancel scan", "running"],
    ["manual add", "manual_add_source_root", "Add data directory", "idle"],
  ] as const)(
    "shows a visible error when %s fails",
    async (_label, failingMutation, buttonName, initialState) => {
      mockSourcesApi({ discoveryState: initialState, failingMutation });
      render(
        <TestProviders>
          <SourcesPage />
        </TestProviders>,
      );

      await userEvent.click(await screen.findByRole("button", { name: buttonName }));

      await waitFor(() =>
        expect(
          screen.getByText(
            "The data-root update failed. Original Codex files were not modified.",
          ),
        ).toBeVisible(),
      );
    },
  );

  /** 后台轮询失败时保留可信来源快照、提示失败并冻结所有数据根写操作。 */
  test("keeps cached sources read-only after a scan status refetch failure", async () => {
    const reads = mockSourcesApi({
      roots: [
        {
          activationState: "ready",
          alias: "Cached root",
          discoveryCode: "userRegistered",
          discoveryLabel: "Manual",
          duplicateCount: 0,
          enabled: true,
          errorCount: 0,
          fileCount: 3,
          id: "codex-root-cached",
          isPrimary: false,
          lastScanAtEpochMs: null,
          skippedCount: 0,
        },
      ],
      scanFailsAfterFirst: true,
    });
    render(
      <TestProviders>
        <SourcesPage />
      </TestProviders>,
    );

    expect(await screen.findAllByText("Cached root")).not.toHaveLength(0);
    await waitFor(
      () =>
        expect(screen.getByRole("button", { name: "Disable Cached root" })).toBeDisabled(),
      { timeout: 4_500 },
    );
    expect(screen.getByRole("alert")).toHaveTextContent(
      "The operation could not be completed. Try again.",
    );
    expect(screen.getAllByText("Cached root")).not.toHaveLength(0);
    expect(reads.scanReads()).toBeGreaterThan(1);
  });

  /** 运行中发现状态的后台重读失败后，旧缓存不能继续授权取消写操作。 */
  test("disables cancel after a running discovery status refetch fails", async () => {
    const reads = mockSourcesApi({
      discoveryFailsAfterFirst: true,
      discoveryState: "running",
    });
    render(
      <TestProviders>
        <SourcesPage />
      </TestProviders>,
    );

    const cancel = await screen.findByRole("button", { name: "Cancel scan" });
    await waitFor(() => expect(cancel).toBeDisabled(), { timeout: 4_500 });
    expect(reads.discoveryReads()).toBeGreaterThan(1);
    await userEvent.click(cancel);

    expect(invokeMock).not.toHaveBeenCalledWith("cancel_root_discovery");
  });

  /** 切换 Agent 会关闭旧客户端弹窗，旧 root ID 不可能被确认到新客户端。 */
  test("closes a root confirmation when the selected client changes", async () => {
    const store = createStore();
    mockSourcesApi({
      roots: [
        {
          activationState: "ready",
          alias: "Codex root",
          discoveryCode: "userRegistered",
          discoveryLabel: "Manual",
          duplicateCount: 0,
          enabled: true,
          errorCount: 0,
          fileCount: 1,
          id: "codex-root-switch",
          isPrimary: false,
          lastScanAtEpochMs: null,
          skippedCount: 0,
        },
      ],
    });
    render(
      <TestProviders store={store}>
        <SourcesPage />
      </TestProviders>,
    );

    await userEvent.click(
      await screen.findByRole("button", { name: "Remove the index for Codex root" }),
    );
    expect(screen.getByRole("dialog", { name: "Remove data-root index" })).toBeVisible();
    act(() => store.set(agentClientAtom, "claudeCode"));

    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Remove data-root index" }),
      ).not.toBeInTheDocument(),
    );
    expect(invokeMock).not.toHaveBeenCalledWith("remove_source_root", expect.anything());
  });

  /** 新的不同动作成功后不再展示上一动作保留的错误。 */
  test("clears an earlier mutation error when a different action succeeds", async () => {
    mockSourcesApi({ failingMutation: "start_root_discovery" });
    render(
      <TestProviders>
        <SourcesPage />
      </TestProviders>,
    );

    await userEvent.click(await screen.findByRole("button", { name: "Quick scan" }));
    expect(
      await screen.findByText(
        "The data-root update failed. Original Codex files were not modified.",
      ),
    ).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Add data directory" }));

    await waitFor(() =>
      expect(
        screen.queryByText(
          "The data-root update failed. Original Codex files were not modified.",
        ),
      ).not.toBeInTheDocument(),
    );
    expect(await screen.findByText("Adding a data directory was cancelled.")).toBeVisible();
  });

  /** 手动登记后的链式索引刷新不能清除本次成功反馈。 */
  test("keeps the registered message after its chained index refresh", async () => {
    mockSourcesApi({ manualAddOutcome: "registered" });
    render(
      <TestProviders>
        <SourcesPage />
      </TestProviders>,
    );

    await userEvent.click(
      await screen.findByRole("button", { name: "Add data directory" }),
    );

    expect(
      await screen.findByText(
        "The data directory was registered. Run a quick scan to build its index.",
      ),
    ).toBeVisible();
    expect(invokeMock).toHaveBeenCalledWith("refresh_local_indexes", {
      clients: ["codex"],
      trigger: "directManual",
    });
  });

  /** 手动登记等待期间失权并恢复，也不能用新代次继续旧链式索引。 */
  test("does not refresh indexes after an in-flight manual add loses authority", async () => {
    let resolveManual:
      | ((value: {
          changed: boolean;
          discovery: null;
          messageCode: "sourceRegistered";
          outcome: "registered";
        }) => void)
      | undefined;
    const manualAdd = new Promise<{
      changed: boolean;
      discovery: null;
      messageCode: "sourceRegistered";
      outcome: "registered";
    }>((resolve) => {
      resolveManual = resolve;
    });
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    mockSourcesApi({ manualAddPromise: manualAdd, scanFailOnceAfterFirst: true });
    render(
      <TestProviders queryClient={queryClient}>
        <SourcesPage />
      </TestProviders>,
    );

    await userEvent.click(
      await screen.findByRole("button", { name: "Add data directory" }),
    );
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("manual_add_source_root", {
        client: "codex",
      }),
    );
    await act(async () => {
      await queryClient.refetchQueries({ queryKey: ["scan-status", "codex"] });
    });
    await waitFor(() =>
      expect(
        screen.getByText("The operation could not be completed. Try again."),
      ).toBeVisible(),
    );
    await act(async () => {
      await queryClient.refetchQueries({ queryKey: ["scan-status", "codex"] });
    });
    await waitFor(() =>
      expect(
        screen.queryByText("The operation could not be completed. Try again."),
      ).not.toBeInTheDocument(),
    );
    await act(async () => {
      resolveManual?.({
        changed: true,
        discovery: null,
        messageCode: "sourceRegistered",
        outcome: "registered",
      });
    });
    expect(
      await screen.findByText(
        "The data directory was registered. Run a quick scan to build its index.",
      ),
    ).toBeVisible();

    expect(invokeMock).not.toHaveBeenCalledWith("refresh_local_indexes", expect.anything());
  });

  /** 候选登记等待期间失权并恢复，也不能启动旧操作的链式索引。 */
  test("does not refresh indexes after an in-flight candidate add loses authority", async () => {
    let resolveCandidate:
      | ((value: {
          added: boolean;
          backgroundState: "indexing";
          client: "codex";
          rootId: string;
        }) => void)
      | undefined;
    const candidateAdd = new Promise<{
      added: boolean;
      backgroundState: "indexing";
      client: "codex";
      rootId: string;
    }>((resolve) => {
      resolveCandidate = resolve;
    });
    const candidate: RootCandidateDto = {
      absolutePath: "/tmp/codex-retry-candidate",
      client: "codex",
      evidence: "codexRollout",
      id: "candidate-retry-authority",
      strategy: "macOsSpotlight",
    };
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    const reads = mockSourcesApi({
      candidateAddHandler: (attempt) =>
        attempt === 1 ? Promise.reject(new Error("candidate add failed")) : candidateAdd,
      candidates: [candidate],
      scanFailOnceAfterFirst: true,
    });
    render(
      <TestProviders queryClient={queryClient}>
        <SourcesPage />
      </TestProviders>,
    );

    await waitFor(() => expect(reads.candidateAdds()).toBe(1));
    const addButton = await screen.findByRole("button", { name: "Add" });
    await userEvent.click(addButton);
    await waitFor(() => expect(reads.candidateAdds()).toBe(2));
    await waitFor(() => expect(addButton).toBeDisabled());
    await act(async () => {
      await queryClient.refetchQueries({ queryKey: ["scan-status", "codex"] });
    });
    await waitFor(() =>
      expect(
        screen.getByText("The operation could not be completed. Try again."),
      ).toBeVisible(),
    );
    await act(async () => {
      await queryClient.refetchQueries({ queryKey: ["scan-status", "codex"] });
    });
    await waitFor(() =>
      expect(
        screen.queryByText("The operation could not be completed. Try again."),
      ).not.toBeInTheDocument(),
    );
    await act(async () => {
      resolveCandidate?.({
        added: true,
        backgroundState: "indexing",
        client: "codex",
        rootId: "candidate-root",
      });
    });
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Add" })).not.toBeInTheDocument(),
    );

    expect(invokeMock).not.toHaveBeenCalledWith("refresh_local_indexes", expect.anything());
  });
});
