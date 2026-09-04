#!/usr/bin/env python3
"""以可预览、无覆盖的方式统一替换项目身份文本和路径。"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path


EXCLUDED_DIRECTORIES = {
    ".git",
    ".idea",
    ".next",
    ".turbo",
    ".vscode",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "release",
    "target",
    "vendor",
}
SNAKE_CASE = re.compile(r"^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$")
KEBAB_CASE = re.compile(r"^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$")


def parse_arguments() -> argparse.Namespace:
    """解析并校验完成一次身份替换所需的显式参数。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--old-display-name-zh", required=True)
    parser.add_argument("--new-display-name-zh", required=True)
    parser.add_argument("--old-display-name-en", required=True)
    parser.add_argument("--new-display-name-en", required=True)
    parser.add_argument("--old-id", required=True)
    parser.add_argument("--new-id", required=True)
    parser.add_argument("--old-kebab", required=True)
    parser.add_argument("--new-kebab", required=True)
    parser.add_argument(
        "--replace",
        action="append",
        default=[],
        metavar="OLD=NEW",
        help="增加一个区分大小写的精确替换，可重复使用",
    )
    parser.add_argument("--apply", action="store_true")
    parser.add_argument(
        "--rename-root",
        action="store_true",
        help="把目录基本名称等于旧标识的项目根目录改为新标识",
    )
    return parser.parse_args()


def build_replacements(args: argparse.Namespace) -> list[tuple[str, str]]:
    """建立去重且按旧文本长度降序排列的替换表，避免短前缀抢先匹配。"""
    pairs = [
        (args.old_display_name_zh, args.new_display_name_zh),
        (args.old_display_name_en, args.new_display_name_en),
        (args.old_id, args.new_id),
        (args.old_kebab, args.new_kebab),
    ]
    for raw in args.replace:
        if "=" not in raw:
            raise ValueError(f"--replace 值非法：{raw!r}")
        old, new = raw.split("=", 1)
        pairs.append((old, new))

    normalized: dict[str, str] = {}
    for old, new in pairs:
        if not old or not new:
            raise ValueError("替换值不得为空")
        if old == new:
            raise ValueError(f"旧值和新值必须不同：{old!r}")
        previous = normalized.get(old)
        if previous is not None and previous != new:
            raise ValueError(f"以下旧值存在冲突的替换映射：{old!r}")
        normalized[old] = new
    return sorted(normalized.items(), key=lambda pair: len(pair[0]), reverse=True)


def replace_exact(text: str, replacements: list[tuple[str, str]]) -> str:
    """按已排序映射替换文本，不推断大小写、词形或法律语义。"""
    for old, new in replacements:
        text = text.replace(old, new)
    return text


def inventory(root: Path) -> tuple[list[Path], list[Path], list[str]]:
    """遍历项目维护树，同时拒绝符号链接并记录确定性排除目录。"""
    files: list[Path] = []
    directories: list[Path] = []
    excluded: list[str] = []
    for current, directory_names, file_names in os.walk(root, topdown=True):
        current_path = Path(current)
        kept_directories: list[str] = []
        for name in sorted(directory_names):
            path = current_path / name
            relative = path.relative_to(root).as_posix()
            if path.is_symlink():
                raise ValueError(f"不允许符号链接：{relative}")
            if name in EXCLUDED_DIRECTORIES and (name != "release" or current_path == root):
                excluded.append(relative + "/")
            else:
                kept_directories.append(name)
                directories.append(path)
        directory_names[:] = kept_directories
        for name in sorted(file_names):
            path = current_path / name
            relative = path.relative_to(root).as_posix()
            if path.is_symlink():
                raise ValueError(f"不允许符号链接：{relative}")
            files.append(path)
    return files, directories, excluded


def read_text(path: Path) -> tuple[str | None, bool]:
    """只把无 NUL 且可按 UTF-8 解码的文件视为可维护文本。"""
    payload = path.read_bytes()
    if b"\x00" in payload:
        return None, True
    try:
        return payload.decode("utf-8"), False
    except UnicodeDecodeError:
        return None, True


def renamed_path(path: Path, root: Path, replacements: list[tuple[str, str]]) -> Path:
    """只替换当前路径项名称，父目录由独立的深度倒序改名负责。"""
    path.relative_to(root)
    destination = path.with_name(replace_exact(path.name, replacements))
    destination.relative_to(root)
    return destination


def plan_changes(
    root: Path, replacements: list[tuple[str, str]]
) -> tuple[list[tuple[Path, str]], list[tuple[Path, Path]], list[str], list[str]]:
    """生成内容修改、路径改名、排除目录和二进制跳过项，不写入磁盘。"""
    files, directories, excluded = inventory(root)
    content_changes: list[tuple[Path, str]] = []
    skipped_binary: list[str] = []
    for path in files:
        text, binary = read_text(path)
        if binary:
            skipped_binary.append(path.relative_to(root).as_posix())
            continue
        assert text is not None
        updated = replace_exact(text, replacements)
        if updated != text:
            content_changes.append((path, updated))

    path_changes: list[tuple[Path, Path]] = []
    for path in files + directories:
        destination = renamed_path(path, root, replacements)
        if destination != path:
            path_changes.append((path, destination))

    sources = {source for source, _ in path_changes}
    destinations: set[Path] = set()
    for source, destination in path_changes:
        if destination in destinations:
            raise ValueError(f"多个路径将映射到同一目标：{destination.relative_to(root)}")
        destinations.add(destination)
        if destination.exists() and destination not in sources:
            raise ValueError(f"目标已存在：{destination.relative_to(root)}")
    return content_changes, path_changes, excluded, skipped_binary


def apply_changes(
    content_changes: list[tuple[Path, str]], path_changes: list[tuple[Path, Path]]
) -> None:
    """先原子替换文本，再按路径深度倒序改名，避免父目录先移动子项。"""
    for path, updated in content_changes:
        original_mode = path.stat().st_mode
        temporary = path.with_name(path.name + ".desktop-rename-project-identity.tmp")
        temporary.write_text(updated, encoding="utf-8")
        temporary.chmod(original_mode)
        temporary.replace(path)
    for source, destination in sorted(
        path_changes, key=lambda pair: len(pair[0].parts), reverse=True
    ):
        destination.parent.mkdir(parents=True, exist_ok=True)
        source.rename(destination)


def plan_root_rename(root: Path, old_id: str, new_id: str, enabled: bool) -> Path | None:
    """仅在显式启用且根目录名精确等于旧标识时规划同级目录改名。"""
    if not enabled:
        return None
    if root.name != old_id:
        raise ValueError("--rename-root 要求项目根目录基本名称与 --old-id 相同")
    destination = root.with_name(new_id)
    if destination.exists():
        raise ValueError(f"根目录重命名目标已存在：{destination}")
    return destination


def find_residuals(root: Path, old_values: list[str]) -> list[str]:
    """复扫维护树中的旧身份文本和路径，确保应用后没有静默遗漏。"""
    files, directories, _ = inventory(root)
    residuals: list[str] = []
    for path in files + directories:
        relative = path.relative_to(root).as_posix()
        if any(old in relative for old in old_values):
            residuals.append(f"path:{relative}")
    for path in files:
        text, binary = read_text(path)
        if binary or text is None:
            continue
        if any(old in text for old in old_values):
            residuals.append(f"text:{path.relative_to(root).as_posix()}")
    return sorted(set(residuals))


def main() -> int:
    """输出机器可读计划，并仅在显式 `--apply` 时执行和复验。"""
    args = parse_arguments()
    root = args.root.expanduser().resolve()
    original_root = root
    if not root.is_dir():
        raise ValueError(f"项目根目录不是目录：{root}")
    if not SNAKE_CASE.fullmatch(args.old_id) or not SNAKE_CASE.fullmatch(args.new_id):
        raise ValueError("--old-id 和 --new-id 必须是 ASCII snake_case")
    if not KEBAB_CASE.fullmatch(args.old_kebab) or not KEBAB_CASE.fullmatch(args.new_kebab):
        raise ValueError("--old-kebab 和 --new-kebab 必须是小写 kebab-case")

    replacements = build_replacements(args)
    root_destination = plan_root_rename(root, args.old_id, args.new_id, args.rename_root)
    content_changes, path_changes, excluded, skipped_binary = plan_changes(root, replacements)
    if args.apply:
        apply_changes(content_changes, path_changes)
        if root_destination is not None:
            root.rename(root_destination)
            root = root_destination
    residuals = find_residuals(root, [old for old, _ in replacements]) if args.apply else []
    result = {
        "mode": "apply" if args.apply else "preview",
        "root": str(root),
        "rootRename": str(root_destination) if root_destination is not None else None,
        "replacements": [{"old": old, "new": new} for old, new in replacements],
        "contentFiles": [
            path.relative_to(original_root).as_posix() for path, _ in content_changes
        ],
        "pathRenames": [
            {
                "from": source.relative_to(original_root).as_posix(),
                "to": destination.relative_to(original_root).as_posix(),
            }
            for source, destination in path_changes
        ],
        "excludedDirectories": sorted(excluded),
        "skippedBinaryFiles": sorted(skipped_binary),
        "residuals": residuals,
    }
    print(json.dumps(result, ensure_ascii=False, indent=2, sort_keys=True))
    return 2 if residuals else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print(json.dumps({"error": str(error)}, ensure_ascii=False), file=sys.stderr)
        sys.exit(1)
