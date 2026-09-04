#!/usr/bin/env python3
"""为下游 Harness 工程升级生成三方计划并安全更新既有受管文件。"""

from __future__ import annotations

import json
import os
from pathlib import Path
import stat
from typing import Any

from harness_upgrade_ownership import (
    explicit_ownership_mode_for_node,
    load_lock,
    load_ownership,
    ownership_mode,
    scan_tree,
)
from harness_upgrade_policy import (
    AUTO_MODES,
    BLOCKING_CLASSES,
    MANUAL_CLASSES,
    REQUIRED_MANAGED_SOURCE_PATHS,
    SCHEMA_VERSION,
)
from harness_upgrade_safety import (
    Snapshot,
    UpgradeError,
    assert_safe_path,
    canonical_directory,
    is_within,
    load_json,
    require_control_paths,
    require_git_root,
    require_source_identity,
    safe_relative_path,
    snapshot_file,
)


def classify_managed(
    candidate: Snapshot | None,
    target: Snapshot | None,
    baseline: dict[str, Any] | None,
    *,
    lock_loaded: bool,
) -> str:
    """按三方快照为纯受管路径生成确定性分类。"""

    if baseline is None:
        if candidate is not None and target is None:
            return "add"
        if candidate is not None and target is not None:
            if lock_loaded:
                return "converged" if candidate == target else "collision"
            return "bootstrap_conflict"
        return "preserve_local"
    old_candidate = baseline.get("candidate")
    old_target = baseline.get("target")
    candidate_changed = candidate != old_candidate
    target_changed = target != old_target
    if candidate_changed and target_changed:
        return "converged" if candidate == target else "conflict"
    if target_changed:
        return "preserve_local"
    if not candidate_changed:
        return "unchanged"
    if candidate is None:
        return "delete"
    if target is None and old_target is not None:
        return "conflict"
    return "update"


def classify_mixed(
    candidate: Snapshot | None,
    target: Snapshot | None,
    baseline: dict[str, Any] | None,
) -> str:
    """为共享或条件所有权路径标记人工合并需求。"""

    if baseline is None:
        if candidate is None:
            return "preserve_local"
        if candidate == target:
            return "converged"
        return "manual_merge" if target is not None else "manual_add"
    if candidate == baseline.get("candidate"):
        return (
            "preserve_local"
            if target != baseline.get("target")
            else "unchanged"
        )
    if candidate == target:
        return "converged"
    return "manual_merge"


def build_plan(
    source_root: Path,
    source_version: str,
    source_commit: str,
    candidate_root: Path,
    target_root: Path,
    ownership_path: Path,
    lock_path: Path,
) -> dict[str, Any]:
    """生成完整只读升级计划，不更改候选、目标或来源锁。"""

    source, source_identity = require_source_identity(
        source_root,
        source_version,
        source_commit,
    )
    candidate = canonical_directory(candidate_root, "候选根目录")
    target = canonical_directory(target_root, "目标根目录")
    roots = (source, candidate, target)
    for index, left in enumerate(roots):
        for right in roots[index + 1 :]:
            if left == right or left in right.parents or right in left.parents:
                raise UpgradeError(
                    "源、候选和目标根目录必须彼此独立且不能嵌套"
                )
    git_identity = require_git_root(target)
    ownership, lock_file = require_control_paths(target, ownership_path, lock_path)
    default_mode, rules = load_ownership(ownership)
    candidate_files, candidate_unsafe, candidate_nodes = scan_tree(
        candidate,
        target_tree=False,
    )
    target_files, target_unsafe, target_nodes = scan_tree(
        target,
        target_tree=True,
    )
    lock = load_lock(lock_file)
    entries = {} if lock is None else lock["entries"]

    problems = [
        f"候选包含符号链接、特殊文件或 Git 元数据：{path}"
        for path in candidate_unsafe
    ]
    for raw_path in REQUIRED_MANAGED_SOURCE_PATHS:
        path = safe_relative_path(raw_path)
        source_path = assert_safe_path(
            source,
            source / path,
            f"源 Harness 必需传播路径 {path}",
            final_may_be_missing=True,
        )
        if not os.path.lexists(source_path):
            continue
        if not stat.S_ISREG(os.lstat(source_path).st_mode):
            problems.append(f"源 Harness 必需传播路径不是普通文件：{path}")
        elif path not in candidate_files:
            problems.append(f"候选缺少源 Harness 必需 managed 路径：{path}")
        elif candidate_files[path]["sha256"] != snapshot_file(source_path)["sha256"]:
            problems.append(f"候选的源 Harness 必需 managed 路径内容不匹配：{path}")
    for path, is_directory in candidate_nodes.items():
        mode = explicit_ownership_mode_for_node(
            path,
            is_directory=is_directory,
            rules=rules,
        )
        if mode in {"protected", "tombstone"}:
            problems.append(f"候选包含禁止的 {mode} 路径：{path}")
    for path, is_directory in target_nodes.items():
        mode = explicit_ownership_mode_for_node(
            path,
            is_directory=is_directory,
            rules=rules,
        )
        if mode == "tombstone":
            problems.append(f"目标包含 tombstone 路径：{path}")

    relevant = set(candidate_files) | set(entries)
    for path in target_files:
        if ownership_mode(path, default_mode, rules) == "tombstone" or path in entries:
            relevant.add(path)

    for unsafe in target_unsafe:
        if any(path == unsafe or path.startswith(f"{unsafe}/") for path in relevant):
            problems.append(f"目标相关路径是符号链接或特殊文件：{unsafe}")

    actions: list[dict[str, Any]] = []
    for raw_path in sorted(relevant):
        path = safe_relative_path(raw_path)
        mode = ownership_mode(path, default_mode, rules)
        candidate_snapshot = candidate_files.get(path)
        target_snapshot = target_files.get(path)
        baseline = entries.get(path)
        if baseline is not None and baseline.get("mode") != mode:
            problems.append(
                f"{path} 的所有权 mode 已变化："
                f"{baseline.get('mode')} -> {mode}"
            )
        if mode == "tombstone":
            if candidate_snapshot is not None:
                classification = "tombstone_candidate"
            elif target_snapshot is not None:
                classification = "tombstone_present"
            else:
                classification = "tombstone_absent"
        elif mode == "protected":
            classification = (
                "protected_candidate"
                if candidate_snapshot is not None
                else "protected"
            )
        elif mode in AUTO_MODES:
            classification = classify_managed(
                candidate_snapshot,
                target_snapshot,
                baseline,
                lock_loaded=lock is not None,
            )
        else:
            classification = classify_mixed(
                candidate_snapshot,
                target_snapshot,
                baseline,
            )
        blocked = classification in BLOCKING_CLASSES
        auto_apply = mode in AUTO_MODES and classification == "update"
        actions.append(
            {
                "path": path,
                "mode": mode,
                "classification": classification,
                "candidate": candidate_snapshot,
                "target": target_snapshot,
                "baseline_candidate": (
                    None if baseline is None else baseline.get("candidate")
                ),
                "baseline_target": (
                    None if baseline is None else baseline.get("target")
                ),
                "auto_apply": auto_apply,
                "blocked": blocked,
            }
        )

    ownership_snapshot = snapshot_file(ownership)
    lock_snapshot = snapshot_file(lock_file) if lock is not None else None
    return {
        "schema_version": SCHEMA_VERSION,
        "source_root": str(source),
        "source_version": source_version,
        "source_commit": source_commit,
        "source_git": source_identity,
        "candidate_root": str(candidate),
        "target_root": str(target),
        "ownership_path": str(ownership),
        "ownership_snapshot": ownership_snapshot,
        "lock_path": str(lock_file),
        "lock_snapshot": lock_snapshot,
        "target_git": git_identity,
        "baseline": "missing" if lock is None else "loaded",
        "blocked": bool(problems) or any(item["blocked"] for item in actions),
        "manual_required": any(
            item["classification"] in MANUAL_CLASSES for item in actions
        ),
        "problems": sorted(problems),
        "actions": actions,
    }


def write_new_plan(path: Path, value: dict[str, Any], forbidden_roots: tuple[Path, ...]) -> None:
    """只在候选/目标之外独占创建新计划，禁止覆盖任何现有文件。"""

    parent = canonical_directory(path.parent, "plan 输出父目录")
    output = parent / path.name
    for root in forbidden_roots:
        if is_within(output, root):
            raise UpgradeError(f"plan 输出必须位于 {root} 之外：{output}")
    if os.path.lexists(output):
        raise UpgradeError(f"plan 输出已存在，拒绝覆盖：{output}")
    payload = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(output, flags, 0o600)
        try:
            remaining = memoryview(payload.encode("utf-8"))
            while remaining:
                written = os.write(descriptor, remaining)
                remaining = remaining[written:]
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        if hasattr(os, "O_DIRECTORY"):
            parent_descriptor = os.open(parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(parent_descriptor)
            finally:
                os.close(parent_descriptor)
    except OSError as exc:
        raise UpgradeError(f"无法创建 plan 输出 {output}：{exc}") from exc


def load_reviewed_plan(plan_path: Path) -> tuple[dict[str, Any], dict[str, Any]]:
    """重算计划并要求与受审 JSON 完全一致，拒绝篡改与状态漂移。"""

    reviewed = load_json(plan_path, "升级 plan")
    if reviewed.get("schema_version") != SCHEMA_VERSION:
        raise UpgradeError(f"升级 plan 的 schema_version 必须为 {SCHEMA_VERSION}")
    required = {
        "source_root",
        "source_version",
        "source_commit",
        "candidate_root",
        "target_root",
        "ownership_path",
        "lock_path",
    }
    if not required.issubset(reviewed):
        raise UpgradeError("升级 plan 缺少 provenance 字段")
    recomputed = build_plan(
        Path(reviewed["source_root"]),
        reviewed["source_version"],
        reviewed["source_commit"],
        Path(reviewed["candidate_root"]),
        Path(reviewed["target_root"]),
        Path(reviewed["ownership_path"]),
        Path(reviewed["lock_path"]),
    )
    if reviewed != recomputed:
        raise UpgradeError(
            "已复核 plan 不再匹配源、所有权、lock、Git 身份、候选或目标；"
            "请生成并复核新 plan"
        )
    return reviewed, recomputed
