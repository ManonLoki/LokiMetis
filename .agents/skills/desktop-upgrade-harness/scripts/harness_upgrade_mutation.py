"""执行受审 Harness 升级计划中的单文件原子更新。"""

from __future__ import annotations

import hashlib
import os
from pathlib import Path
import shutil
import stat
import tempfile
from typing import Any
import uuid

from harness_upgrade_core import (
    load_reviewed_plan,
)
from harness_upgrade_policy import AUTO_MODES
from harness_upgrade_safety import (
    Snapshot,
    UpgradeError,
    canonical_directory,
    safe_relative_path,
    snapshot_fd,
    snapshot_file,
    validate_snapshot,
)
from harness_upgrade_preflight import (
    assert_current_snapshot,
    assert_plan_still_current,
)


def open_parent_descriptor(root: Path, relative: str) -> tuple[int, str]:
    """通过 O_NOFOLLOW 逐级打开既有父目录，返回稳定目录描述符。"""

    parts = Path(relative).parts
    if not parts:
        raise UpgradeError(f"文件路径非法：{relative}")
    flags = os.O_RDONLY
    if hasattr(os, "O_DIRECTORY"):
        flags |= os.O_DIRECTORY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    descriptor = os.open(root, flags)
    try:
        for part in parts[:-1]:
            next_descriptor = os.open(part, flags, dir_fd=descriptor)
            observed = os.fstat(next_descriptor)
            if not stat.S_ISDIR(observed.st_mode):
                os.close(next_descriptor)
                raise UpgradeError(f"父路径不是目录：{relative}")
            os.close(descriptor)
            descriptor = next_descriptor
        return descriptor, parts[-1]
    except Exception:
        os.close(descriptor)
        raise


def replace_existing_posix(
    candidate_root: Path,
    target_root: Path,
    relative: str,
    candidate_snapshot: Snapshot,
    target_snapshot: Snapshot,
) -> None:
    """使用稳定父目录描述符原子替换一个既有受管文件。"""

    source_parent, source_name = open_parent_descriptor(candidate_root, relative)
    target_parent, target_name = open_parent_descriptor(target_root, relative)
    source_descriptor = -1
    temporary_name = f".harness-upgrade-{uuid.uuid4().hex}.tmp"
    temporary_created = False
    try:
        read_flags = os.O_RDONLY
        if hasattr(os, "O_NOFOLLOW"):
            read_flags |= os.O_NOFOLLOW
        source_descriptor = os.open(source_name, read_flags, dir_fd=source_parent)
        if snapshot_fd(source_descriptor) != candidate_snapshot:
            raise UpgradeError(f"候选在 apply 期间发生变化：{relative}")

        target_descriptor = os.open(target_name, read_flags, dir_fd=target_parent)
        try:
            if snapshot_fd(target_descriptor) != target_snapshot:
                raise UpgradeError(f"目标在 apply 期间发生变化：{relative}")
        finally:
            os.close(target_descriptor)

        write_flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
        if hasattr(os, "O_NOFOLLOW"):
            write_flags |= os.O_NOFOLLOW
        temporary_descriptor = os.open(
            temporary_name,
            write_flags,
            int(candidate_snapshot["mode"]),
            dir_fd=target_parent,
        )
        temporary_created = True
        copied_digest = hashlib.sha256()
        try:
            while chunk := os.read(source_descriptor, 1024 * 1024):
                copied_digest.update(chunk)
                view = memoryview(chunk)
                while view:
                    written = os.write(temporary_descriptor, view)
                    view = view[written:]
            if hasattr(os, "fchmod"):
                os.fchmod(temporary_descriptor, int(candidate_snapshot["mode"]))
            os.fsync(temporary_descriptor)
        finally:
            os.close(temporary_descriptor)
        if copied_digest.hexdigest() != candidate_snapshot["sha256"]:
            raise UpgradeError(f"候选在复制期间发生变化：{relative}")
        if stat.S_IMODE(os.fstat(source_descriptor).st_mode) != candidate_snapshot["mode"]:
            raise UpgradeError(f"候选 mode 在复制期间发生变化：{relative}")

        target_descriptor = os.open(target_name, read_flags, dir_fd=target_parent)
        try:
            if snapshot_fd(target_descriptor) != target_snapshot:
                raise UpgradeError(f"目标在 replace 前发生变化：{relative}")
        finally:
            os.close(target_descriptor)
        os.replace(
            temporary_name,
            target_name,
            src_dir_fd=target_parent,
            dst_dir_fd=target_parent,
        )
        temporary_created = False
        os.fsync(target_parent)
    finally:
        if source_descriptor != -1:
            os.close(source_descriptor)
        if temporary_created:
            try:
                os.unlink(temporary_name, dir_fd=target_parent)
            except OSError:
                pass
        os.close(source_parent)
        os.close(target_parent)


def replace_existing_portable(
    source: Path,
    destination: Path,
    candidate_snapshot: Snapshot,
    target_snapshot: Snapshot,
) -> None:
    """在无 dir_fd 平台以双重边界复验和同目录原子替换更新文件。"""

    if snapshot_file(source) != candidate_snapshot:
        raise UpgradeError(f"候选在 apply 期间发生变化：{source}")
    if snapshot_file(destination) != target_snapshot:
        raise UpgradeError(f"目标在 apply 期间发生变化：{destination}")
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as stream:
            temporary = Path(stream.name)
            with source.open("rb") as source_stream:
                shutil.copyfileobj(source_stream, stream)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, int(candidate_snapshot["mode"]))
        if snapshot_file(temporary) != candidate_snapshot:
            raise UpgradeError(f"候选在复制期间发生变化：{source}")
        if snapshot_file(destination) != target_snapshot:
            raise UpgradeError(f"目标在 replace 前发生变化：{destination}")
        os.replace(temporary, destination)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def apply_plan(plan_path: Path, approval: str, selected_path: str) -> dict[str, Any]:
    """重算计划后一次只替换一个既有受管文件，避免批量部分应用。"""

    if approval != "apply-managed-changes":
        raise UpgradeError("apply 要求传入 --approval apply-managed-changes")
    plan, _ = load_reviewed_plan(plan_path)
    if plan["blocked"]:
        raise UpgradeError("升级 plan 已被阻断；请在 apply 前解决 conflict")
    pending = [
        item
        for item in plan["actions"]
        if isinstance(item, dict) and item.get("auto_apply") is True
    ]
    for item in pending:
        relative = safe_relative_path(item["path"])
        if (
            item.get("mode") not in AUTO_MODES
            or item.get("classification") != "update"
        ):
            raise UpgradeError(f"plan 包含不安全的自动操作：{relative}")
    selected = safe_relative_path(selected_path)
    matches = [item for item in pending if item["path"] == selected]
    if len(matches) != 1:
        raise UpgradeError(
            f"--path 必须精确指定一个已复核且可自动应用的 update：{selected}"
        )
    item = matches[0]
    if item["mode"] == "managed-self":
        normal_pending = [
            action["path"] for action in pending if action["mode"] != "managed-self"
        ]
        if normal_pending:
            raise UpgradeError(
                "managed-self 更新必须等待全部普通 managed 更新解决后再执行："
                + ", ".join(sorted(normal_pending))
            )
        self_pending = sorted(
            action["path"]
            for action in pending
            if action["mode"] == "managed-self"
        )
        self_pending.sort(
            key=lambda path: (Path(path).name == "harness_upgrade.py", path)
        )
        if selected != self_pending[0]:
            raise UpgradeError(
                f"managed-self 更新顺序要求先处理 {self_pending[0]}，再处理 {selected}"
            )

    assert_plan_still_current(plan)
    candidate = canonical_directory(Path(plan["candidate_root"]), "候选根目录")
    target = canonical_directory(Path(plan["target_root"]), "目标根目录")
    candidate_snapshot = validate_snapshot(
        item.get("candidate"), f"{selected}.candidate"
    )
    target_snapshot = validate_snapshot(item.get("target"), f"{selected}.target")
    if candidate_snapshot is None or target_snapshot is None:
        raise UpgradeError(f"update 要求两个文件都已存在：{selected}")
    source = assert_current_snapshot(
        candidate,
        selected,
        candidate_snapshot,
        "候选",
    )
    destination = assert_current_snapshot(
        target,
        selected,
        target_snapshot,
        "目标",
    )
    if os.name == "posix":
        replace_existing_posix(
            candidate,
            target,
            selected,
            candidate_snapshot,
            target_snapshot,
        )
    else:
        replace_existing_portable(
            source,
            destination,
            candidate_snapshot,
            target_snapshot,
        )
    return {"applied": [selected], "count": 1}
