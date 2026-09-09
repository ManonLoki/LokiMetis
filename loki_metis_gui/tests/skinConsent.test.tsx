import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";

import type { PreparedSkinImportBatch, SkinDescriptor } from "../src/api/skins";
import { ImportDialog } from "../src/components/skins/SkinDialogs";
import { TestProviders } from "./testUtils";

/** 构造只包含本测试所需字段的导入批次。 */
function importBatch(skins: SkinDescriptor[]): PreparedSkinImportBatch {
  return {
    items: skins.map((skin, index) => ({
      archiveName: `${skin.id}.zip`,
      itemId: `item-${index + 1}`,
      skin,
    })),
    skipped: [],
    token: "batch-1",
    totalFiles: skins.length,
  };
}

const legacySkin: SkinDescriptor = {
  author: "External author",
  id: "legacy-script",
  name: "Legacy Script",
  packageType: "legacySkin",
  previewDataUrl: "",
  source: "user",
  supportedColorModes: ["light"],
  version: "1.0.0",
};

const pureTheme: SkinDescriptor = {
  ...legacySkin,
  id: "pure-theme",
  name: "Pure Theme",
  packageType: "theme",
};

describe("skin import code consent", () => {
  /** 兼容皮肤导入必须明确披露第三方代码，且主动勾选前不能提交。 */
  test("requires an explicit acknowledgement for compatible skin imports", async () => {
    const onCommit = vi.fn();
    render(
      <TestProviders>
        <ImportDialog
          batch={importBatch([legacySkin])}
          onCancel={vi.fn()}
          onCommit={onCommit}
          onSelectedChange={vi.fn()}
          pending={false}
          selected={["item-1"]}
        />
      </TestProviders>,
    );

    expect(screen.getByText("Selected skins contain third-party code")).toBeVisible();
    expect(screen.getByText(/renderer-inject\.js/i)).toBeVisible();
    expect(screen.getByText(/do not review or prove the script is safe/i)).toBeVisible();
    expect(screen.queryByText(/safe preflight/i)).not.toBeInTheDocument();
    const commit = screen.getByRole("button", { name: "Import selected (1)" });
    expect(commit).toBeDisabled();

    await userEvent.click(
      screen.getByRole("checkbox", {
        name: /I understand these files contain third-party code/i,
      }),
    );
    expect(commit).toBeEnabled();
    await userEvent.click(commit);
    expect(onCommit).toHaveBeenCalledWith(true);
  });

  /** 纯主题继续走无脚本路径，不要求或伪造第三方代码授权。 */
  test("does not request script consent for a pure theme", async () => {
    const onCommit = vi.fn();
    render(
      <TestProviders>
        <ImportDialog
          batch={importBatch([pureTheme])}
          onCancel={vi.fn()}
          onCommit={onCommit}
          onSelectedChange={vi.fn()}
          pending={false}
          selected={["item-1"]}
        />
      </TestProviders>,
    );

    expect(
      screen.queryByText("Selected skins contain third-party code"),
    ).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Import selected (1)" }));
    expect(onCommit).toHaveBeenCalledWith(false);
  });

  /** 取消审核只关闭批次，不可误触提交回调。 */
  test("cancel never commits an untrusted import", async () => {
    const onCancel = vi.fn();
    const onCommit = vi.fn();
    render(
      <TestProviders>
        <ImportDialog
          batch={importBatch([legacySkin])}
          onCancel={onCancel}
          onCommit={onCommit}
          onSelectedChange={vi.fn()}
          pending={false}
          selected={["item-1"]}
        />
      </TestProviders>,
    );

    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();
    expect(onCommit).not.toHaveBeenCalled();
  });
});
