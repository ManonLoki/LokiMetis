"""Harness 升级所有权、文件树和共同基线读取。"""

from __future__ import annotations

import fnmatch
import os
from pathlib import Path
import stat
from typing import Any

from harness_upgrade_policy import (
    MINIMUM_OWNERSHIP_RULES,
    REQUIRED_MANAGED_SOURCE_PATHS,
    SCHEMA_VERSION,
    VALID_MODES,
)
from harness_upgrade_safety import (
    Snapshot,
    UpgradeError,
    load_json,
    safe_relative_path,
    snapshot_file,
    validate_snapshot,
)


def load_ownership(path: Path) -> tuple[str, list[tuple[str, str]]]:
    """加载有序所有权规则；具体规则必须位于兜底规则之前。"""

    data = load_json(path, "所有权 manifest")
    if data.get("schema_version") != 1:
        raise UpgradeError("所有权 manifest 的 schema_version 必须为 1")
    default_mode = data.get("default_mode")
    if default_mode != "protected":
        raise UpgradeError("所有权 manifest 的 default_mode 必须为 protected")
    raw_rules = data.get("rules")
    if not isinstance(raw_rules, list) or not raw_rules:
        raise UpgradeError("所有权 manifest 的 rules 必须是非空列表")
    rules: list[tuple[str, str]] = []
    seen_patterns: set[str] = set()
    for index, item in enumerate(raw_rules):
        if not isinstance(item, dict) or set(item) != {"pattern", "mode"}:
            raise UpgradeError(f"所有权规则 {index} 必须包含 pattern/mode")
        pattern = item.get("pattern")
        mode = item.get("mode")
        if not isinstance(pattern, str) or not pattern:
            raise UpgradeError(f"所有权规则 {index} 的 pattern 非法")
        if pattern in seen_patterns:
            raise UpgradeError(f"所有权规则的 pattern 重复：{pattern}")
        if mode not in VALID_MODES:
            raise UpgradeError(f"所有权规则 {index} 的 mode 非法：{mode!r}")
        seen_patterns.add(pattern)
        rules.append((pattern, mode))
    observed_rules = dict(rules)
    for pattern, expected_mode in MINIMUM_OWNERSHIP_RULES.items():
        if observed_rules.get(pattern) != expected_mode:
            raise UpgradeError(
                "所有权 manifest 削弱了必需保护："
                f"{pattern} 必须为 {expected_mode}"
            )
        probe = (
            f"{pattern[:-3]}/__harness_policy_probe__"
            if pattern.endswith("/**")
            else pattern
        )
        effective_mode = next(
            (
                mode
                for earlier_pattern, mode in rules
                if fnmatch.fnmatchcase(probe, earlier_pattern)
            ),
            default_mode,
        )
        if effective_mode != expected_mode:
            raise UpgradeError(
                "所有权 manifest 的规则顺序削弱了必需保护："
                f"{pattern} 的有效 mode 必须为 {expected_mode}"
            )
    self_index = next(
        index
        for index, item in enumerate(rules)
        if item == (".agents/skills/desktop-upgrade-harness/**", "managed-self")
    )
    generic_index = next(
        index
        for index, item in enumerate(rules)
        if item == (".agents/skills/**", "managed")
    )
    if self_index >= generic_index:
        raise UpgradeError(
            "managed-self 所有权规则必须位于通用 managed 规则之前"
        )
    for required_path in REQUIRED_MANAGED_SOURCE_PATHS:
        effective_mode = next(
            (
                mode
                for pattern, mode in rules
                if fnmatch.fnmatchcase(required_path, pattern)
            ),
            default_mode,
        )
        if effective_mode != "managed":
            raise UpgradeError(
                f"必需传播路径的有效所有权必须保持 managed：{required_path}"
            )
    return default_mode, rules


def ownership_mode(path: str, default_mode: str, rules: list[tuple[str, str]]) -> str:
    """按清单顺序返回路径所有权。"""

    for pattern, mode in rules:
        if fnmatch.fnmatchcase(path, pattern):
            return mode
    return default_mode


def explicit_ownership_mode_for_node(
    path: str,
    *,
    is_directory: bool,
    rules: list[tuple[str, str]],
) -> str | None:
    """返回节点命中的显式规则；目录额外用后代探针匹配 `/**` 规则。"""

    if is_directory:
        probe = f"{path}/__harness_node_probe__"
        for pattern, mode in rules:
            if fnmatch.fnmatchcase(probe, pattern):
                return mode
    for pattern, mode in rules:
        if fnmatch.fnmatchcase(path, pattern):
            return mode
    return None


def scan_tree(
    root: Path,
    *,
    target_tree: bool,
) -> tuple[dict[str, Snapshot], list[str], dict[str, bool]]:
    """枚举所有节点及普通文件快照，并报告链接、特殊文件和候选 Git 元数据。"""

    files: dict[str, Snapshot] = {}
    unsafe: list[str] = []
    nodes: dict[str, bool] = {}
    for current, directories, filenames in os.walk(root, followlinks=False):
        current_path = Path(current)
        kept_directories: list[str] = []
        for name in directories:
            child = current_path / name
            relative = child.relative_to(root).as_posix()
            if target_tree and relative == ".git":
                continue
            nodes[relative] = True
            is_junction = bool(
                hasattr(os.path, "isjunction") and os.path.isjunction(child)
            )
            if child.is_symlink() or is_junction:
                unsafe.append(relative)
            elif not child.is_dir():
                unsafe.append(relative)
            elif not target_tree and relative == ".git":
                unsafe.append(relative)
            else:
                kept_directories.append(name)
        directories[:] = kept_directories
        for name in filenames:
            child = current_path / name
            relative = child.relative_to(root).as_posix()
            nodes[relative] = False
            try:
                observed = child.lstat()
            except OSError as exc:
                raise UpgradeError(f"无法检查 {child}：{exc}") from exc
            is_junction = bool(
                hasattr(os.path, "isjunction") and os.path.isjunction(child)
            )
            if (
                stat.S_ISLNK(observed.st_mode)
                or is_junction
                or not stat.S_ISREG(observed.st_mode)
            ):
                unsafe.append(relative)
                continue
            files[relative] = snapshot_file(child)
    return files, sorted(unsafe), nodes


def load_lock(path: Path) -> dict[str, Any] | None:
    """读取并严格校验上一版来源锁；缺失表示必须走 bootstrap audit。"""

    if not os.path.lexists(path):
        return None
    data = load_json(path, "上游 lock")
    if data.get("schema_version") != SCHEMA_VERSION:
        raise UpgradeError(
            f"上游 lock 的 schema_version 必须为 {SCHEMA_VERSION}"
        )
    entries = data.get("entries")
    if not isinstance(entries, dict):
        raise UpgradeError("上游 lock 的 entries 必须是 object")
    for raw_path, entry in entries.items():
        path_key = safe_relative_path(raw_path)
        if path_key != raw_path or not isinstance(entry, dict):
            raise UpgradeError(f"上游 lock entry 非法：{raw_path!r}")
        if set(entry) != {"mode", "candidate", "target"}:
            raise UpgradeError(f"{raw_path} 的上游 lock 字段非法")
        if entry.get("mode") not in VALID_MODES:
            raise UpgradeError(f"{raw_path} 的上游 lock mode 非法")
        validate_snapshot(entry.get("candidate"), f"{raw_path}.candidate")
        validate_snapshot(entry.get("target"), f"{raw_path}.target")
    return data
