import { createStore } from "jotai/vanilla";
import { describe, expect, it } from "vitest";

import {
  createPageSessionState,
  type PageSessionState,
} from "./pageSessionState";

type TestTab = "all" | "failed";

const DEFAULT_STATE: PageSessionState<TestTab, { keyword: string }> = {
  activeTab: "all",
  query: { keyword: "" },
  page: 1,
  pageSize: 20,
};

const pageSession = createPageSessionState(DEFAULT_STATE);

describe("page session state", () => {
  it("keeps tabs queries and pagination when a route unmounts and remounts", () => {
    const applicationStore = createStore();

    applicationStore.set(pageSession.setSelectionAtom, {
      activeTab: "failed",
      query: { keyword: "needle" },
    });
    applicationStore.set(pageSession.setPaginationAtom, {
      page: 1,
      pageSize: 50,
    });
    applicationStore.set(pageSession.setPaginationAtom, {
      page: 4,
      pageSize: 50,
    });

    expect(applicationStore.get(pageSession.stateAtom)).toEqual({
      activeTab: "failed",
      query: { keyword: "needle" },
      page: 4,
      pageSize: 50,
    });
  });

  it("resets to page one when the page size changes", () => {
    const applicationStore = createStore();
    applicationStore.set(pageSession.setPaginationAtom, { page: 4, pageSize: 20 });
    applicationStore.set(pageSession.setPaginationAtom, { page: 4, pageSize: 50 });

    expect(applicationStore.get(pageSession.stateAtom)).toMatchObject({
      page: 1,
      pageSize: 50,
    });
  });

  it("resets to page one when the tab or query scope changes", () => {
    const applicationStore = createStore();
    applicationStore.set(pageSession.setPaginationAtom, { page: 4, pageSize: 20 });
    applicationStore.set(pageSession.setSelectionAtom, {
      activeTab: "failed",
      query: { keyword: "new scope" },
    });

    expect(applicationStore.get(pageSession.stateAtom)).toMatchObject({
      activeTab: "failed",
      query: { keyword: "new scope" },
      page: 1,
    });
  });

  it("falls back to page one only after a successful empty page result", () => {
    const applicationStore = createStore();
    applicationStore.set(pageSession.setPaginationAtom, { page: 3, pageSize: 20 });

    expect(
      applicationStore.set(pageSession.reconcilePageAfterResultAtom, {
        status: "success",
        itemCount: 0,
      }),
    ).toBe(true);
    expect(applicationStore.get(pageSession.stateAtom).page).toBe(1);
  });

  it("does not treat loading errors or an empty first page as a stale page", () => {
    const applicationStore = createStore();

    for (const status of ["loading", "error"] as const) {
      applicationStore.set(pageSession.setPaginationAtom, { page: 3, pageSize: 20 });
      expect(
        applicationStore.set(pageSession.reconcilePageAfterResultAtom, {
          status,
          itemCount: 0,
        }),
      ).toBe(false);
      expect(applicationStore.get(pageSession.stateAtom).page).toBe(3);
    }

    applicationStore.set(pageSession.setPaginationAtom, { page: 1, pageSize: 20 });
    expect(
      applicationStore.set(pageSession.reconcilePageAfterResultAtom, {
        status: "success",
        itemCount: 0,
      }),
    ).toBe(false);
  });

  it("starts from defaults in a new application store", () => {
    const firstProgramStore = createStore();
    firstProgramStore.set(pageSession.setPaginationAtom, { page: 5, pageSize: 100 });

    const nextProgramStore = createStore();
    expect(nextProgramStore.get(pageSession.stateAtom)).toEqual(DEFAULT_STATE);
  });
});
