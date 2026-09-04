#!/usr/bin/env python3
"""检查 Cargo package/workspace 中受管 Rust 声明是否有紧邻中文文档注释。"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import fnmatch
import json
import os
from pathlib import Path
import stat
import sys
import tomllib
from typing import Any


DECLARATION_KINDS = frozenset({"enum", "fn", "struct", "trait", "type", "union"})
FUNCTION_MODIFIERS = frozenset({"async", "const", "default", "unsafe"})
SOURCE_DIRECTORIES = ("src", "tests")
DELIMITER_PAIRS = {"(": ")", "[": "]", "{": "}"}


@dataclass(frozen=True)
class Token:
    """保存注释归属与声明识别所需的最小 Rust 词法信息。"""

    kind: str
    text: str
    line: int
    column: int


def _contains_han(text: str) -> bool:
    """识别常用及扩展 CJK 统一表意文字。"""

    return any(
        "\u3400" <= character <= "\u4dbf"
        or "\u4e00" <= character <= "\u9fff"
        or "\uf900" <= character <= "\ufaff"
        or "\U00020000" <= character <= "\U0003134f"
        for character in text
    )


def _quoted_end(source: str, quote_start: int) -> int | None:
    """返回普通字符串字面量结束偏移，未闭合时返回空值。"""

    cursor = quote_start + 1
    escaped = False
    while cursor < len(source):
        character = source[cursor]
        if escaped:
            escaped = False
        elif character == "\\":
            escaped = True
        elif character == '"':
            return cursor + 1
        cursor += 1
    return None


def _char_end(source: str, quote_start: int) -> int | None:
    """只识别真正字符字面量，避免把 Rust lifetime 当作字符吞掉。"""

    cursor = quote_start + 1
    if cursor >= len(source) or source[cursor] in "\r\n'":
        return None
    if source[cursor] == "\\":
        cursor += 1
        if cursor >= len(source) or source[cursor] in "\r\n":
            return None
        if source[cursor] == "u" and cursor + 1 < len(source) and source[cursor + 1] == "{":
            closing = source.find("}", cursor + 2)
            if closing < 0:
                return None
            cursor = closing + 1
        else:
            cursor += 1
    else:
        cursor += 1
    return cursor + 1 if cursor < len(source) and source[cursor] == "'" else None


def _raw_string_end(source: str, start: int) -> int | None:
    """识别 Rust raw、byte raw 与 C raw 字符串并返回结束偏移。"""

    cursor = start
    if source.startswith(("br", "cr"), cursor):
        cursor += 2
    elif source.startswith("r", cursor):
        cursor += 1
    else:
        return None
    hashes_start = cursor
    while cursor < len(source) and source[cursor] == "#":
        cursor += 1
    if cursor >= len(source) or source[cursor] != '"':
        return None
    hashes = source[hashes_start:cursor]
    terminator = '"' + hashes
    end = source.find(terminator, cursor + 1)
    return None if end < 0 else end + len(terminator)


def _advance_position(fragment: str, line: int, column: int) -> tuple[int, int]:
    """按已消费源码片段更新一基行列位置。"""

    newline_count = fragment.count("\n")
    if newline_count:
        return line + newline_count, len(fragment.rsplit("\n", 1)[1]) + 1
    return line, column + len(fragment)


def _lex(source: str) -> tuple[list[Token], list[str]]:
    """过滤普通注释和字面量，只保留声明与文档关联所需 token。"""

    tokens: list[Token] = []
    errors: list[str] = []
    cursor = 0
    line = 1
    column = 1
    while cursor < len(source):
        start = cursor
        start_line = line
        start_column = column
        if source[cursor].isspace():
            cursor += 1
        elif source.startswith("//", cursor):
            end = source.find("\n", cursor)
            cursor = len(source) if end < 0 else end
            payload = source[start:cursor]
            if payload.startswith("///") and not payload.startswith("////"):
                tokens.append(Token("doc_outer", payload, start_line, start_column))
            elif payload.startswith("//!"):
                tokens.append(Token("doc_inner", payload, start_line, start_column))
        elif source.startswith("/*", cursor):
            depth = 1
            cursor += 2
            while cursor < len(source) and depth:
                if source.startswith("/*", cursor):
                    depth += 1
                    cursor += 2
                elif source.startswith("*/", cursor):
                    depth -= 1
                    cursor += 2
                else:
                    cursor += 1
            if depth:
                errors.append(f"第 {start_line} 行存在未闭合块注释")
                cursor = len(source)
            payload = source[start:cursor]
            if payload.startswith("/**") and not payload.startswith("/***"):
                tokens.append(Token("doc_outer", payload, start_line, start_column))
            elif payload.startswith("/*!"):
                tokens.append(Token("doc_inner", payload, start_line, start_column))
        else:
            raw_end = _raw_string_end(source, cursor)
            if raw_end is not None:
                cursor = raw_end
                tokens.append(Token("string", source[start:cursor], start_line, start_column))
            elif source[cursor] == '"' or (
                source[cursor] in {"b", "c"}
                and cursor + 1 < len(source)
                and source[cursor + 1] == '"'
            ):
                quote_start = cursor if source[cursor] == '"' else cursor + 1
                end = _quoted_end(source, quote_start)
                if end is None:
                    errors.append(f"第 {start_line} 行存在未闭合字符串字面量")
                    cursor = len(source)
                else:
                    cursor = end
                tokens.append(Token("string", source[start:cursor], start_line, start_column))
            elif source[cursor] == "'" or (
                source[cursor] == "b"
                and cursor + 1 < len(source)
                and source[cursor + 1] == "'"
            ):
                quote_start = cursor if source[cursor] == "'" else cursor + 1
                end = _char_end(source, quote_start)
                if end is None:
                    cursor += 1
                    tokens.append(Token("punct", source[start:cursor], start_line, start_column))
                else:
                    cursor = end
                    tokens.append(Token("char", source[start:cursor], start_line, start_column))
            elif source[cursor] == "_" or source[cursor].isalpha():
                cursor += 1
                while cursor < len(source) and (
                    source[cursor] == "_" or source[cursor].isalnum()
                ):
                    cursor += 1
                tokens.append(Token("word", source[start:cursor], start_line, start_column))
            else:
                cursor += 1
                tokens.append(Token("punct", source[start:cursor], start_line, start_column))
        line, column = _advance_position(source[start:cursor], line, column)
    return tokens, errors


def _delimiter_errors(tokens: list[Token]) -> list[str]:
    """报告括号、方括号和花括号不平衡，避免残缺源码空通过。"""

    stack: list[Token] = []
    closing_to_opening = {closing: opening for opening, closing in DELIMITER_PAIRS.items()}
    errors: list[str] = []
    for token in tokens:
        if token.text in DELIMITER_PAIRS:
            stack.append(token)
        elif token.text in closing_to_opening:
            if not stack or stack[-1].text != closing_to_opening[token.text]:
                errors.append(f"第 {token.line} 行存在不匹配的 `{token.text}`")
            else:
                stack.pop()
    errors.extend(f"第 {token.line} 行存在未闭合的 `{token.text}`" for token in stack)
    return errors


def _matching_open(tokens: list[Token], closing_index: int, opening: str, closing: str) -> int | None:
    """从右向左匹配括号，供可见性与 attribute 前缀识别。"""

    depth = 0
    for index in range(closing_index, -1, -1):
        if tokens[index].text == closing:
            depth += 1
        elif tokens[index].text == opening:
            depth -= 1
            if depth == 0:
                return index
    return None


def _matching_close(tokens: list[Token], opening_index: int) -> int | None:
    """从左向右找到宏调用分隔组的结束 token。"""

    opening = tokens[opening_index].text
    closing = DELIMITER_PAIRS.get(opening)
    if closing is None:
        return None
    depth = 0
    for index in range(opening_index, len(tokens)):
        if tokens[index].text == opening:
            depth += 1
        elif tokens[index].text == closing:
            depth -= 1
            if depth == 0:
                return index
    return None


def _macro_body_indexes(tokens: list[Token]) -> set[int]:
    """标记宏定义或调用 token 组，宏展开声明由宏定义处语义复核。"""

    skipped: set[int] = set()
    for index, token in enumerate(tokens):
        if token.text != "!" or index == 0 or tokens[index - 1].kind != "word":
            continue
        opening_index = index + 1
        if (
            tokens[index - 1].text == "macro_rules"
            and opening_index < len(tokens)
            and tokens[opening_index].kind == "word"
        ):
            opening_index += 1
        if opening_index >= len(tokens) or tokens[opening_index].text not in DELIMITER_PAIRS:
            continue
        closing_index = _matching_close(tokens, opening_index)
        if closing_index is not None:
            skipped.update(range(opening_index + 1, closing_index))
    return skipped


def _declaration_prefix_start(tokens: list[Token], keyword_index: int) -> int:
    """找到声明修饰符起点，使其前方 attribute/doc 可正确关联。"""

    cursor = keyword_index - 1
    while cursor >= 0:
        token = tokens[cursor]
        if token.kind == "word" and token.text in FUNCTION_MODIFIERS | {"pub"}:
            cursor -= 1
            continue
        if token.kind == "word" and token.text == "extern":
            cursor -= 1
            continue
        if (
            token.kind == "string"
            and cursor > 0
            and tokens[cursor - 1].kind == "word"
            and tokens[cursor - 1].text == "extern"
        ):
            cursor -= 2
            continue
        if token.text == ")":
            opening = _matching_open(tokens, cursor, "(", ")")
            if (
                opening is not None
                and opening > 0
                and tokens[opening - 1].kind == "word"
                and tokens[opening - 1].text == "pub"
            ):
                cursor = opening - 2
                continue
        break
    return cursor + 1


def _attribute_start(tokens: list[Token], closing_index: int) -> int | None:
    """若当前位置结束一个 outer attribute，返回其 `#` token 下标。"""

    if tokens[closing_index].text != "]":
        return None
    opening = _matching_open(tokens, closing_index, "[", "]")
    if opening is None or opening == 0 or tokens[opening - 1].text != "#":
        return None
    if opening >= 2 and tokens[opening - 2].text == "!":
        return None
    return opening - 1


def _split_top_level_arguments(tokens: list[Token]) -> list[list[Token]]:
    """按顶层逗号拆分 attribute 参数，同时保留嵌套 token 树。"""

    arguments: list[list[Token]] = []
    current: list[Token] = []
    stack: list[str] = []
    for token in tokens:
        if token.text in DELIMITER_PAIRS:
            stack.append(token.text)
        elif stack and token.text == DELIMITER_PAIRS[stack[-1]]:
            stack.pop()
        if token.text == "," and not stack:
            arguments.append(current)
            current = []
        else:
            current.append(token)
    arguments.append(current)
    return arguments


def _attribute_meta_has_chinese_doc(tokens: list[Token]) -> bool:
    """识别直接或嵌套 cfg_attr 中可生效的中文 doc 元数据。"""

    direct_doc = (
        len(tokens) == 3
        and tokens[0].kind == "word"
        and tokens[0].text == "doc"
        and tokens[1].text == "="
        and tokens[2].kind == "string"
        and _contains_han(tokens[2].text)
    )
    if direct_doc:
        return True
    if (
        len(tokens) < 5
        or tokens[0].kind != "word"
        or tokens[0].text != "cfg_attr"
        or tokens[1].text != "("
        or tokens[-1].text != ")"
    ):
        return False
    closing = _matching_close(tokens, 1)
    if closing != len(tokens) - 1:
        return False
    arguments = _split_top_level_arguments(tokens[2:-1])
    return len(arguments) >= 2 and any(
        _attribute_meta_has_chinese_doc(argument) for argument in arguments[1:]
    )


def _has_chinese_item_doc(tokens: list[Token], prefix_start: int) -> bool:
    """检查声明前连续 outer doc/attribute 集合是否含中文文档。"""

    cursor = prefix_start - 1
    while cursor >= 0:
        token = tokens[cursor]
        if token.kind == "doc_outer":
            if _contains_han(token.text):
                return True
            cursor -= 1
            continue
        attribute_start = _attribute_start(tokens, cursor)
        if attribute_start is not None:
            attribute = tokens[attribute_start : cursor + 1]
            if _attribute_meta_has_chinese_doc(attribute[2:-1]):
                return True
            cursor = attribute_start - 1
            continue
        break
    return False


def _declaration_name(tokens: list[Token], keyword_index: int) -> str | None:
    """提取具名声明；函数指针和宏变量不会被误判。"""

    next_index = keyword_index + 1
    if next_index >= len(tokens) or tokens[next_index].kind != "word":
        return None
    if (
        tokens[next_index].text == "r"
        and next_index + 2 < len(tokens)
        and tokens[next_index + 1].text == "#"
        and tokens[next_index + 2].kind == "word"
    ):
        return f"r#{tokens[next_index + 2].text}"
    return tokens[next_index].text


def _inspect_source(relative: str, source: str) -> tuple[int, list[dict[str, Any]], list[str]]:
    """检查一个 Rust 文件并返回声明数、违规和词法错误。"""

    tokens, lexical_errors = _lex(source)
    lexical_errors.extend(_delimiter_errors(tokens))
    skipped = _macro_body_indexes(tokens)
    violations: list[dict[str, Any]] = []
    declarations = 0
    for index, token in enumerate(tokens):
        if index in skipped or token.kind != "word" or token.text not in DECLARATION_KINDS:
            continue
        name = _declaration_name(tokens, index)
        if name is None:
            continue
        declarations += 1
        prefix_start = _declaration_prefix_start(tokens, index)
        if not _has_chinese_item_doc(tokens, prefix_start):
            violations.append(
                {
                    "path": relative,
                    "line": token.line,
                    "column": token.column,
                    "kind": token.text,
                    "name": name,
                    "reason": "missing_chinese_outer_doc",
                }
            )
    return declarations, violations, lexical_errors


def _canonical_root(root: Path) -> tuple[Path | None, list[str]]:
    """解析项目根并拒绝符号链接、缺失路径或普通文件。"""

    if root.is_symlink():
        return None, [f"项目根不得是符号链接: {root}"]
    try:
        resolved = root.resolve(strict=True)
    except OSError as error:
        return None, [f"无法解析项目根 {root}: {error}"]
    if not resolved.is_dir():
        return None, [f"项目根不是目录: {resolved}"]
    return resolved, []


def _read_manifest(path: Path, workspace_root: Path) -> tuple[dict[str, Any] | None, list[str]]:
    """读取一个根内普通 UTF-8 Cargo manifest，并把故障转成稳定错误。"""

    relative = path.relative_to(workspace_root).as_posix()
    if path.is_symlink():
        return None, [f"Cargo manifest 不得是符号链接: {relative}"]
    try:
        payload = path.read_bytes()
    except OSError as error:
        return None, [f"无法读取 Cargo manifest {relative}: {error}"]
    if b"\0" in payload:
        return None, [f"Cargo manifest 含 NUL 字节: {relative}"]
    try:
        return tomllib.loads(payload.decode("utf-8")), []
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        return None, [f"Cargo manifest 无法解析: {relative}: {error}"]


def _package_roots(root: Path) -> tuple[list[Path], list[str]]:
    """从 package 或 virtual workspace 根解析全部受管 package。"""

    manifest_path = root / "Cargo.toml"
    if not manifest_path.exists():
        return [], ["项目根缺少 Cargo.toml"]
    manifest, errors = _read_manifest(manifest_path, root)
    if manifest is None:
        return [], errors
    roots: list[Path] = [root] if isinstance(manifest.get("package"), dict) else []
    workspace = manifest.get("workspace")
    if isinstance(workspace, dict):
        members = workspace.get("members", [])
        excludes = workspace.get("exclude", [])
        if not isinstance(members, list) or not all(isinstance(item, str) for item in members):
            errors.append("Cargo workspace.members 必须是字符串列表")
            return [], errors
        if not isinstance(excludes, list) or not all(isinstance(item, str) for item in excludes):
            errors.append("Cargo workspace.exclude 必须是字符串列表")
            return [], errors
        for pattern in members:
            member_pattern = Path(pattern)
            if member_pattern.is_absolute() or ".." in member_pattern.parts:
                errors.append(f"Cargo workspace member 不得越界: {pattern}")
                continue
            matches = sorted(root.glob(pattern))
            if not matches:
                errors.append(f"Cargo workspace member 未匹配任何路径: {pattern}")
            for candidate in matches:
                relative = candidate.relative_to(root).as_posix()
                if any(fnmatch.fnmatch(relative, excluded) for excluded in excludes):
                    continue
                if candidate.is_symlink():
                    errors.append(f"Cargo workspace member 不得是符号链接: {relative}")
                    continue
                try:
                    resolved = candidate.resolve(strict=True)
                    resolved.relative_to(root)
                except (OSError, ValueError) as error:
                    errors.append(f"Cargo workspace member 无法安全解析: {relative}: {error}")
                    continue
                if not resolved.is_dir() or not (resolved / "Cargo.toml").is_file():
                    errors.append(f"Cargo workspace member 缺少普通 Cargo.toml: {relative}")
                    continue
                member_manifest, member_errors = _read_manifest(resolved / "Cargo.toml", root)
                errors.extend(member_errors)
                if member_manifest is not None and not isinstance(member_manifest.get("package"), dict):
                    errors.append(f"Cargo workspace member 缺少 [package]: {relative}")
                elif member_manifest is not None:
                    roots.append(resolved)
    if not roots and not errors:
        errors.append("项目根未解析出任何 Cargo package")
    return sorted(set(roots)), errors


def _rust_sources(package_root: Path, workspace_root: Path) -> tuple[list[Path], list[str]]:
    """枚举 package 的生产、测试和构建源码，并拒绝链接或异常文件。"""

    sources: list[Path] = []
    errors: list[str] = []
    build_script = package_root / "build.rs"
    if build_script.exists():
        if build_script.is_symlink() or not build_script.is_file():
            errors.append(
                f"Rust 构建脚本必须是普通文件: {build_script.relative_to(workspace_root).as_posix()}"
            )
        else:
            sources.append(build_script)
    for directory_name in SOURCE_DIRECTORIES:
        directory = package_root / directory_name
        if not directory.exists():
            continue
        relative_directory = directory.relative_to(workspace_root).as_posix()
        if directory.is_symlink():
            errors.append(f"Rust 源码目录不得是符号链接: {relative_directory}")
            continue
        if not directory.is_dir():
            errors.append(f"Rust 源码路径不是目录: {relative_directory}")
            continue

        def record_walk_error(error: OSError) -> None:
            """把目录枚举故障转成稳定失败，避免权限问题造成空通过。"""

            errors.append(f"无法枚举 Rust 源码目录 {relative_directory}: {error}")

        for current, directory_names, file_names in os.walk(
            directory, followlinks=False, onerror=record_walk_error
        ):
            current_path = Path(current)
            directory_names.sort()
            for name in list(directory_names):
                candidate = current_path / name
                relative = candidate.relative_to(workspace_root).as_posix()
                try:
                    mode = candidate.lstat().st_mode
                except OSError as error:
                    errors.append(f"无法检查 Rust 源码目录 {relative}: {error}")
                    directory_names.remove(name)
                    continue
                if stat.S_ISLNK(mode):
                    errors.append(f"Rust 源码目录不得是符号链接: {relative}")
                    directory_names.remove(name)
                elif not stat.S_ISDIR(mode):
                    errors.append(f"Rust 源码子路径不是目录: {relative}")
                    directory_names.remove(name)
            for name in sorted(file_names):
                if not name.endswith(".rs"):
                    continue
                candidate = current_path / name
                relative = candidate.relative_to(workspace_root).as_posix()
                try:
                    mode = candidate.lstat().st_mode
                except OSError as error:
                    errors.append(f"无法检查 Rust 源码 {relative}: {error}")
                    continue
                if stat.S_ISLNK(mode):
                    errors.append(f"Rust 源码不得是符号链接: {relative}")
                elif stat.S_ISREG(mode):
                    sources.append(candidate)
                else:
                    errors.append(f"Rust 源码不是普通文件: {relative}")
    if not sources and not errors:
        errors.append(
            f"Cargo package 未找到 Rust 源码: {package_root.relative_to(workspace_root).as_posix() or '.'}"
        )
    return sorted(set(sources)), errors


def inspect_project(root: Path) -> dict[str, Any]:
    """返回稳定 JSON 结构的只读中文注释检查报告。"""

    canonical, errors = _canonical_root(root)
    report: dict[str, Any] = {
        "ok": False,
        "root": str(root),
        "checkedPackages": 0,
        "checkedRustFiles": 0,
        "checkedDeclarations": 0,
        "violations": [],
        "errors": errors,
    }
    if canonical is None:
        return report
    report["root"] = str(canonical)
    packages, package_errors = _package_roots(canonical)
    report["errors"].extend(package_errors)
    report["checkedPackages"] = len(packages)
    for package_root in packages:
        sources, source_errors = _rust_sources(package_root, canonical)
        report["errors"].extend(source_errors)
        for source_path in sources:
            relative = source_path.relative_to(canonical).as_posix()
            try:
                payload = source_path.read_bytes()
            except OSError as error:
                report["errors"].append(f"无法读取 Rust 源码 {relative}: {error}")
                continue
            if b"\0" in payload:
                report["errors"].append(f"Rust 源码含 NUL 字节: {relative}")
                continue
            try:
                source = payload.decode("utf-8")
            except UnicodeDecodeError as error:
                report["errors"].append(f"Rust 源码不是 UTF-8: {relative}: {error}")
                continue
            report["checkedRustFiles"] += 1
            declarations, violations, lexical_errors = _inspect_source(relative, source)
            report["checkedDeclarations"] += declarations
            report["violations"].extend(violations)
            report["errors"].extend(f"{relative}: {error}" for error in lexical_errors)
    if report["checkedRustFiles"] and not report["checkedDeclarations"]:
        report["errors"].append("未发现受中文注释门禁管理的 Rust 声明")
    report["violations"].sort(
        key=lambda item: (str(item["path"]), int(item["line"]), int(item["column"]))
    )
    report["errors"].sort()
    report["ok"] = not report["errors"] and not report["violations"]
    return report


def _parser() -> argparse.ArgumentParser:
    """建立不依赖 shell 包装的稳定命令行参数。"""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--json", action="store_true")
    return parser


def main(arguments: list[str] | None = None) -> int:
    """执行门禁；0 为通过，1 为注释违规，2 为检查器运行错误。"""

    options = _parser().parse_args(arguments)
    report = inspect_project(options.root)
    if options.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    else:
        for error in report["errors"]:
            print(f"ERROR: {error}", file=sys.stderr)
        for violation in report["violations"]:
            print(
                "ERROR: Rust 声明缺少紧邻的中文文档注释: "
                f"{violation['path']}:{violation['line']} "
                f"{violation['kind']} {violation['name']}",
                file=sys.stderr,
            )
        if report["ok"]:
            print(
                "Rust Chinese comment check passed: "
                f"{report['checkedPackages']} package(s), "
                f"{report['checkedRustFiles']} file(s), "
                f"{report['checkedDeclarations']} declaration(s)."
            )
    if report["errors"]:
        return 2
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
