"""完成 Harness 升级后原子记录共同基线。"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
from pathlib import Path
import stat
import tempfile
from typing import Any
import uuid

from harness_upgrade_core import load_reviewed_plan
from harness_upgrade_ownership import load_lock
from harness_upgrade_policy import AUTO_MODES, SCHEMA_VERSION
from harness_upgrade_safety import (
    UpgradeError,
    assert_safe_path,
    canonical_directory,
    safe_relative_path,
)
from harness_upgrade_preflight import assert_plan_still_current


def write_all(file_descriptor: int, payload: bytes) -> None:
    """把完整载荷写入描述符，避免短写造成锁文件截断。"""

    remaining = memoryview(payload)
    while remaining:
        written = os.write(file_descriptor, remaining)
        remaining = remaining[written:]


def write_lock_posix(target: Path, payload: bytes) -> None:
    """以稳定目录描述符创建 `.harness` 并原子替换来源锁。"""

    directory_flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        directory_flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        directory_flags |= os.O_NOFOLLOW
    root_descriptor = os.open(target, directory_flags)
    harness_descriptor = -1
    temporary_name = f".upstream-lock-{uuid.uuid4().hex}.tmp"
    temporary_created = False
    try:
        try:
            os.mkdir(".harness", mode=0o755, dir_fd=root_descriptor)
        except FileExistsError:
            pass
        harness_descriptor = os.open(
            ".harness",
            directory_flags,
            dir_fd=root_descriptor,
        )
        if not stat.S_ISDIR(os.fstat(harness_descriptor).st_mode):
            raise UpgradeError(".harness 不是稳定目录")
        write_flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
        if hasattr(os, "O_NOFOLLOW"):
            write_flags |= os.O_NOFOLLOW
        descriptor = os.open(
            temporary_name,
            write_flags,
            0o600,
            dir_fd=harness_descriptor,
        )
        temporary_created = True
        try:
            write_all(descriptor, payload)
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
        try:
            observed = os.stat(
                "upstream-lock.json",
                dir_fd=harness_descriptor,
                follow_symlinks=False,
            )
        except FileNotFoundError:
            observed = None
        if observed is not None and not stat.S_ISREG(observed.st_mode):
            raise UpgradeError("上游 lock 不是普通文件")
        os.replace(
            temporary_name,
            "upstream-lock.json",
            src_dir_fd=harness_descriptor,
            dst_dir_fd=harness_descriptor,
        )
        temporary_created = False
        os.fsync(harness_descriptor)
    finally:
        if temporary_created and harness_descriptor != -1:
            try:
                os.unlink(temporary_name, dir_fd=harness_descriptor)
            except OSError:
                pass
        if harness_descriptor != -1:
            os.close(harness_descriptor)
        os.close(root_descriptor)


def write_lock_atomic(target: Path, lock_path: Path, value: dict[str, Any]) -> None:
    """只在精确 `.harness` 目录内原子替换来源锁。"""

    assert_safe_path(
        target,
        lock_path,
        "上游 lock",
        final_may_be_missing=True,
    )
    payload = (
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")
    if os.name == "posix":
        write_lock_posix(target, payload)
        return

    harness_directory = target / ".harness"
    if not os.path.lexists(harness_directory):
        harness_directory.mkdir(mode=0o755)
    assert_safe_path(
        target,
        harness_directory,
        "上游 lock 目录",
        final_may_be_missing=False,
    )
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=harness_directory, delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        assert_safe_path(
            target,
            lock_path,
            "上游 lock",
            final_may_be_missing=True,
        )
        if lock_path.is_symlink():
            raise UpgradeError(f"拒绝替换符号链接 lock：{lock_path}")
        os.replace(temporary, lock_path)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def record_lock(args: argparse.Namespace) -> dict[str, Any]:
    """把已复核且已完成应用/人工合并的计划记录为新共同基线。"""

    expected = "bootstrap-verified-baseline" if args.bootstrap else "record-verified-baseline"
    if args.approval != expected:
        raise UpgradeError(f"record 要求传入 --approval {expected}")
    plan, _ = load_reviewed_plan(args.plan)
    target = canonical_directory(Path(plan["target_root"]), "目标根目录")
    if (
        args.source_version != plan["source_version"]
        or args.source_commit != plan["source_commit"]
    ):
        raise UpgradeError(
            "record 的源版本/commit 必须与已复核 plan 精确匹配"
        )
    assert_plan_still_current(plan)
    lock_path = Path(plan["lock_path"])
    existing_lock = load_lock(lock_path)

    manual_paths = {
        item["path"]
        for item in plan["actions"]
        if item["classification"] == "manual_merge"
    }
    resolved_manual = {safe_relative_path(path) for path in args.resolved_manual}
    if resolved_manual != manual_paths:
        raise UpgradeError(
            "record 要求 --resolved-manual 精确列出已复核混合所有权操作的路径："
            f"预期={sorted(manual_paths)}，实际={sorted(resolved_manual)}"
        )
    if plan["problems"]:
        raise UpgradeError("存在路径或所有权问题的 plan 不能 record")

    if args.bootstrap:
        if plan["baseline"] != "missing" or existing_lock is not None:
            raise UpgradeError("bootstrap record 要求 lock 不存在")
        for item in plan["actions"]:
            classification = item["classification"]
            if classification in {"protected_candidate", "tombstone_candidate", "tombstone_present"}:
                raise UpgradeError(f"bootstrap 包含禁止路径：{item['path']}")
            if item["mode"] in AUTO_MODES:
                if classification in {"add", "delete", "update", "conflict", "collision"}:
                    raise UpgradeError(
                        f"bootstrap 的 managed 路径尚未解决：{item['path']}"
                    )
                if (
                    classification == "bootstrap_conflict"
                    and item["candidate"] != item["target"]
                ):
                    raise UpgradeError(
                        f"bootstrap 的 managed 重叠项必须先收敛：{item['path']}"
                    )
                if item["blocked"] and classification != "bootstrap_conflict":
                    raise UpgradeError(
                        f"bootstrap 包含不可复核的 blocker：{item['path']}"
                    )
            elif classification == "manual_add":
                raise UpgradeError(
                    f"bootstrap 的 mixed 路径必须先创建并重新生成 plan：{item['path']}"
                )
    else:
        if plan["baseline"] != "loaded" or existing_lock is None:
            raise UpgradeError("非 bootstrap record 要求已有 lock")
        if plan["blocked"]:
            raise UpgradeError("不能 record 已阻断的升级 plan")
        unresolved = [
            item["path"]
            for item in plan["actions"]
            if item["classification"] in {"add", "delete", "update", "manual_add"}
        ]
        if unresolved:
            raise UpgradeError(
                "仍有 managed 操作未解决：" + ", ".join(unresolved)
            )

    previous_entries = {} if existing_lock is None else existing_lock["entries"]
    entries: dict[str, Any] = {}
    for item in plan["actions"]:
        if item["mode"] in {"protected", "tombstone"}:
            continue
        if item["candidate"] is None and item["target"] is None:
            continue
        if (
            item["mode"] in AUTO_MODES
            and item["classification"] == "preserve_local"
            and item["path"] in previous_entries
        ):
            entries[item["path"]] = previous_entries[item["path"]]
            continue
        entries[item["path"]] = {
            "mode": item["mode"],
            "candidate": item["candidate"],
            "target": item["target"],
        }
    lock = {
        "schema_version": SCHEMA_VERSION,
        "recorded_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "harness_source": {
            "version": plan["source_version"],
            "commit": plan["source_commit"],
        },
        "entries": entries,
    }
    assert_plan_still_current(plan)
    write_lock_atomic(target, lock_path, lock)
    return {"recorded": str(lock_path), "entries": len(entries)}
