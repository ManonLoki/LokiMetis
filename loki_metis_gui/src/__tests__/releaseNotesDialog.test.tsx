import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";

import { ReleaseNotesDialogTemplate } from "../components/ReleaseNotesDialog";
import { TestProviders } from "../../tests/testUtils";

describe("release notes dialog", () => {
  /** 验证有限错误文案不泄漏底层路径，并由弹窗自身重试控件触发回调。 */
  test("shows a bounded release notes load failure and retries from its own control", async () => {
    const onRetry = vi.fn();
    render(
      <TestProviders>
        <ReleaseNotesDialogTemplate
          language="en-US"
          onClose={vi.fn()}
          onRetry={onRetry}
          opened
          releases={undefined}
          status="error"
        />
      </TestProviders>,
    );

    expect(
      screen.getByText(/not generated before the first formal release/i),
    ).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledOnce();
  });
});
