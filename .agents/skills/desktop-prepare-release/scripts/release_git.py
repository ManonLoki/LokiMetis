#!/usr/bin/env python3
"""检查发布工作树并把 Agent 已复核的精确路径提交到本地 Git。"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from typing import Sequence


HEAD_PATTERN = re.compile(r"^[0-9a-f]{40}$")
SECRET_PATTERNS = (
    re.compile(br"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----"),
    re.compile(br"(?:^|[^A-Za-z0-9])AKIA[0-9A-Z]{16}(?:$|[^A-Za-z0-9])"),
    re.compile(br"(?:^|[^A-Za-z0-9])gh[pousr]_[A-Za-z0-9]{20,}(?:$|[^A-Za-z0-9])"),
    re.compile(br"(?:^|[^A-Za-z0-9])sk-[A-Za-z0-9_-]{20,}(?:$|[^A-Za-z0-9_-])"),
)
CACHED_PATCH_ARGUMENTS = (
    "diff",
    "--cached",
    "--binary",
    "--full-index",
    "--no-renames",
    "--no-ext-diff",
    "--no-textconv",
)


class ReleaseGitError(RuntimeError):
    """表示发布提交的仓库边界、快照或 Git 操作不安全。"""


def run_git(
    root: Path,
    arguments: Sequence[str],
    *,
    check: bool = True,
    text: bool = True,
    environment: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str] | subprocess.CompletedProcess[bytes]:
    """在精确项目根运行 Git；从不经 shell，也不绕过 hooks。"""

    try:
        command_environment = {
            **os.environ,
            "GIT_TERMINAL_PROMPT": "0",
            "GCM_INTERACTIVE": "Never",
        }
        if environment:
            command_environment.update(environment)
        result = subprocess.run(
            ["git", "-C", str(root), *arguments],
            check=False,
            capture_output=True,
            text=text,
            encoding="utf-8" if text else None,
            env=command_environment,
        )
    except FileNotFoundError as error:
        raise ReleaseGitError("git executable is unavailable") from error
    if check and result.returncode != 0:
        stderr = result.stderr.decode("utf-8", "replace") if not text else result.stderr
        stdout = result.stdout.decode("utf-8", "replace") if not text else result.stdout
        detail = stderr.strip() or stdout.strip() or "unknown git failure"
        raise ReleaseGitError(f"git {' '.join(arguments)} failed: {detail}")
    return result


def resolve_repository(project_root: str) -> tuple[Path, str, str]:
    """要求独立非符号链接 Git 顶层、具名分支与可解析的 40 位 HEAD。"""

    supplied = Path(project_root).expanduser()
    if supplied.is_symlink():
        raise ReleaseGitError("project root must not be a symbolic link")
    try:
        root = supplied.resolve(strict=True)
    except FileNotFoundError as error:
        raise ReleaseGitError("project root does not exist") from error
    if not root.is_dir():
        raise ReleaseGitError("project root is not a directory")
    query = run_git(
        root,
        ["rev-parse", "--is-inside-work-tree", "--show-toplevel", "HEAD"],
    )
    values = query.stdout.splitlines()
    if len(values) != 3 or values[0] != "true":
        raise ReleaseGitError("project root is not a Git work tree with HEAD")
    if Path(values[1]).resolve(strict=True) != root:
        raise ReleaseGitError("project root must equal the independent Git top level")
    head = values[2]
    if not HEAD_PATTERN.fullmatch(head):
        raise ReleaseGitError("HEAD must resolve to a 40-character lowercase commit")
    branch_result = run_git(root, ["symbolic-ref", "--quiet", "--short", "HEAD"], check=False)
    branch = branch_result.stdout.strip()
    if branch_result.returncode != 0 or not branch:
        raise ReleaseGitError("HEAD must be attached to a named branch")
    return root, head, branch


def status_bytes(root: Path) -> bytes:
    """读取完整工作树状态，包含 ignored 规则外的全部未跟踪文件。"""

    return run_git(
        root,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        text=False,
    ).stdout


def repository_snapshot_digest(
    root: Path,
    status_value: bytes,
    *,
    head: str,
    branch: str,
) -> str:
    """摘要分支、HEAD、状态、diff、index 与未跟踪字节，阻断切换竞态。"""

    digest = hashlib.sha256()
    digest.update(b"branch\0")
    digest.update(branch.encode("utf-8", "surrogateescape"))
    digest.update(b"\0head\0")
    digest.update(head.encode("ascii"))
    digest.update(b"\0")
    digest.update(b"status\0")
    digest.update(status_value)
    for label, arguments in (
        (b"worktree\0", ["diff", "--binary", "--no-ext-diff", "--no-textconv"]),
        (b"index\0", ["diff", "--cached", "--binary", "--no-ext-diff", "--no-textconv"]),
    ):
        digest.update(label)
        digest.update(run_git(root, arguments, text=False).stdout)
    for relative in nul_paths(root, ["ls-files", "--others", "--exclude-standard", "-z"]):
        normalized = normalize_approved_path(relative)
        candidate = root.joinpath(*PurePosixPath(normalized).parts)
        metadata = candidate.lstat()
        digest.update(b"untracked\0")
        digest.update(normalized.encode("utf-8", "surrogateescape"))
        digest.update(b"\0")
        if stat.S_ISLNK(metadata.st_mode):
            digest.update(b"symlink\0")
            digest.update(os.readlink(candidate).encode("utf-8", "surrogateescape"))
        elif stat.S_ISREG(metadata.st_mode):
            digest.update(b"file\0")
            with candidate.open("rb") as handle:
                while block := handle.read(1024 * 1024):
                    digest.update(block)
        else:
            raise ReleaseGitError(f"untracked path is not a regular file or symlink: {relative!r}")
    return digest.hexdigest()


def decode_status_records(value: bytes) -> list[str]:
    """仅用于 JSON 报告显示 NUL 分隔状态记录，不把显示值重新解释为 pathspec。"""

    return [item.decode("utf-8", "surrogateescape") for item in value.split(b"\0") if item]


def inspect_repository(project_root: str) -> dict[str, object]:
    """返回当前 HEAD、clean 结论与快照摘要，供 Agent 逐项复核真实 diff。"""

    root, head, branch = resolve_repository(project_root)
    status = status_bytes(root)
    return {
        "status": "clean" if not status else "dirty",
        "projectRoot": str(root),
        "branch": branch,
        "head": head,
        "statusSha256": repository_snapshot_digest(
            root,
            status,
            head=head,
            branch=branch,
        ),
        "records": decode_status_records(status),
    }


def normalize_approved_path(value: str) -> str:
    """拒绝绝对路径、Git pathspec magic、元数据和候选产物目录。"""

    if not value or "\0" in value or "\\" in value or value.startswith(":"):
        raise ReleaseGitError(f"unsafe approved path: {value!r}")
    path = PurePosixPath(value)
    if path.is_absolute() or path == PurePosixPath(".") or any(
        part in ("", ".", "..") for part in path.parts
    ):
        raise ReleaseGitError(f"unsafe approved path: {value!r}")
    if path.parts[0] == ".git" or path.parts[0] == "release" or path.parts[0].startswith(
        ".release-clean."
    ):
        raise ReleaseGitError(f"release metadata or Git internals cannot be approved: {value!r}")
    return path.as_posix()


def path_is_approved(changed: str, approved: Sequence[str]) -> bool:
    """允许精确文件或已批准目录的后代，但不允许相似前缀。"""

    return any(changed == item or changed.startswith(f"{item}/") for item in approved)


def nul_paths(root: Path, arguments: Sequence[str]) -> list[str]:
    """读取 Git NUL 分隔路径输出，避免空格和引号歧义。"""

    output = run_git(root, arguments, text=False).stdout
    return [item.decode("utf-8", "surrogateescape") for item in output.split(b"\0") if item]


def staged_diff_contains_secret(root: Path) -> bool:
    """只匹配高置信度私钥/令牌形态；不把匹配内容写入错误或日志。"""

    patch = run_git(
        root,
        ["diff", "--cached", "--binary", "--no-ext-diff", "--no-textconv", "--unified=0"],
        text=False,
    ).stdout
    return any(pattern.search(patch) for pattern in SECRET_PATTERNS)


def cached_index_patch(
    root: Path, *, environment: dict[str, str] | None = None
) -> bytes:
    """返回包含新增、删除、mode 与二进制字节的规范化完整 index patch。"""

    return run_git(
        root,
        list(CACHED_PATCH_ARGUMENTS),
        text=False,
        environment=environment,
    ).stdout


def freeze_expected_index_patch(root: Path, literal_pathspecs: Sequence[str]) -> bytes:
    """在隔离 index 中冻结当前快照执行同一 add 后应得到的精确 patch。"""

    git_directory_result = run_git(root, ["rev-parse", "--git-dir"])
    git_directory_lines = git_directory_result.stdout.splitlines()
    if len(git_directory_lines) != 1:
        raise ReleaseGitError("cannot resolve the repository Git directory")
    git_directory = Path(git_directory_lines[0])
    if not git_directory.is_absolute():
        git_directory = root / git_directory
    index_path = git_directory / "index"
    if index_path.is_symlink() or not index_path.is_file():
        raise ReleaseGitError("repository index must be a regular file")
    index_parent = index_path.parent
    if index_parent.is_symlink() or not index_parent.is_dir():
        raise ReleaseGitError("repository index parent must be a regular directory")

    with tempfile.TemporaryDirectory(
        prefix=".release-reviewed-index-", dir=index_parent
    ) as temporary_directory:
        temporary_index = Path(temporary_directory) / "index"
        shutil.copyfile(index_path, temporary_index)
        temporary_environment = {"GIT_INDEX_FILE": str(temporary_index)}
        run_git(
            root,
            ["add", "-A", "--", *literal_pathspecs],
            environment=temporary_environment,
        )
        return cached_index_patch(root, environment=temporary_environment)


def commit_approved(
    project_root: str,
    *,
    expected_status_sha256: str,
    message: str,
    paths: Sequence[str],
) -> dict[str, object]:
    """在快照未变化时暂存精确路径、运行正常 hooks 提交，并要求最终 clean。"""

    root, previous_head, branch = resolve_repository(project_root)
    if not re.fullmatch(r"[0-9a-f]{64}", expected_status_sha256):
        raise ReleaseGitError("expected status SHA-256 must be 64 lowercase hexadecimal characters")
    if not message.strip() or "\0" in message:
        raise ReleaseGitError("commit message must be non-empty and contain no NUL")
    approved = tuple(dict.fromkeys(normalize_approved_path(value) for value in paths))
    if not approved:
        raise ReleaseGitError("at least one reviewed path is required")

    before = status_bytes(root)
    if repository_snapshot_digest(
        root,
        before,
        head=previous_head,
        branch=branch,
    ) != expected_status_sha256:
        raise ReleaseGitError("working tree changed after review; inspect it again before committing")
    if not before:
        raise ReleaseGitError("working tree is clean; refusing an empty release commit")

    staged_before = nul_paths(root, ["diff", "--cached", "--name-only", "-z"])
    unapproved_staged = [path for path in staged_before if not path_is_approved(path, approved)]
    if unapproved_staged:
        raise ReleaseGitError(
            "index contains staged paths outside the reviewed scope: " + ", ".join(unapproved_staged)
        )

    literal_pathspecs = [f":(literal){path}" for path in approved]
    expected_index_patch = freeze_expected_index_patch(root, literal_pathspecs)
    frozen_status = status_bytes(root)
    _, frozen_head, frozen_branch = resolve_repository(str(root))
    if (
        frozen_head != previous_head
        or frozen_branch != branch
        or repository_snapshot_digest(
            root,
            frozen_status,
            head=frozen_head,
            branch=frozen_branch,
        )
        != expected_status_sha256
    ):
        raise ReleaseGitError(
            "working tree, branch, HEAD, or index changed while freezing the reviewed content"
        )

    run_git(root, ["add", "-A", "--", *literal_pathspecs])
    reviewed_index_patch = cached_index_patch(root)
    if reviewed_index_patch != expected_index_patch:
        raise ReleaseGitError(
            "staged content changed after review; inspect it again before committing"
        )
    staged = nul_paths(root, ["diff", "--cached", "--name-only", "-z"])
    if not staged:
        raise ReleaseGitError("reviewed paths produced no staged changes")
    unapproved = [path for path in staged if not path_is_approved(path, approved)]
    if unapproved:
        raise ReleaseGitError(
            "staged paths escaped the reviewed scope: " + ", ".join(unapproved)
        )
    if staged_diff_contains_secret(root):
        raise ReleaseGitError(
            "potential secret detected in reviewed staged bytes; remove it and inspect again"
        )

    unstaged = run_git(root, ["diff", "--quiet"], check=False)
    if unstaged.returncode not in (0, 1):
        raise ReleaseGitError("cannot verify unstaged tracked changes")
    untracked = nul_paths(root, ["ls-files", "--others", "--exclude-standard", "-z"])
    if unstaged.returncode == 1 or untracked:
        raise ReleaseGitError(
            "reviewed paths do not cover the complete working tree; refusing a partial release commit"
        )
    _, commit_parent, commit_branch = resolve_repository(str(root))
    if commit_parent != previous_head or commit_branch != branch:
        raise ReleaseGitError("branch or HEAD changed before the reviewed commit")

    # 不传 --no-verify；任一 pre-commit/commit-msg hook、签名或提交失败都会直接阻断。
    commit_result = run_git(root, ["commit", "-m", message], check=False)
    if commit_result.returncode != 0:
        raise ReleaseGitError("git commit failed; hooks were not bypassed")

    final_status = status_bytes(root)
    if final_status:
        raise ReleaseGitError("commit succeeded but working tree is not clean; release must stop")
    _, head, final_branch = resolve_repository(str(root))
    if head == previous_head:
        raise ReleaseGitError("git commit did not advance HEAD")
    if final_branch != branch:
        raise ReleaseGitError("git commit changed the reviewed branch unexpectedly")
    parents = run_git(root, ["rev-list", "--parents", "-n", "1", head]).stdout.split()
    if parents != [head, previous_head]:
        raise ReleaseGitError("reviewed commit is not the direct non-merge child of the reviewed HEAD")
    committed_patch = run_git(
        root,
        [
            "diff",
            "--binary",
            "--full-index",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            previous_head,
            head,
        ],
        text=False,
    ).stdout
    if committed_patch != reviewed_index_patch:
        raise ReleaseGitError("a commit hook changed the reviewed staged content; release must stop")
    return {
        "status": "committed",
        "projectRoot": str(root),
        "previousHead": previous_head,
        "head": head,
        "paths": list(approved),
        "clean": True,
    }


def build_parser() -> argparse.ArgumentParser:
    """建立只读 inspect 与显式 reviewed-path commit 两个入口。"""

    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    inspect = subparsers.add_parser("inspect", help="report HEAD and complete worktree status")
    inspect.add_argument("--project-root", required=True)
    commit = subparsers.add_parser("commit", help="commit the exact Agent-reviewed paths locally")
    commit.add_argument("--project-root", required=True)
    commit.add_argument("--expected-status-sha256", required=True)
    commit.add_argument("--message", required=True)
    commit.add_argument("--path", action="append", required=True)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    """输出稳定 JSON；所有歧义或 Git 失败均以非零状态停止。"""

    arguments = build_parser().parse_args(argv)
    try:
        if arguments.command == "inspect":
            result = inspect_repository(arguments.project_root)
        else:
            result = commit_approved(
                arguments.project_root,
                expected_status_sha256=arguments.expected_status_sha256,
                message=arguments.message,
                paths=arguments.path,
            )
    except (OSError, ReleaseGitError) as error:
        print(json.dumps({"status": "error", "error": str(error)}, ensure_ascii=False), file=sys.stderr)
        return 1
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
