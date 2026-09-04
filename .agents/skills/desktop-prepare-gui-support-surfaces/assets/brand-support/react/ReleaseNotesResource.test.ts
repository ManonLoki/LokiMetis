import { describe, expect, it, vi } from "vitest";

import {
  LOAD_RELEASE_NOTES_COMMAND,
  decodeReleaseNotesDocument,
  loadBundledReleaseNotes,
} from "./releaseNotesResource";

const VALID_DOCUMENT = {
  releases: [
    {
      bugFixes: [{ "en-US": "Fix resource display", "zh-CN": "修复资源展示" }],
      featureOptimizations: [
        {
          "en-US": "Add bundled release notes",
          "zh-CN": "新增候选内更新日志",
        },
      ],
      releaseDate: "2026-08-27",
      version: "v1.2.3",
    },
  ],
  schemaVersion: 2,
};

describe("bundled release notes resource", () => {
  /** 加载器只调用固定窄命令，并返回经过校验的候选资源。 */
  it("loads the packaged document through the narrow Tauri command", async () => {
    const invokeCommand = vi.fn(async () => VALID_DOCUMENT);

    await expect(loadBundledReleaseNotes(invokeCommand)).resolves.toEqual(
      VALID_DOCUMENT.releases,
    );
    expect(invokeCommand).toHaveBeenCalledOnce();
    expect(invokeCommand).toHaveBeenCalledWith(LOAD_RELEASE_NOTES_COMMAND);
  });

  /** 未知字段、多重 v 前缀和空版本条目必须在 IPC 边界失败关闭。 */
  it("rejects malformed or unbounded IPC documents", () => {
    expect(() =>
      decodeReleaseNotesDocument({ ...VALID_DOCUMENT, unexpected: true }),
    ).toThrow("invalid release notes document");
    expect(() =>
      decodeReleaseNotesDocument({
        ...VALID_DOCUMENT,
        releases: [
          {
            ...VALID_DOCUMENT.releases[0],
            version: "vv1.2.3",
          },
        ],
      }),
    ).toThrow("invalid release notes entry");
    expect(() =>
      decodeReleaseNotesDocument({
        ...VALID_DOCUMENT,
        releases: [
          {
            ...VALID_DOCUMENT.releases[0],
            featureOptimizations: [{ "zh-CN": "缺少英文翻译" }],
          },
        ],
      }),
    ).toThrow("invalid release notes featureOptimizations item");
  });

  /** 版本日期必须最新在前，不能依赖页面裁剪掩盖资源顺序错误。 */
  it("rejects releases that are not ordered newest first", () => {
    expect(() =>
      decodeReleaseNotesDocument({
        releases: [
          VALID_DOCUMENT.releases[0],
          {
            ...VALID_DOCUMENT.releases[0],
            releaseDate: "2026-08-28",
            version: "v1.2.2",
          },
        ],
        schemaVersion: 2,
      }),
    ).toThrow("invalid release notes entry");
  });
});
