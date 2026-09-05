import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";

import { MonitorImagesPage } from "../src/pages/MonitorImagesPage";
import { monitorCapabilitiesFixture, TestProviders } from "./testUtils";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

/** 含 JPEG/PNG/GIF 的本机图库快照。 */
function mixedGallery() {
  return {
    images: [
      {
        id: "img-jpeg",
        filename: "photo.jpg",
        format: "jpeg",
        image: "data:image/jpeg;base64,/9j/4AAQ=",
      },
      {
        id: "img-png",
        filename: "icon.png",
        format: "png",
        image: "data:image/png;base64,iVBORw0KGgo=",
      },
      {
        id: "img-gif",
        filename: "loop.gif",
        format: "gif",
        image: "data:image/gif;base64,R0lGODlh=",
      },
    ],
    counts: { jpeg: 1, png: 1, gif: 1 },
  };
}

describe("monitor images page", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  /** 空库显示空态，上传走本机存储命令。 */
  test("empty_library_shows_empty_state_and_uploads_via_local_command", async () => {
    const empty = { images: [], counts: { jpeg: 0, png: 0, gif: 0 } };
    invokeMock.mockImplementation(async (command: string) => {
      if (command === "get_monitor_capabilities") {
        return monitorCapabilitiesFixture({ aiTools: [] });
      }
      if (command === "list_monitor_images_cmd") return empty;
      if (command === "save_monitor_image_cmd") return mixedGallery();
      throw new Error(`unexpected command ${command}`);
    });
    const { container } = render(
      <TestProviders>
        <MonitorImagesPage />
      </TestProviders>,
    );
    expect(await screen.findByText("No local images yet")).toBeVisible();
    expect(screen.getByRole("button", { name: "Choose images" })).toBeVisible();
    expect(screen.queryByText(/remote/i)).not.toBeInTheDocument();
    const fileInput = container.querySelector('input[type="file"]');
    expect(fileInput).not.toBeNull();
    const file = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], "icon.png", {
      type: "image/png",
    });
    await userEvent.upload(fileInput as HTMLInputElement, file);
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        "save_monitor_image_cmd",
        expect.objectContaining({ filename: "icon.png", bytes: expect.any(Array) }),
      );
    });
    expect(invokeMock).not.toHaveBeenCalledWith("upload_remote_images");
  });

  /** 对本机图库快照断言筛选控件、预览卡片与删除走本机命令。 */
  test("gallery_snapshot_shows_filters_preview_cards_and_local_delete", async () => {
    invokeMock.mockImplementation(async (command: string, payload?: { id?: string }) => {
      if (command === "get_monitor_capabilities") {
        return monitorCapabilitiesFixture({ aiTools: [] });
      }
      if (command === "list_monitor_images_cmd") return mixedGallery();
      if (command === "delete_monitor_image_cmd") {
        expect(payload?.id).toBe("img-png");
        return {
          images: mixedGallery().images.filter((item) => item.id !== "img-png"),
          counts: { jpeg: 1, png: 0, gif: 1 },
        };
      }
      throw new Error(`unexpected command ${command}`);
    });
    render(
      <TestProviders>
        <MonitorImagesPage />
      </TestProviders>,
    );
    expect(await screen.findByText("3 images")).toBeVisible();
    expect(screen.getByRole("radio", { name: "All 3" })).toBeVisible();
    expect(screen.getByRole("radio", { name: "JPEG 1" })).toBeVisible();
    expect(screen.getByRole("radio", { name: "PNG 1" })).toBeVisible();
    expect(screen.getByRole("radio", { name: "GIF 1" })).toBeVisible();
    expect(screen.getByRole("img", { name: "photo.jpg" })).toHaveAttribute(
      "src",
      "data:image/jpeg;base64,/9j/4AAQ=",
    );
    expect(screen.getByText("JPEG")).toBeVisible();
    expect(screen.getByText("PNG")).toBeVisible();
    expect(screen.getByText("GIF")).toBeVisible();
    expect(screen.getAllByText("Local library")).toHaveLength(3);
    await userEvent.click(screen.getByRole("radio", { name: "PNG 1" }));
    expect(screen.getByRole("img", { name: "icon.png" })).toBeVisible();
    expect(screen.queryByRole("img", { name: "photo.jpg" })).not.toBeInTheDocument();
    await userEvent.click(screen.getAllByRole("button", { name: "···" })[0]);
    await userEvent.click(await screen.findByRole("menuitem", { name: "Delete image" }));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("delete_monitor_image_cmd", { id: "img-png" });
    });
    expect(invokeMock).not.toHaveBeenCalledWith("delete_remote_image");
  });
});
