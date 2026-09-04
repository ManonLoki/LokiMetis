import "@testing-library/jest-dom/vitest";

import { cleanup } from "@testing-library/react";
import { afterEach, beforeAll, vi } from "vitest";

import { appI18n, initializeI18n } from "../src/i18n";

/** 为 jsdom 提供 Mantine 布局观察所需的最小宿主实现。 */
class ResizeObserverMock {
  /** 测试不需要实际观察布局。 */
  observe(): void {}

  /** 测试不需要移除单个观察目标。 */
  unobserve(): void {}

  /** 测试结束时没有宿主资源需要释放。 */
  disconnect(): void {}
}

globalThis.ResizeObserver = ResizeObserverMock;

const storage = new Map<string, string>();
Object.defineProperty(window, "localStorage", {
  configurable: true,
  value: {
    clear: () => storage.clear(),
    getItem: (key: string) => storage.get(key) ?? null,
    key: (index: number) => [...storage.keys()][index] ?? null,
    /** 返回测试存储中的当前条目数。 */
    get length() {
      return storage.size;
    },
    removeItem: (key: string) => storage.delete(key),
    setItem: (key: string, value: string) => storage.set(key, value),
  },
});

Object.defineProperty(window, "matchMedia", {
  configurable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    addEventListener: vi.fn(),
    addListener: vi.fn(),
    dispatchEvent: vi.fn(),
    matches: false,
    media: query,
    onchange: null,
    removeEventListener: vi.fn(),
    removeListener: vi.fn(),
  })),
  writable: true,
});

beforeAll(async () => {
  await initializeI18n();
});

afterEach(async () => {
  cleanup();
  window.localStorage.clear();
  await appI18n.changeLanguage("en-US");
});
