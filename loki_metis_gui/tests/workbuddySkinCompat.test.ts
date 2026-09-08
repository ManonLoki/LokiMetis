// @ts-expect-error jsdom 是 Vitest 已锁定的运行时依赖，但当前工作区未单列其类型包。
import { JSDOM } from "jsdom";
import { describe, expect, test } from "vitest";

import adapterSource from "../resources/workbuddy-skin-host-compat.js?raw";

/** 描述适配器实际创建并负责清理的单个别名记录。 */
interface OwnedAlias {
  className: string;
  ruleId: string;
}

/** 描述 WorkBuddy 宿主适配器暴露的可观察生命周期状态。 */
interface CompatibilityState {
  appliedRules: string[];
  cleanup: () => boolean;
  ensure: () => unknown;
  host: string;
  ownedAliases: OwnedAlias[];
  skippedRules: string[];
  version: string;
}

/** 构造与 WorkBuddy 5.3.5 ChatInput 一致的无内容输入区层级。 */
function composerMarkup(id: string): string {
  return `<section id="${id}" class="cb-chat-input-root"><div class="cb-chat-input-content"><div class="cb-chat-input-main-area"><div role="textbox" contenteditable="true"></div></div><div class="cb-chat-input-toolbar"><span data-cb-chat-input-toolbar-selector="true"></span><span data-cb-chat-input-toolbar-right="true"></span></div></div></section>`;
}

/** 构造不含任何真实会话内容的 WorkBuddy 5.3.5 结构夹具。 */
function workBuddyDom(home = false): JSDOM {
  const content = home
    ? `<div class="wb-cb-chat"><div class="wb-home-page"><section class="wb-home-composer"><div class="wb-home-composer__input-slot">${composerMarkup("home-composer")}</div></section></div></div>`
    : `<div class="wb-cb-chat"><div class="chat-container">${composerMarkup("conversation-composer")}</div></div>`;
  return new JSDOM(
    `<!doctype html><html><head><title>WorkBuddy</title></head><body data-application-name="workbuddy" data-electron-desktop="true" data-product-name="WorkBuddy"><div id="root"><div class="teams-container"><div id="workbuddy-menubar-container"><div role="menubar"></div></div><div id="workbuddy-window-controls-container"></div><div data-view-id="sidebar"><div class="conversation-sidebar"><div class="conversation-list"></div></div></div><div data-view-id="main-content"><div class="teams-main-content"><div class="main-content">${content}</div></div></div></div></div></body></html>`,
    { runScripts: "dangerously", url: "file:///WorkBuddy/renderer/index.html" },
  );
}

/** 在隔离 JSDOM 中执行仓库内的只读宿主适配器。 */
function installAdapter(dom: JSDOM): CompatibilityState | undefined {
  const script = dom.window.document.createElement("script");
  script.textContent = adapterSource;
  dom.window.document.body.appendChild(script);
  return (dom.window as unknown as Record<string, CompatibilityState | undefined>)[
    "__BIFANG_CODEX_SKIN_COMPAT__"
  ];
}

/** 等待宿主适配器的 160ms 合并窗口完成一次重算。 */
function waitForReconcile(dom: JSDOM): Promise<void> {
  return new Promise((resolve) => dom.window.setTimeout(resolve, 240));
}

describe("WorkBuddy skin host compatibility", () => {
  /** 欢迎页和普通会话都只把别名添加到真实 ChatInput section。 */
  test.each([
    ["home", true, "home-composer"],
    ["conversation", false, "conversation-composer"],
  ])("maps_%s_surfaces_and_cleans_every_owned_artifact", (_name, home, composerId) => {
    const dom = workBuddyDom(home);
    const state = installAdapter(dom);
    const document = dom.window.document;

    expect(state?.host).toBe("workBuddy");
    expect(state?.version).toBe("5");
    expect(
      document.querySelectorAll("#loki-metis-workbuddy-skin-compat-style"),
    ).toHaveLength(1);
    expect(document.querySelector(".teams-container")).toHaveClass(
      "loki-metis-workbuddy-shell",
    );
    expect(document.querySelector(".conversation-list")).toHaveClass(
      "loki-metis-workbuddy-sidebar-surface",
    );
    expect(document.querySelector(".main-content")).toHaveClass(
      "loki-metis-workbuddy-main-surface",
    );
    expect(document.querySelector(`#${composerId}`)).toHaveClass(
      "loki-metis-workbuddy-composer",
    );
    if (home) {
      expect(document.querySelector(".wb-home-composer")).not.toHaveClass(
        "loki-metis-workbuddy-composer",
      );
      expect(document.querySelector(".wb-home-composer__input-slot")).not.toHaveClass(
        "loki-metis-workbuddy-composer",
      );
    }
    expect(document.documentElement).toHaveAttribute(
      "data-loki-metis-workbuddy-skin-compat",
      "5",
    );
    const styleText = document.querySelector("style")?.textContent;
    expect(styleText).toContain("--skin-background-image");
    expect(styleText).toContain("--dream-skin-art");
    expect(styleText).toContain("--cb-content-background: transparent !important");
    expect(styleText).toContain(
      "--cb-content-border-color: var(--skin-line, GrayText) !important",
    );
    expect(styleText).toContain("--cb-main-area-background: transparent !important");
    expect(styleText).toContain(
      "--cb-main-area-border-color: var(--skin-line, GrayText) !important",
    );
    expect(styleText).toContain("--cb-main-area-box-shadow: none !important");

    const second = installAdapter(dom);
    expect(second).toBe(state);
    expect(
      document.querySelectorAll("#loki-metis-workbuddy-skin-compat-style"),
    ).toHaveLength(1);
    expect(document.querySelectorAll(".loki-metis-workbuddy-composer")).toHaveLength(1);

    expect(state?.cleanup()).toBe(true);
    expect(state?.cleanup()).toBe(true);
    expect(document.querySelector("#loki-metis-workbuddy-skin-compat-style")).toBeNull();
    expect(document.querySelector("[class*='loki-metis-workbuddy-']")).toBeNull();
    expect(document.documentElement).not.toHaveAttribute(
      "data-loki-metis-workbuddy-skin-compat",
    );
    expect(
      (dom.window as unknown as Record<string, unknown>)["__BIFANG_CODEX_SKIN_COMPAT__"],
    ).toBeUndefined();
    dom.window.close();
  });

  /** 安装前已存在的兼容别名不进入适配器所有权，也不在清理时删除。 */
  test("preserves_a_preexisting_composer_alias_during_cleanup", () => {
    const dom = workBuddyDom();
    const composer = dom.window.document.querySelector("#conversation-composer");
    composer?.classList.add("loki-metis-workbuddy-composer");

    const state = installAdapter(dom);

    expect(
      state?.ownedAliases.some(
        (entry) => entry.className === "loki-metis-workbuddy-composer",
      ),
    ).toBe(false);
    expect(state?.cleanup()).toBe(true);
    expect(composer).toHaveClass("loki-metis-workbuddy-composer");
    dom.window.close();
  });

  /** WorkBuddy 重写稳定节点 class 时，观察器会恢复适配器拥有的别名。 */
  test("restores_owned_aliases_after_host_class_rewrites", async () => {
    const dom = workBuddyDom();
    const state = installAdapter(dom);
    const shell = dom.window.document.querySelector(".teams-container");
    const composer = dom.window.document.querySelector("#conversation-composer");

    shell?.setAttribute("class", "teams-container");
    composer?.setAttribute("class", "cb-chat-input-root");
    await waitForReconcile(dom);

    expect(shell).toHaveClass("loki-metis-workbuddy-shell");
    expect(composer).toHaveClass("loki-metis-workbuddy-composer");
    expect(
      state?.ownedAliases.filter(
        (entry) => entry.className === "loki-metis-workbuddy-composer",
      ),
    ).toHaveLength(1);
    state?.cleanup();
    dom.window.close();
  });

  /** 输入区候选迁移时只保留当前唯一候选的兼容别名。 */
  test("moves_the_composer_alias_to_the_current_unique_candidate", async () => {
    const dom = workBuddyDom();
    const state = installAdapter(dom);
    const document = dom.window.document;
    const oldComposer = document.querySelector("#conversation-composer");
    const container = oldComposer?.parentElement;

    oldComposer?.replaceChildren();
    container?.insertAdjacentHTML("beforeend", composerMarkup("replacement-composer"));
    await waitForReconcile(dom);

    expect(oldComposer).not.toHaveClass("loki-metis-workbuddy-composer");
    expect(document.querySelector("#replacement-composer")).toHaveClass(
      "loki-metis-workbuddy-composer",
    );
    expect(state?.appliedRules).toContain("workbuddy-composer-surface");
    expect(state?.skippedRules).not.toContain("workbuddy-composer-surface");
    state?.cleanup();
    dom.window.close();
  });

  /** 多个有效输入区候选会撤销旧别名，并以 skipped 明确报告歧义。 */
  test("removes_the_composer_alias_and_reports_ambiguous_candidates", async () => {
    const dom = workBuddyDom();
    const state = installAdapter(dom);
    const document = dom.window.document;
    const composer = document.querySelector("#conversation-composer");

    composer?.parentElement?.insertAdjacentHTML(
      "beforeend",
      composerMarkup("ambiguous-composer"),
    );
    await waitForReconcile(dom);

    expect(document.querySelectorAll(".loki-metis-workbuddy-composer")).toHaveLength(0);
    expect(state?.appliedRules).not.toContain("workbuddy-composer-surface");
    expect(state?.skippedRules).toContain("workbuddy-composer-surface");
    state?.cleanup();
    dom.window.close();
  });

  /** 聊天根存在但语义探针不完整时，composer 规则必须明确降级。 */
  test("reports_a_missing_composer_candidate_as_skipped", () => {
    const dom = workBuddyDom();
    dom.window.document.querySelector("#conversation-composer")?.replaceChildren();

    const state = installAdapter(dom);

    expect(state?.appliedRules).not.toContain("workbuddy-composer-surface");
    expect(state?.skippedRules).toContain("workbuddy-composer-surface");
    state?.cleanup();
    dom.window.close();
  });

  /** 仅伪造标题和根节点的普通本机页面不得安装适配器。 */
  test("rejects_a_local_page_without_WorkBuddy_body_markers", () => {
    const dom = new JSDOM(
      "<!doctype html><html><head><title>WorkBuddy</title></head><body><div id='root'></div></body></html>",
      { runScripts: "dangerously", url: "file:///tmp/index.html" },
    );
    expect(installAdapter(dom)).toBeUndefined();
    expect(
      dom.window.document.querySelector("#loki-metis-workbuddy-skin-compat-style"),
    ).toBeNull();
    dom.window.close();
  });
});
