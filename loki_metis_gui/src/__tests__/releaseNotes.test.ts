import { describe, expect, test, vi } from "vitest";

import {
  LOAD_RELEASE_NOTES_COMMAND,
  decodeReleaseNotesDocument,
  loadBundledReleaseNotes,
  selectVisibleReleaseNotes,
} from "../lib/releaseNotes";

const validDocument = {
  releases: [
    {
      bugFixes: [],
      featureOptimizations: [{ "en-US": "Desktop foundation", "zh-CN": "桌面基础" }],
      releaseDate: "2026-09-05",
      version: "v0.1.0",
    },
  ],
  schemaVersion: 2,
} as const;

describe("local release notes", () => {
  /** 验证前端只调用固定窄命令并接受完整 schema v2。 */
  test("loads the packaged document through the narrow Tauri command", async () => {
    const invoker = vi.fn().mockResolvedValue(validDocument);
    await expect(loadBundledReleaseNotes(invoker)).resolves.toMatchObject({
      schemaVersion: 2,
    });
    expect(invoker).toHaveBeenCalledWith(LOAD_RELEASE_NOTES_COMMAND);
  });

  /** 验证空资源、非规范版本及额外字段都失败关闭。 */
  test("rejects malformed candidate resources", () => {
    expect(() => decodeReleaseNotesDocument({ releases: [], schemaVersion: 2 })).toThrow();
    expect(() =>
      decodeReleaseNotesDocument({
        ...validDocument,
        releases: [{ ...validDocument.releases[0], version: "0.1.0" }],
      }),
    ).toThrow();
    expect(() => decodeReleaseNotesDocument({ ...validDocument, extra: true })).toThrow();
  });

  /** 验证双语内容只在展示边界选择当前语言。 */
  test("selects English release-note translations from the active locale", () => {
    const document = decodeReleaseNotesDocument(validDocument);
    expect(selectVisibleReleaseNotes(document, "en-US")[0]?.featureOptimizations).toEqual([
      "Desktop foundation",
    ]);
  });
});
