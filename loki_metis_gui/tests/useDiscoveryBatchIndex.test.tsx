import { act, renderHook, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { QueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, test, vi } from "vitest";

import type { RootCandidateDto, RootDiscoveryStatusDto } from "../src/api/usage";
import { useDiscoveryBatchIndex } from "../src/pages/useDiscoveryBatchIndex";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 构造可在运行态和终态之间切换的数据源发现快照。 */
function discoveryStatus(state: RootDiscoveryStatusDto["state"]): RootDiscoveryStatusDto {
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
    volumesCompleted: 1,
    volumesTotal: 1,
  };
}

/** 描述回归 hook 的动态权威状态输入。 */
interface HookProps {
  businessReady: boolean;
  discovery: RootDiscoveryStatusDto;
}

describe("discovery batch authoritative state", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  /** 终态候选读取期间失去权威查询后不得继续触发后台索引写入。 */
  test("stops terminal writes when authoritative queries fail mid-flight", async () => {
    let resolveCandidates: ((value: RootCandidateDto[]) => void) | undefined;
    invokeMock.mockImplementation((command: string) => {
      if (command === "list_root_candidates") {
        return new Promise<RootCandidateDto[]>((resolve) => {
          resolveCandidates = resolve;
        });
      }
      throw new Error(`unexpected command: ${command}`);
    });
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    /** 为 hook 提供与生产一致且可观察的 QueryClient。 */
    const wrapper = ({ children }: { children: ReactNode }) => (
      <TestProviders queryClient={queryClient}>{children}</TestProviders>
    );
    const { result, rerender } = renderHook(
      ({ businessReady, discovery }: HookProps) =>
        useDiscoveryBatchIndex({
          businessReady,
          candidates: [],
          client: "codex",
          discovery,
        }),
      {
        initialProps: { businessReady: true, discovery: discoveryStatus("running") },
        wrapper,
      },
    );

    await waitFor(() => expect(result.current.isActive).toBe(true));
    rerender({ businessReady: true, discovery: discoveryStatus("complete") });
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("list_root_candidates"));

    rerender({ businessReady: false, discovery: discoveryStatus("complete") });
    await act(async () => {
      resolveCandidates?.([]);
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.isRefreshing).toBe(false));

    expect(result.current.isActive).toBe(true);
    expect(invokeMock).not.toHaveBeenCalledWith("refresh_local_indexes", expect.anything());
    expect(invokeMock.mock.calls).toEqual([["list_root_candidates"]]);
  });

  /** 终态处理因短暂失权中止后必须保留批次，并在恢复时恰好刷新一次。 */
  test("resumes one terminal refresh after authoritative queries recover", async () => {
    let candidateReads = 0;
    let resolveFirstCandidates: ((value: RootCandidateDto[]) => void) | undefined;
    invokeMock.mockImplementation((command: string) => {
      if (command === "list_root_candidates") {
        candidateReads += 1;
        if (candidateReads === 1) {
          return new Promise<RootCandidateDto[]>((resolve) => {
            resolveFirstCandidates = resolve;
          });
        }
        return Promise.resolve([]);
      }
      if (command === "refresh_local_indexes") return Promise.resolve([]);
      throw new Error(`unexpected command: ${command}`);
    });
    const queryClient = new QueryClient({
      defaultOptions: { mutations: { retry: false }, queries: { retry: false } },
    });
    const wrapper = ({ children }: { children: ReactNode }) => (
      <TestProviders queryClient={queryClient}>{children}</TestProviders>
    );
    const { result, rerender } = renderHook(
      ({ businessReady, discovery }: HookProps) =>
        useDiscoveryBatchIndex({
          businessReady,
          candidates: [],
          client: "codex",
          discovery,
        }),
      {
        initialProps: { businessReady: true, discovery: discoveryStatus("running") },
        wrapper,
      },
    );

    await waitFor(() => expect(result.current.isActive).toBe(true));
    rerender({ businessReady: true, discovery: discoveryStatus("complete") });
    await waitFor(() => expect(candidateReads).toBe(1));
    rerender({ businessReady: false, discovery: discoveryStatus("complete") });
    await act(async () => {
      resolveFirstCandidates?.([]);
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.isRefreshing).toBe(false));
    expect(result.current.isActive).toBe(true);

    rerender({ businessReady: true, discovery: discoveryStatus("complete") });
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("refresh_local_indexes", {
        clients: ["codex"],
        trigger: "discoveryBatch",
      }),
    );
    await waitFor(() => expect(result.current.isActive).toBe(false));
    expect(
      invokeMock.mock.calls.filter(([command]) => command === "refresh_local_indexes"),
    ).toHaveLength(1);
  });
});
