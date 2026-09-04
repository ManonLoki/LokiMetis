#!/usr/bin/env python3
"""按 Rust、前端与通用文本配置报告重构候选并拒绝硬超限文件。"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Any

RUST_REVIEW_THRESHOLD = 400
RUST_HARD_LINE_LIMIT = 800
FRONTEND_REVIEW_THRESHOLD = 500
FRONTEND_HARD_LINE_LIMIT = 1000
DEFAULT_REVIEW_THRESHOLD = 500
DEFAULT_HARD_LINE_LIMIT = 2000

LINE_LIMIT_PROFILES: dict[str, dict[str, int | str]] = {
    "rust": {
        "label": "Rust 代码",
        "reviewThreshold": RUST_REVIEW_THRESHOLD,
        "hardLimit": RUST_HARD_LINE_LIMIT,
    },
    "frontend": {
        "label": "前端代码",
        "reviewThreshold": FRONTEND_REVIEW_THRESHOLD,
        "hardLimit": FRONTEND_HARD_LINE_LIMIT,
    },
    "maintained_text": {
        "label": "其他人工维护文本",
        "reviewThreshold": DEFAULT_REVIEW_THRESHOLD,
        "hardLimit": DEFAULT_HARD_LINE_LIMIT,
    },
}

FRONTEND_CODE_SUFFIXES = frozenset(
    {
        ".astro",
        ".cjs",
        ".css",
        ".cts",
        ".html",
        ".js",
        ".jsx",
        ".less",
        ".mjs",
        ".mts",
        ".sass",
        ".scss",
        ".svelte",
        ".ts",
        ".tsx",
        ".vue",
    }
)
GENERATED_LOCKFILE_NAMES = frozenset(
    {
        "Cargo.lock",
        "package-lock.json",
        "pnpm-lock.yaml",
        "poetry.lock",
        "uv.lock",
        "yarn.lock",
    }
)


def _canonical_root(root: Path) -> tuple[Path | None, list[str]]:
    """解析检查根目录，并拒绝缺失目录和符号链接根。"""

    if root.is_symlink():
        return None, [f"项目根不得是符号链接: {root}"]
    try:
        resolved = root.resolve(strict=True)
    except OSError as error:
        return None, [f"无法解析项目根 {root}: {error}"]
    if not resolved.is_dir():
        return None, [f"项目根不是目录: {resolved}"]
    return resolved, []


def _git_visible_paths(root: Path) -> tuple[list[str], list[str]]:
    """用 NUL 分隔 Git 清单枚举已跟踪和未忽略的未跟踪路径。"""

    command = (
        "git",
        "-C",
        str(root),
        "ls-files",
        "--cached",
        "--others",
        "--exclude-standard",
        "-z",
    )
    try:
        result = subprocess.run(command, capture_output=True, check=False)
    except OSError as error:
        return [], [f"无法执行 Git 文件枚举: {error}"]
    if result.returncode != 0:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        return [], [f"Git 文件枚举失败({result.returncode}): {detail or '无诊断'}"]
    try:
        paths = [item.decode("utf-8") for item in result.stdout.split(b"\0") if item]
    except UnicodeDecodeError as error:
        return [], [f"Git 路径不是 UTF-8，无法安全检查: {error}"]
    if len(paths) != len(set(paths)):
        return [], ["Git 文件枚举包含重复路径"]
    return sorted(paths), []


def _safe_candidate(
    root: Path, relative: str
) -> tuple[Path | None, str | None, str | None]:
    """把 Git 相对路径约束在项目根内，并且不跟随符号链接。"""

    if not relative or os.path.isabs(relative):
        return None, f"Git 返回非法相对路径: {relative!r}", None
    candidate = root / relative
    try:
        lexical = Path(os.path.abspath(candidate))
        lexical.relative_to(root)
    except (OSError, ValueError):
        return None, f"Git 路径越过项目根: {relative!r}", None
    if candidate.is_symlink():
        return None, None, "symlink"
    if not candidate.exists():
        return None, None, "missing"
    try:
        if not candidate.is_file():
            return None, f"Git 路径不是普通文件: {relative!r}", None
    except OSError as error:
        return None, f"无法检查 Git 路径 {relative!r}: {error}", None
    return candidate, None, None


def _read_text(candidate: Path, relative: str) -> tuple[str | None, str | None]:
    """读取 UTF-8 文本；含 NUL 的普通文件按二进制跳过。"""

    try:
        payload = candidate.read_bytes()
    except OSError as error:
        return None, f"无法读取 {relative!r}: {error}"
    if b"\0" in payload:
        return None, None
    try:
        return payload.decode("utf-8"), None
    except UnicodeDecodeError as error:
        return None, f"无 NUL 的 Git 文件不是 UTF-8，无法分类 {relative!r}: {error}"


def _line_limit_profile(relative: str) -> tuple[str, int, int]:
    """按源码后缀选择确定性的行数配置，Rust 优先于前端与通用文本。"""

    suffix = Path(relative).suffix.lower()
    if suffix == ".rs":
        profile_name = "rust"
    elif suffix in FRONTEND_CODE_SUFFIXES:
        profile_name = "frontend"
    else:
        profile_name = "maintained_text"
    profile = LINE_LIMIT_PROFILES[profile_name]
    return (
        profile_name,
        int(profile["reviewThreshold"]),
        int(profile["hardLimit"]),
    )


def inspect_repository(root: Path) -> dict[str, Any]:
    """返回稳定的行数检查报告，不修改仓库。"""

    canonical, errors = _canonical_root(root)
    report: dict[str, Any] = {
        "ok": False,
        "root": str(root),
        "lineLimitProfiles": LINE_LIMIT_PROFILES,
        "checkedTextFiles": 0,
        "excludedGeneratedFiles": [],
        "skippedBinaryFiles": 0,
        "skippedMissingFiles": 0,
        "skippedSymlinks": 0,
        "reviewCandidates": [],
        "violations": [],
        "errors": errors,
    }
    if canonical is None:
        return report
    report["root"] = str(canonical)
    relative_paths, git_errors = _git_visible_paths(canonical)
    report["errors"].extend(git_errors)
    if git_errors:
        return report

    for relative in relative_paths:
        candidate, path_error, skip_reason = _safe_candidate(canonical, relative)
        if path_error:
            report["errors"].append(path_error)
            continue
        if candidate is None:
            if skip_reason == "symlink":
                report["skippedSymlinks"] += 1
            elif skip_reason == "missing":
                report["skippedMissingFiles"] += 1
            continue
        if candidate.name in GENERATED_LOCKFILE_NAMES:
            report["excludedGeneratedFiles"].append(relative)
            continue
        text, read_error = _read_text(candidate, relative)
        if read_error:
            report["errors"].append(read_error)
            continue
        if text is None:
            report["skippedBinaryFiles"] += 1
            continue
        report["checkedTextFiles"] += 1
        line_count = len(text.splitlines())
        profile, review_threshold, hard_limit = _line_limit_profile(relative)
        if line_count > hard_limit:
            report["violations"].append(
                {
                    "path": relative,
                    "lines": line_count,
                    "profile": profile,
                    "threshold": review_threshold,
                    "limit": hard_limit,
                }
            )
        elif line_count > review_threshold:
            report["reviewCandidates"].append(
                {
                    "path": relative,
                    "lines": line_count,
                    "profile": profile,
                    "threshold": review_threshold,
                    "limit": hard_limit,
                }
            )

    report["excludedGeneratedFiles"].sort()
    report["reviewCandidates"].sort(key=lambda item: str(item["path"]))
    report["violations"].sort(key=lambda item: str(item["path"]))
    report["errors"].sort()
    report["ok"] = not report["errors"] and not report["violations"]
    return report


def _parser() -> argparse.ArgumentParser:
    """建立稳定命令行参数，不要求 shell 包装。"""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--json", action="store_true")
    return parser


def main(arguments: list[str] | None = None) -> int:
    """执行检查；0 为通过或待语义复核，1 为硬超限，2 为检查器错误。"""

    options = _parser().parse_args(arguments)
    report = inspect_repository(options.root)
    if options.json:
        print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    else:
        for error in report["errors"]:
            print(f"ERROR: {error}", file=sys.stderr)
        for candidate in report["reviewCandidates"]:
            profile = LINE_LIMIT_PROFILES[str(candidate["profile"])]
            print(
                f"REVIEW: {profile['label']}超过 {candidate['threshold']} 行，"
                "建议按职责重构并复核高内聚、职责单一和职责相近性: "
                f"{candidate['path']} ({candidate['lines']} lines)",
                file=sys.stderr,
            )
        for violation in report["violations"]:
            profile = LINE_LIMIT_PROFILES[str(violation["profile"])]
            rust_hint = (
                "；拆分 Rust 模块时必须使用 <module>/mod.rs 目录结构"
                if violation["profile"] == "rust"
                else ""
            )
            print(
                f"ERROR: {profile['label']}超过 {violation['limit']} 行，必须按职责拆分"
                f"{rust_hint}: {violation['path']} ({violation['lines']} lines)",
                file=sys.stderr,
            )
        if report["ok"]:
            print(
                "File line-limit check passed: "
                f"{report['checkedTextFiles']} maintained text file(s), "
                "profiles=rust(400/800), frontend(500/1000), "
                "maintained-text(500/2000)."
            )
    if report["errors"]:
        return 2
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
