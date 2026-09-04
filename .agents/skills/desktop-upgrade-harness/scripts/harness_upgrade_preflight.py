"""Harness 升级写入前的快照与计划复验。"""

from __future__ import annotations

import os
from pathlib import Path
from typing import Any

from harness_upgrade_safety import (
    Snapshot,
    UpgradeError,
    assert_safe_path,
    canonical_directory,
    require_git_root,
    safe_relative_path,
    snapshot_file,
    validate_snapshot,
)


def assert_current_snapshot(
    root: Path,
    relative: str,
    expected: Snapshot | None,
    label: str,
) -> Path:
    """重验普通文件快照和祖先边界，阻止计划后的静默变化。"""

    path = assert_safe_path(
        root,
        root / relative,
        label,
        final_may_be_missing=expected is None,
    )
    if expected is None:
        if os.path.lexists(path):
            raise UpgradeError(f"{label}意外存在：{relative}")
        return path
    if not path.is_file() or path.is_symlink():
        raise UpgradeError(f"{label}不是普通文件：{relative}")
    observed = snapshot_file(path)
    if observed != expected:
        raise UpgradeError(
            f"{label}在 plan 后发生变化：{relative}；"
            f"预期 {expected}，实际 {observed}"
        )
    return path


def assert_plan_still_current(plan: dict[str, Any]) -> None:
    """在写入前重验控制文件、Git 身份以及计划覆盖的两棵文件树。"""

    candidate = canonical_directory(Path(plan["candidate_root"]), "候选根目录")
    target = canonical_directory(Path(plan["target_root"]), "目标根目录")
    ownership = Path(plan["ownership_path"])
    if snapshot_file(ownership) != validate_snapshot(
        plan["ownership_snapshot"],
        "ownership_snapshot",
    ):
        raise UpgradeError("所有权 manifest 在 plan 后发生变化")
    lock_path = Path(plan["lock_path"])
    expected_lock = validate_snapshot(plan["lock_snapshot"], "lock_snapshot")
    if expected_lock is None:
        if os.path.lexists(lock_path):
            raise UpgradeError("上游 lock 在 plan 后出现")
    elif snapshot_file(lock_path) != expected_lock:
        raise UpgradeError("上游 lock 在 plan 后发生变化")
    if require_git_root(target) != plan["target_git"]:
        raise UpgradeError("目标 Git 身份或工作树状态在 plan 后发生变化")
    for item in plan["actions"]:
        relative = safe_relative_path(item["path"])
        assert_current_snapshot(
            candidate,
            relative,
            validate_snapshot(item.get("candidate"), f"{relative}.candidate"),
            "候选",
        )
        assert_current_snapshot(
            target,
            relative,
            validate_snapshot(item.get("target"), f"{relative}.target"),
            "目标",
        )
