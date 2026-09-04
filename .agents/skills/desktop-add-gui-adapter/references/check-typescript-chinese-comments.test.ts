/** 覆盖 GUI TypeScript 中文声明注释门禁的收敛范围与失败关闭边界。 */

import { createRequire } from "node:module"
import { mkdtemp, rm, symlink, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import path from "node:path"
import { afterEach, describe, expect, test } from "vitest"

/** 描述参考检查器的单个语法或运行错误。 */
interface CheckerError {
  file?: string
  line?: number
  column?: number
  message?: string
}

/** 描述参考检查器的单个声明违规。 */
interface CheckerViolation {
  file: string
  line: number
  column: number
  kind: string
  name: string
  message: string
}

/** 描述单文件检查结果。 */
interface SourceResult {
  checkedDeclarations: number
  errors: CheckerError[]
  violations: CheckerViolation[]
}

/** 描述完整扫描根检查结果。 */
interface ProjectResult {
  ok: boolean
  checkedFiles: number
  checkedDeclarations: number
  errors: string[]
  violations: CheckerViolation[]
}

/** 描述 CommonJS 参考检查器对测试暴露的稳定接口。 */
interface CheckerModule {
  inspectChineseComments(source: string, filePath: string): SourceResult
  inspectProject(root: string): ProjectResult
  isChineseCommentPolicyExcluded(relativePath: string, isDirectory?: boolean): boolean
}

const require = createRequire(import.meta.url)
const checker = require("./check-typescript-chinese-comments.cjs") as CheckerModule
const temporaryRoots: string[] = []

afterEach(async () => {
  await Promise.all(temporaryRoots.splice(0).map((root) => rm(root, { force: true, recursive: true })))
})

describe("TypeScript Chinese declaration comments", () => {
  test("accepts governed declarations without requiring locals or inline callbacks", () => {
    const source = `
/** 定义账户传输结构。 */
interface Account { id: string }
/** 表示账户编号。 */
type AccountId = string
/** 表示账户状态。 */
enum AccountState { Ready }
/** 封装账户读取能力。 */
class AccountReader {
  /** 初始化账户读取器。 */
  constructor() {}
  /** 读取账户。 */
  read(): Account { return { id: "1" } }
}
/** 创建账户读取器。 */
function createReader(): AccountReader { return new AccountReader() }
/** 渲染账户组件。 */
export const AccountView = () => [1].map((value) => value).join("")
/** 提供账户读取状态。 */
export const useAccount = () => {
  const local = createReader()
  return local.read()
}
/** 验证账户读取成功。 */
test("reads an account", () => expect(createReader().read().id).toBe("1"))
`

    const result = checker.inspectChineseComments(source, "src/account.test.tsx")
    expect(result.errors).toEqual([])
    expect(result.violations).toEqual([])
    expect(result.checkedDeclarations).toBe(10)
  })

  /** 验证延迟命名导出和默认导出不会绕过组件或 hook 门禁。 */
  test("tracks components and hooks exported after their declarations", () => {
    const source = `
/** 渲染延迟导出的账户组件。 */
const DeferredView = () => null
const useDeferred = () => undefined
/** 渲染默认导出的账户组件。 */
const DefaultView = () => null
const LocalView = () => null
export { DeferredView, useDeferred }
export default DefaultView
`

    const result = checker.inspectChineseComments(source, "src/deferred.tsx")
    expect(result.errors).toEqual([])
    expect(result.checkedDeclarations).toBe(3)
    expect(result.violations.map((violation) => violation.name)).toEqual(["useDeferred"])
  })

  /** 验证业务同名调用不冒充场景，显式 Vitest 别名仍纳入门禁。 */
  test("recognizes test scenarios only from test files or Vitest imports", () => {
    const business = `
/** 执行业务健康检查。 */
function runWorkflow() { test("health-check") }
`
    const businessResult = checker.inspectChineseComments(business, "src/workflow.ts")
    expect(businessResult.errors).toEqual([])
    expect(businessResult.violations).toEqual([])
    expect(businessResult.checkedDeclarations).toBe(1)

    const scenario = `
import { test as scenario } from "vitest"
/** 验证账户场景。 */
scenario("account", () => undefined)
`
    const scenarioResult = checker.inspectChineseComments(scenario, "src/scenarios.ts")
    expect(scenarioResult.errors).toEqual([])
    expect(scenarioResult.violations).toEqual([])
    expect(scenarioResult.checkedDeclarations).toBe(1)
  })

  test.each([
    ["missing", "interface Missing { value: string }"],
    ["English only", "/** Stores state. */\ntype State = string"],
    ["detached", "/** 表示状态。 */\n\nenum State { Ready }"],
    ["Chinese string", 'function load() { return "中文不是注释" }'],
    ["unrelated trailing", "const value = 1 // 定义状态。\ninterface State {}"]
  ])("rejects %s evidence", (_label, source) => {
    const result = checker.inspectChineseComments(source, "src/invalid.ts")
    expect(result.errors).toEqual([])
    expect(result.violations).not.toEqual([])
  })

  test("fails closed on parse diagnostics", () => {
    const result = checker.inspectChineseComments("/** 读取账户。 */\nfunction load(", "src/broken.ts")
    expect(result.errors).not.toEqual([])
    expect(result.violations).toEqual([])
  })

  test("uses exact generated-file and directory exclusions", () => {
    expect(checker.isChineseCommentPolicyExcluded("src/routeTree.gen.ts")).toBe(true)
    expect(checker.isChineseCommentPolicyExcluded("src/nested/routeTree.gen.ts")).toBe(false)
    expect(checker.isChineseCommentPolicyExcluded("src/build/view.ts")).toBe(false)
    expect(checker.isChineseCommentPolicyExcluded("dist/app.js")).toBe(true)
    expect(checker.isChineseCommentPolicyExcluded("feature/node_modules/pkg/index.ts")).toBe(true)
  })

  test("fails closed on invalid UTF-8 and source symlinks", async () => {
    const root = await mkdtemp(path.join(tmpdir(), "ts-comment-gate-"))
    temporaryRoots.push(root)
    await writeFile(path.join(root, "valid.ts"), "/** 定义状态。 */\ninterface State {}\n")
    await writeFile(path.join(root, "invalid.ts"), Uint8Array.from([0xff, 0xfe]))
    try {
      await symlink(path.join(root, "valid.ts"), path.join(root, "linked.ts"))
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EPERM") throw error
    }

    const report = checker.inspectProject(root)
    expect(report.ok).toBe(false)
    expect(report.errors.some((error) => error.includes("无法安全读取源码"))).toBe(true)
    if (process.platform !== "win32") {
      expect(report.errors.some((error) => error.includes("符号链接"))).toBe(true)
    }
  })
})
