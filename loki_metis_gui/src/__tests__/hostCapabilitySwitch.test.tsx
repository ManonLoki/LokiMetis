import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";

import { HostCapabilitySwitch } from "../components/HostCapabilitySwitch";
import { TestProviders } from "../../tests/testUtils";

describe("authoritative host capability switch", () => {
  /** 验证写入失败后回读实际状态、回滚开关并提供可操作错误。 */
  test("system_notification_switch_rolls_back_after_denial", async () => {
    const getEnabled = vi.fn().mockResolvedValue(false);
    const setEnabled = vi.fn().mockRejectedValue(new Error("host rejected"));
    render(
      <TestProviders>
        <HostCapabilitySwitch
          getEnabled={getEnabled}
          id="system_notification"
          queryKey={["test-notification"]}
          setEnabled={setEnabled}
        />
      </TestProviders>,
    );

    const toggle = await screen.findByRole("switch", { name: "System notifications" });
    await waitFor(() => expect(toggle).toBeEnabled());
    await userEvent.click(toggle);

    await screen.findByRole("alert");
    expect(toggle).not.toBeChecked();
    expect(getEnabled).toHaveBeenCalledTimes(2);
    expect(getEnabled).toHaveBeenNthCalledWith(1);
    expect(getEnabled).toHaveBeenNthCalledWith(2);
    expect(screen.getByText(/Check system notification permission/i)).toBeVisible();
  });

  /** 验证开机自启写入失败后同样回读并回滚实际状态。 */
  test("autostart_switch_rolls_back_after_failure", async () => {
    const getEnabled = vi.fn().mockResolvedValue(false);
    const setEnabled = vi.fn().mockRejectedValue(new Error("host rejected"));
    render(
      <TestProviders>
        <HostCapabilitySwitch
          getEnabled={getEnabled}
          id="autostart"
          queryKey={["test-autostart"]}
          setEnabled={setEnabled}
        />
      </TestProviders>,
    );

    const toggle = await screen.findByRole("switch", { name: "Start at login" });
    await waitFor(() => expect(toggle).toBeEnabled());
    await userEvent.click(toggle);

    await screen.findByRole("alert");
    expect(toggle).not.toBeChecked();
    expect(getEnabled).toHaveBeenCalledTimes(2);
    expect(getEnabled).toHaveBeenNthCalledWith(1);
    expect(getEnabled).toHaveBeenNthCalledWith(2);
    expect(screen.getByText(/Check system login items/i)).toBeVisible();
  });
});
