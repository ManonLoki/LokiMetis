import { render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";

import { PetOverlayPage } from "../src/pages/PetOverlayPage";
import { monitorImageBytesToDataUrl } from "../src/pet-overlay-image";
import { TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const pngMagic = [137, 80, 78, 71];

describe("desktop pet overlay page", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_pet_overlay_view") {
        return {
          slots: [
            { tool: "codex", name: "Codex", occupied: true, imageId: "img-1" },
            { tool: "claudeCode", name: "Claude Code", occupied: false, imageId: null },
            { tool: "grok", name: "Grok Build", occupied: false, imageId: null },
            { tool: "workBuddy", name: "WorkBuddy", occupied: false, imageId: null },
          ],
        };
      }
      if (command === "get_monitor_image_bytes") {
        return pngMagic;
      }
      throw new Error(`unexpected command ${command}`);
    });
  });

  /** 悬浮窗根渲染四项 Agent 槽位，不出现未批准工具。 */
  test("pet_overlay_page_renders_four_approved_slots", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    expect(await screen.findByTestId("pet-overlay-page")).toBeVisible();
    expect(await screen.findByText("Codex")).toBeVisible();
    expect(screen.getByText("Claude Code")).toBeVisible();
    expect(screen.getByText("Grok Build")).toBeVisible();
    expect(screen.getByText("WorkBuddy")).toBeVisible();
    expect(screen.queryByText(/Cursor/i)).not.toBeInTheDocument();
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("get_pet_overlay_view");
    });
  });

  /** 占用槽位必须用 data: URL，才能通过现有 img-src CSP。 */
  test("occupied_slot_uses_csp_allowed_data_url_not_blob", async () => {
    render(
      <TestProviders>
        <PetOverlayPage />
      </TestProviders>,
    );
    const image = await screen.findByRole("img", { name: "Codex" });
    const src = image.getAttribute("src") ?? "";
    expect(src).toBe(monitorImageBytesToDataUrl(pngMagic));
    expect(src.startsWith("data:image/png;base64,")).toBe(true);
    expect(src.startsWith("blob:")).toBe(false);
    expect(invokeMock).toHaveBeenCalledWith("get_monitor_image_bytes", { id: "img-1" });
  });
});
