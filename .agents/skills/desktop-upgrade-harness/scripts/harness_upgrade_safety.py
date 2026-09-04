"""Harness 升级的路径、快照、来源和控制文件安全边界。"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
from typing import Any

from harness_upgrade_policy import (
    LOCK_RELATIVE,
    OWNERSHIP_RELATIVE,
    VERSION_PATTERN,
)


class UpgradeError(RuntimeError):
    """表示路径、来源、基线或应用条件不满足，调用方必须停止升级。"""


Snapshot = dict[str, str | int]


def lexical_absolute(path: Path) -> Path:
    """生成不追随最终符号链接的绝对规范化路径。"""

    return Path(os.path.abspath(os.fspath(path)))


def is_within(path: Path, root: Path) -> bool:
    """判断一个绝对路径是否等于或位于给定根目录内。"""

    return path == root or root in path.parents


def canonical_directory(path: Path, label: str) -> Path:
    """解析并校验一个非符号链接目录。"""

    if path.is_symlink():
        raise UpgradeError(f"{label}不得是符号链接：{path}")
    try:
        resolved = path.resolve(strict=True)
    except OSError as exc:
        raise UpgradeError(f"无法解析{label} {path}：{exc}") from exc
    if not resolved.is_dir():
        raise UpgradeError(f"{label}不是目录：{resolved}")
    return resolved


def safe_relative_path(raw: str) -> str:
    """规范化清单、lock 与 plan 中的相对路径并拒绝逃逸。"""

    candidate = Path(raw)
    if (
        not raw
        or candidate.is_absolute()
        or ".." in candidate.parts
        or "\x00" in raw
    ):
        raise UpgradeError(f"相对路径不安全：{raw!r}")
    normalized = candidate.as_posix()
    if normalized in {".", ""} or normalized != raw.replace("\\", "/"):
        raise UpgradeError(f"相对路径不是规范形式：{raw!r}")
    return normalized


def assert_safe_path(
    root: Path,
    path: Path,
    label: str,
    *,
    final_may_be_missing: bool,
) -> Path:
    """拒绝根外路径、任一祖先符号链接及中间非目录。"""

    absolute = lexical_absolute(path)
    if not is_within(absolute, root):
        raise UpgradeError(f"{label}越出声明根目录：{absolute}")
    relative = absolute.relative_to(root)
    cursor = root
    parts = relative.parts
    for index, part in enumerate(parts):
        cursor /= part
        exists = os.path.lexists(cursor)
        if not exists:
            if index != len(parts) - 1 and not final_may_be_missing:
                raise UpgradeError(f"{label}的父路径缺失：{cursor}")
            break
        observed = os.lstat(cursor)
        is_junction = bool(
            hasattr(os.path, "isjunction") and os.path.isjunction(cursor)
        )
        if stat.S_ISLNK(observed.st_mode) or is_junction:
            raise UpgradeError(f"{label}包含符号链接或目录联接：{cursor}")
        if index != len(parts) - 1 and not stat.S_ISDIR(observed.st_mode):
            raise UpgradeError(f"{label}的父路径不是目录：{cursor}")
    return absolute


def load_json(path: Path, label: str) -> dict[str, Any]:
    """读取非符号链接 JSON 对象并稳定报告语法或顶层类型错误。"""

    if path.is_symlink() or not path.is_file():
        raise UpgradeError(f"{label}必须是非符号链接的普通文件：{path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise UpgradeError(f"无法读取{label} {path}：{exc}") from exc
    if not isinstance(value, dict):
        raise UpgradeError(f"{label}必须包含 JSON 对象：{path}")
    return value


def snapshot_fd(file_descriptor: int) -> Snapshot:
    """从已打开的普通文件描述符读取内容摘要与权限位。"""

    observed = os.fstat(file_descriptor)
    if not stat.S_ISREG(observed.st_mode):
        raise UpgradeError("快照来源不是普通文件")
    permission_mode = stat.S_IMODE(observed.st_mode)
    if permission_mode & ~0o777:
        raise UpgradeError(
            f"快照来源使用了不受支持的特殊权限位：{oct(permission_mode)}"
        )
    digest = hashlib.sha256()
    os.lseek(file_descriptor, 0, os.SEEK_SET)
    while chunk := os.read(file_descriptor, 1024 * 1024):
        digest.update(chunk)
    os.lseek(file_descriptor, 0, os.SEEK_SET)
    return {
        "sha256": digest.hexdigest(),
        "mode": permission_mode,
    }


def snapshot_file(path: Path) -> Snapshot:
    """在不追随最终符号链接的前提下读取普通文件快照。"""

    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        file_descriptor = os.open(path, flags)
    except OSError as exc:
        raise UpgradeError(f"无法打开普通文件 {path}：{exc}") from exc
    try:
        return snapshot_fd(file_descriptor)
    finally:
        os.close(file_descriptor)


def validate_snapshot(value: Any, label: str) -> Snapshot | None:
    """严格校验 plan/lock 中的可选文件快照。"""

    if value is None:
        return None
    if not isinstance(value, dict) or set(value) != {"sha256", "mode"}:
        raise UpgradeError(f"{label}的快照非法")
    digest = value.get("sha256")
    mode = value.get("mode")
    if (
        not isinstance(digest, str)
        or len(digest) != 64
        or any(character not in "0123456789abcdef" for character in digest)
        or not isinstance(mode, int)
        or mode < 0
        or mode > 0o777
    ):
        raise UpgradeError(f"{label}的快照非法")
    return {"sha256": digest, "mode": mode}


def require_git_root(root: Path, label: str = "目标") -> dict[str, str | int | bool]:
    """确认独立且已有提交的 Git 根，并绑定分支、HEAD 与工作区状态。"""

    def git_output(*arguments: str, allow_failure: bool = False) -> str:
        result = subprocess.run(
            ["git", "-C", str(root), *arguments],
            check=False,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0 and not allow_failure:
            raise UpgradeError(
                f"{label} Git 命令失败（{' '.join(arguments)}）："
                f"{result.stderr.strip()}"
            )
        return result.stdout.strip() if result.returncode == 0 else "unborn"

    observed_root = Path(git_output("rev-parse", "--show-toplevel")).resolve(strict=True)
    if observed_root != root:
        raise UpgradeError(
            f"{label} Git 顶层目录不匹配：预期 {root}，实际 {observed_root}"
        )
    common_raw = Path(git_output("rev-parse", "--git-common-dir"))
    common = (
        common_raw
        if common_raw.is_absolute()
        else lexical_absolute(root / common_raw)
    ).resolve(strict=True)
    common_stat = common.stat()
    head = git_output("rev-parse", "--verify", "HEAD", allow_failure=True)
    if head == "unborn":
        raise UpgradeError(f"{label} Git 仓库必须已有 HEAD 提交")
    status_result = subprocess.run(
        [
            "git",
            "-C",
            str(root),
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
            "-z",
        ],
        check=False,
        capture_output=True,
    )
    if status_result.returncode != 0:
        raise UpgradeError(
            f"{label} Git 状态检查失败："
            f"{status_result.stderr.decode('utf-8', errors='replace').strip()}"
        )
    status_digest = hashlib.sha256(status_result.stdout).hexdigest()
    return {
        "top_level": str(root),
        "common_dir": str(common),
        "common_device": common_stat.st_dev,
        "common_inode": common_stat.st_ino,
        "head": head,
        "branch": git_output("branch", "--show-current"),
        "dirty": bool(status_result.stdout),
        "status_sha256": status_digest,
    }


def require_source_identity(
    source_root: Path,
    expected_version: str,
    expected_commit: str,
) -> tuple[Path, dict[str, str | int | bool]]:
    """验证来源 Harness 根、已提交版本和干净工作区，并返回可审计身份。"""

    source = canonical_directory(source_root, "源 Harness 根目录")
    identity = require_git_root(source, "源 Harness")
    if identity["dirty"]:
        raise UpgradeError("源 Harness Git 工作树必须保持干净")
    if expected_commit != identity["head"]:
        raise UpgradeError(
            "源 commit 与源 Harness HEAD 不匹配："
            f"预期 {expected_commit}，实际 {identity['head']}"
        )
    version_file = source / "Version.md"
    assert_safe_path(
        source,
        version_file,
        "源 Harness Version.md",
        final_may_be_missing=False,
    )
    if version_file.is_symlink() or not version_file.is_file():
        raise UpgradeError("源 Harness Version.md 必须是普通文件")
    match = VERSION_PATTERN.search(version_file.read_text(encoding="utf-8"))
    if not match:
        raise UpgradeError("源 Harness Version.md 没有当前版本")
    observed_version = match.group(1)
    if expected_version != observed_version:
        raise UpgradeError(
            "源版本与源 Harness Version.md 不匹配："
            f"预期 {expected_version}，实际 {observed_version}"
        )
    return source, identity


def require_control_paths(
    target: Path,
    ownership_path: Path,
    lock_path: Path,
) -> tuple[Path, Path]:
    """把所有权清单和来源锁固定到下游内的唯一维护位置。"""

    expected_ownership = target / OWNERSHIP_RELATIVE
    try:
        observed_ownership = ownership_path.resolve(strict=True)
    except OSError as exc:
        raise UpgradeError(
            f"无法解析所有权 manifest {ownership_path}：{exc}"
        ) from exc
    if observed_ownership != expected_ownership:
        raise UpgradeError(
            f"所有权 manifest 必须精确位于 {expected_ownership}"
        )
    assert_safe_path(
        target,
        observed_ownership,
        "所有权 manifest",
        final_may_be_missing=False,
    )
    if not observed_ownership.is_file():
        raise UpgradeError(f"缺少所有权 manifest：{observed_ownership}")

    expected_lock = target / LOCK_RELATIVE
    try:
        observed_lock = lock_path.resolve(strict=False)
    except OSError as exc:
        raise UpgradeError(f"无法解析上游 lock {lock_path}：{exc}") from exc
    if observed_lock != expected_lock:
        raise UpgradeError(f"lock 必须精确位于 {expected_lock}")
    assert_safe_path(
        target,
        observed_lock,
        "上游 lock",
        final_may_be_missing=True,
    )
    return observed_ownership, observed_lock
