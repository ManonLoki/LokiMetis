#!/usr/bin/env node
"use strict"

/**
 * GUI 下游使用的 TypeScript/TSX 中文声明注释存在性门禁参考实现。
 *
 * 该工具只检查稳定 AST 声明，不检查局部变量或普通匿名回调，也不会修改源码。
 */

const fs = require("node:fs")
const path = require("node:path")
const { TextDecoder } = require("node:util")
const ts = require("typescript")

/** 匹配至少一个汉字，只作为存在性证据。 */
const HAN_CHARACTER = /\p{Script=Han}/u
/** 只允许精确排除由路由工具生成且禁止手改的文件。 */
const EXACT_EXCLUDED_FILES = new Set(["src/routeTree.gen.ts"])
/** 只从扫描根排除依赖、版本控制和最终产物目录。 */
const EXCLUDED_ROOT_DIRECTORIES = new Set([".git", "coverage", "dist", "node_modules"])
/** 嵌套依赖和版本控制目录同样不属于人工源码。 */
const EXCLUDED_ANYWHERE_DIRECTORIES = new Set([".git", "node_modules"])
/** 识别 Vitest 风格的真实测试场景入口。 */
const TEST_CALL_NAMES = new Set(["it", "test"])

/** 将跨平台相对路径统一为 POSIX 形式。 */
function normalizeRelativePath(relativePath) {
  return relativePath.split(path.sep).join("/")
}

/** 判断路径是否命中精确生成物或封闭目录排除。 */
function isChineseCommentPolicyExcluded(relativePath, isDirectory = false) {
  const normalized = normalizeRelativePath(relativePath).replace(/^\.\//, "")
  if (!isDirectory && EXACT_EXCLUDED_FILES.has(normalized)) return true
  const segments = normalized.split("/").filter(Boolean)
  if (segments.length === 0) return false
  if (EXCLUDED_ROOT_DIRECTORIES.has(segments[0])) return true
  return segments.some((segment) => EXCLUDED_ANYWHERE_DIRECTORIES.has(segment))
}

/** 根据扩展名选择 TypeScript 或 TSX 解析模式。 */
function scriptKind(filePath) {
  return filePath.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS
}

/** 返回声明的稳定名称；匿名默认导出仍使用可诊断占位名。 */
function declarationName(node) {
  const name = node.name
  if (name !== undefined && typeof name.getText === "function") {
    return name.getText().replace(/\s+/g, " ").trim().slice(0, 80)
  }
  return "<anonymous>"
}

/** 判断声明是否具有 export 修饰符。 */
function isExported(node) {
  return node.modifiers?.some((modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword) === true
}

/** 判断导出变量是否是 React 组件或 hook 的稳定命名。 */
function isComponentOrHookName(name) {
  return /^[A-Z]/.test(name) || /^use[A-Z0-9_]/.test(name)
}

/** 收集当前模块直接或延迟导出的本地绑定名。 */
function locallyExportedNames(sourceFile) {
  const names = new Set()
  for (const statement of sourceFile.statements) {
    if (ts.isVariableStatement(statement) && isExported(statement)) {
      for (const declaration of statement.declarationList.declarations) {
        if (ts.isIdentifier(declaration.name)) names.add(declaration.name.text)
      }
    } else if (
      ts.isExportDeclaration(statement) &&
      statement.moduleSpecifier === undefined &&
      statement.exportClause !== undefined &&
      ts.isNamedExports(statement.exportClause)
    ) {
      for (const element of statement.exportClause.elements) {
        names.add((element.propertyName ?? element.name).text)
      }
    } else if (
      ts.isExportAssignment(statement) &&
      ts.isIdentifier(statement.expression)
    ) {
      names.add(statement.expression.text)
    }
  }
  return names
}

/** 判断文件路径是否明确属于 Vitest 场景文件。 */
function isTestSourceFile(fileName) {
  const normalized = normalizeRelativePath(fileName)
  const segments = normalized.split("/")
  return (
    /\.(?:test|spec)\.tsx?$/.test(normalized) ||
    segments.some((segment) => segment === "test" || segment === "tests" || segment === "__tests__")
  )
}

/** 收集从 Vitest 显式导入的本地 test/it 绑定名。 */
function importedVitestTestNames(sourceFile) {
  const names = new Set()
  for (const statement of sourceFile.statements) {
    if (
      !ts.isImportDeclaration(statement) ||
      !ts.isStringLiteral(statement.moduleSpecifier) ||
      statement.moduleSpecifier.text !== "vitest" ||
      statement.importClause?.namedBindings === undefined ||
      !ts.isNamedImports(statement.importClause.namedBindings)
    ) {
      continue
    }
    for (const element of statement.importClause.namedBindings.elements) {
      const importedName = (element.propertyName ?? element.name).text
      if (TEST_CALL_NAMES.has(importedName)) names.add(element.name.text)
    }
  }
  return names
}

/** 从 test.only、test.each(...) 等调用链提取根调用名。 */
function callRootName(expression) {
  if (ts.isIdentifier(expression)) return expression.text
  if (ts.isPropertyAccessExpression(expression)) return callRootName(expression.expression)
  if (ts.isCallExpression(expression)) return callRootName(expression.expression)
  return undefined
}

/** 将同一锚点的重复发现合并为一项要求。 */
function addRequirement(requirements, sourceFile, anchor, kind, name) {
  const start = anchor.getStart(sourceFile)
  const key = `${start}:${anchor.end}:${kind}:${name}`
  if (!requirements.has(key)) requirements.set(key, { anchor, kind, name })
}

/** 收集规则明确覆盖的声明；不收集字段、局部变量或普通匿名回调。 */
function collectRequirements(sourceFile) {
  const requirements = new Map()
  const exportedNames = locallyExportedNames(sourceFile)
  const testNames = importedVitestTestNames(sourceFile)
  if (isTestSourceFile(sourceFile.fileName)) {
    for (const name of TEST_CALL_NAMES) testNames.add(name)
  }

  /** 递归识别当前节点的稳定声明职责。 */
  function visit(node) {
    if (ts.isInterfaceDeclaration(node)) {
      addRequirement(requirements, sourceFile, node, "interface", declarationName(node))
    } else if (ts.isTypeAliasDeclaration(node)) {
      addRequirement(requirements, sourceFile, node, "type", declarationName(node))
    } else if (ts.isEnumDeclaration(node)) {
      addRequirement(requirements, sourceFile, node, "enum", declarationName(node))
    } else if (ts.isClassDeclaration(node)) {
      addRequirement(requirements, sourceFile, node, "class", declarationName(node))
    } else if (ts.isFunctionDeclaration(node)) {
      addRequirement(requirements, sourceFile, node, "function", declarationName(node))
    } else if (ts.isConstructorDeclaration(node)) {
      addRequirement(requirements, sourceFile, node, "constructor", "constructor")
    } else if (
      ts.isMethodDeclaration(node) ||
      ts.isGetAccessorDeclaration(node) ||
      ts.isSetAccessorDeclaration(node)
    ) {
      addRequirement(requirements, sourceFile, node, "method", declarationName(node))
    }

    if (ts.isVariableStatement(node)) {
      for (const declaration of node.declarationList.declarations) {
        if (!ts.isIdentifier(declaration.name) || declaration.initializer === undefined) continue
        const initializer = declaration.initializer
        if (
          (isExported(node) ||
            (node.parent === sourceFile && exportedNames.has(declaration.name.text))) &&
          (ts.isArrowFunction(initializer) || ts.isFunctionExpression(initializer)) &&
          isComponentOrHookName(declaration.name.text)
        ) {
          addRequirement(
            requirements,
            sourceFile,
            node,
            "exported-component-or-hook",
            declaration.name.text
          )
        }
      }
    }

    if (
      ts.isExpressionStatement(node) &&
      ts.isCallExpression(node.expression) &&
      testNames.has(callRootName(node.expression.expression))
    ) {
      const scenario = node.expression.arguments[0]
      const name = scenario !== undefined ? scenario.getText(sourceFile).slice(0, 80) : "<scenario>"
      addRequirement(requirements, sourceFile, node, "test", name)
    }

    ts.forEachChild(node, visit)
  }

  visit(sourceFile)
  return [...requirements.values()]
}

/** 提取声明前最后一条真正紧邻的源码注释。 */
function adjacentComment(source, anchor, sourceFile) {
  const start = anchor.getStart(sourceFile)
  const triviaStart = anchor.getFullStart()
  const trivia = source.slice(triviaStart, start)
  const comments = [...trivia.matchAll(/\/\/[^\r\n]*|\/\*[\s\S]*?\*\//g)]
  const last = comments.at(-1)
  if (last?.index === undefined) return undefined
  const comment = last[0]
  const commentStart = triviaStart + last.index
  const commentEnd = commentStart + comment.length
  const trailingTrivia = source.slice(commentEnd, start)
  const lineBreaks = trailingTrivia.match(/\r?\n/g)?.length ?? 0
  if (lineBreaks > 1 || trailingTrivia.trim() !== "") return undefined

  if (comment.startsWith("//") || (comment.startsWith("/*") && lineBreaks > 0)) {
    const lineStart = source.lastIndexOf("\n", commentStart - 1) + 1
    if (source.slice(lineStart, commentStart).trim() !== "") return undefined
  }
  return comment
}

/** 把语法诊断转换为稳定的一基位置。 */
function syntaxErrors(sourceFile) {
  return (sourceFile.parseDiagnostics ?? []).map((diagnostic) => {
    const start = diagnostic.start ?? 0
    const location = sourceFile.getLineAndCharacterOfPosition(start)
    return {
      file: sourceFile.fileName,
      line: location.line + 1,
      column: location.character + 1,
      message: ts.flattenDiagnosticMessageText(diagnostic.messageText, " ")
    }
  })
}

/** 检查一段 TypeScript/TSX 源码并同时返回声明数和语法错误。 */
function inspectChineseComments(source, filePath) {
  const sourceFile = ts.createSourceFile(
    filePath,
    source,
    ts.ScriptTarget.Latest,
    true,
    scriptKind(filePath)
  )
  const errors = syntaxErrors(sourceFile)
  if (errors.length > 0) return { checkedDeclarations: 0, errors, violations: [] }

  const requirements = collectRequirements(sourceFile)
  const violations = requirements
    .filter((requirement) => {
      const comment = adjacentComment(source, requirement.anchor, sourceFile)
      return comment === undefined || !HAN_CHARACTER.test(comment)
    })
    .map((requirement) => {
      const location = sourceFile.getLineAndCharacterOfPosition(
        requirement.anchor.getStart(sourceFile)
      )
      return {
        file: filePath,
        line: location.line + 1,
        column: location.character + 1,
        kind: requirement.kind,
        name: requirement.name,
        message: "声明必须具有紧邻且包含汉字的业务注释"
      }
    })
  return { checkedDeclarations: requirements.length, errors: [], violations }
}

/** 以严格 UTF-8 读取源码，并拒绝 NUL 字节。 */
function readUtf8Source(filePath) {
  const bytes = fs.readFileSync(filePath)
  if (bytes.includes(0)) throw new Error("源码包含 NUL 字节")
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes)
}

/** 枚举扫描根中的人工维护 TypeScript/TSX 文件，并拒绝源码符号链接。 */
function governedTypeScriptFiles(root) {
  const files = []
  const errors = []

  /** 深度优先遍历目录，同时保持诊断顺序稳定。 */
  function visit(directory, relativeDirectory) {
    const entries = fs.readdirSync(directory, { withFileTypes: true }).sort((left, right) =>
      left.name.localeCompare(right.name)
    )
    for (const entry of entries) {
      const relative = relativeDirectory === "" ? entry.name : path.join(relativeDirectory, entry.name)
      const normalized = normalizeRelativePath(relative)
      if (isChineseCommentPolicyExcluded(normalized, entry.isDirectory())) continue
      const absolute = path.join(directory, entry.name)
      if (entry.isSymbolicLink()) {
        errors.push(`${normalized}: 人工源码路径不得是符号链接`)
      } else if (entry.isDirectory()) {
        visit(absolute, relative)
      } else if (
        entry.isFile() &&
        (normalized.endsWith(".ts") || normalized.endsWith(".tsx"))
      ) {
        files.push({ absolute, relative: normalized })
      }
    }
  }

  visit(root, "")
  return { errors, files }
}

/** 检查完整 GUI 前端根目录，并聚合稳定报告。 */
function inspectProject(rootInput) {
  const report = {
    ok: false,
    root: rootInput,
    checkedFiles: 0,
    checkedDeclarations: 0,
    errors: [],
    violations: []
  }
  let root
  try {
    root = path.resolve(rootInput)
    const rootStatus = fs.lstatSync(root)
    if (rootStatus.isSymbolicLink()) throw new Error("扫描根不得是符号链接")
    if (!rootStatus.isDirectory()) throw new Error("扫描根不是目录")
    report.root = root
  } catch (error) {
    report.errors.push(`无法解析扫描根: ${error.message}`)
    return report
  }

  let discovered
  try {
    discovered = governedTypeScriptFiles(root)
  } catch (error) {
    report.errors.push(`无法枚举 TypeScript 源码: ${error.message}`)
    return report
  }
  report.errors.push(...discovered.errors)
  if (discovered.files.length === 0) report.errors.push("未找到可检查的 TypeScript/TSX 源码")

  for (const file of discovered.files) {
    try {
      const inspected = inspectChineseComments(readUtf8Source(file.absolute), file.relative)
      report.checkedFiles += 1
      report.checkedDeclarations += inspected.checkedDeclarations
      report.errors.push(
        ...inspected.errors.map(
          (error) => `${error.file}:${error.line}:${error.column}: TypeScript 语法错误: ${error.message}`
        )
      )
      report.violations.push(...inspected.violations)
    } catch (error) {
      report.errors.push(`${file.relative}: 无法安全读取源码: ${error.message}`)
    }
  }
  if (report.checkedDeclarations === 0 && report.errors.length === 0) {
    report.errors.push("未发现受中文注释门禁管理的声明")
  }
  report.ok = report.errors.length === 0 && report.violations.length === 0
  return report
}

/** 解析最小命令行参数，未知参数直接失败。 */
function parseArguments(arguments_) {
  let root = "."
  let json = false
  for (let index = 0; index < arguments_.length; index += 1) {
    const argument = arguments_[index]
    if (argument === "--json") json = true
    else if (argument === "--root" && arguments_[index + 1] !== undefined) {
      root = arguments_[index + 1]
      index += 1
    } else throw new Error(`未知或不完整参数: ${argument}`)
  }
  return { json, root }
}

/** 运行门禁并用 0/1/2 区分通过、声明违规和运行错误。 */
function main(arguments_ = process.argv.slice(2)) {
  let options
  try {
    options = parseArguments(arguments_)
  } catch (error) {
    process.stderr.write(`${error.message}\n`)
    return 2
  }
  const report = inspectProject(options.root)
  if (options.json) process.stdout.write(`${JSON.stringify(report)}\n`)
  else {
    for (const error of report.errors) process.stderr.write(`ERROR: ${error}\n`)
    for (const violation of report.violations) {
      process.stderr.write(
        `ERROR: ${violation.file}:${violation.line}:${violation.column}: ${violation.kind} ${violation.name}: ${violation.message}\n`
      )
    }
  }
  if (report.errors.length > 0) return 2
  if (report.violations.length > 0) return 1
  return 0
}

module.exports = {
  inspectChineseComments,
  inspectProject,
  isChineseCommentPolicyExcluded,
  main
}

if (require.main === module) process.exitCode = main()
