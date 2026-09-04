#!/usr/bin/env python3
"""管理下游 Rust 产品的确定性语义化版本状态。"""

from __future__ import annotations

import argparse
import copy
import json
import os
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

STATE_RELATIVE = Path(".harness/version-state.json")
SEMVER_PATTERN = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")
CHANGE_ID_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:/-]{0,127}$")
SOURCE_COMMIT_PATTERN = re.compile(r"^[0-9a-fA-F]{40}$")
VERSION_LINE_PATTERN = re.compile(
    r'^(?P<prefix>\s*version\s*=\s*")(?P<version>[^"]+)(?P<suffix>"\s*(?:#.*)?(?:\r?\n)?)$'
)
KINDS = ("feature", "bug-fix", "major", "maintenance")


class GateError(RuntimeError):
    """表示必须失败关闭的版本契约错误。"""


@dataclass(frozen=True, order=True)
class Version:
    """表示受限在 0..100 的稳定三段语义化版本。"""

    major: int
    minor: int
    patch: int

    @classmethod
    def parse(cls, raw: object) -> "Version":
        if not isinstance(raw, str):
            raise GateError("version value must be a string")
        match = SEMVER_PATTERN.fullmatch(raw)
        if not match:
            raise GateError(
                f"unsupported version {raw!r}; expected stable MAJOR.MINOR.PATCH"
            )
        values = tuple(int(value) for value in match.groups())
        if any(value > 100 for value in values):
            raise GateError(f"version component outside inclusive range 0..100: {raw}")
        return cls(*values)

    def __str__(self) -> str:
        return f"{self.major}.{self.minor}.{self.patch}"


def _require_regular_file(path: Path, label: str) -> None:
    if path.is_symlink() or not path.is_file():
        raise GateError(f"{label} must be a regular non-symlink file: {path}")


def _project_directory(raw: str) -> Path:
    candidate = Path(raw).expanduser()
    if candidate.is_symlink():
        raise GateError(f"project root must not be a symlink: {candidate}")
    root = candidate.resolve()
    if not root.is_dir():
        raise GateError(f"project root is not a directory: {root}")
    return root


def _initialization_root(raw: str) -> Path:
    """解析初始化根目录；init 是唯一允许在独立 Git 建立前运行的命令。"""

    return _project_directory(raw)


def _project_root(raw: str) -> Path:
    root = _project_directory(raw)
    try:
        result = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            cwd=root,
            check=True,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        raise GateError("project root must be inside an independent Git repository") from exc
    git_root = Path(result.stdout.strip()).resolve()
    if git_root != root:
        raise GateError(f"Git top-level {git_root} does not equal project root {root}")
    return root


def _locate_version_line(text: str) -> list[tuple[int, re.Match[str]]]:
    """定位 `[workspace.package]` 小节中匹配版本行的行号与正则匹配对象。"""
    section: str | None = None
    matches: list[tuple[int, re.Match[str]]] = []
    for index, line in enumerate(text.splitlines(keepends=True)):
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            section = stripped[1:-1].strip()
            continue
        if section == "workspace.package":
            match = VERSION_LINE_PATTERN.fullmatch(line)
            if match:
                matches.append((index, match))
    return matches


def _cargo_version(path: Path) -> tuple[Version, str]:
    _require_regular_file(path, "root Cargo.toml")
    text = path.read_text(encoding="utf-8")
    matches = _locate_version_line(text)
    if len(matches) != 1:
        raise GateError(
            "root Cargo.toml must contain exactly one string version in [workspace.package]"
        )
    return Version.parse(matches[0][1].group("version")), text


def _replace_cargo_version(text: str, version: Version) -> str:
    matches = _locate_version_line(text)
    if len(matches) != 1:
        raise GateError("unable to update exactly one [workspace.package].version")
    lines = text.splitlines(keepends=True)
    index, match = matches[0]
    lines[index] = f'{match.group("prefix")}{version}{match.group("suffix")}'
    return "".join(lines)


def _atomic_write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.parent.is_symlink():
        raise GateError(f"refusing symlink state directory: {path.parent}")
    mode = path.stat().st_mode & 0o777 if path.exists() else 0o644
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temp_path = Path(temporary)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temp_path, mode)
        os.replace(temp_path, path)
    finally:
        if temp_path.exists():
            temp_path.unlink()


def _new_state(version: Version) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "cycle_base_version": str(version),
        "target_version": str(version),
        "feature_bump_applied": False,
        "pending_changes": [],
        "applied_bug_ids": [],
        "last_release": None,
    }


def _state_json(state: dict[str, Any]) -> str:
    return json.dumps(state, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def _exact_keys(value: Any, keys: set[str]) -> bool:
    return isinstance(value, dict) and set(value) == keys


def _load_state(path: Path) -> dict[str, Any]:
    _require_regular_file(path, "version state")
    try:
        state = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise GateError(f"invalid version state: {path}") from exc
    required = {
        "schema_version",
        "cycle_base_version",
        "target_version",
        "feature_bump_applied",
        "pending_changes",
        "applied_bug_ids",
        "last_release",
    }
    if not _exact_keys(state, required):
        raise GateError("version state has unexpected schema fields")
    if state["schema_version"] != 1:
        raise GateError("unsupported version state schema")
    base = Version.parse(state["cycle_base_version"])
    target = Version.parse(state["target_version"])
    if target < base:
        raise GateError("target_version cannot be lower than cycle_base_version")
    if not isinstance(state["feature_bump_applied"], bool):
        raise GateError("feature_bump_applied must be boolean")
    if not isinstance(state["pending_changes"], list):
        raise GateError("pending_changes must be an array")
    if not isinstance(state["applied_bug_ids"], list):
        raise GateError("applied_bug_ids must be an array")
    seen: set[str] = set()
    for item in state["pending_changes"]:
        if not _exact_keys(item, {"change_id", "kind", "required_version"}):
            raise GateError("pending change has unexpected fields")
        _validate_change_id(item["change_id"])
        if item["kind"] not in {"feature", "bug-fix", "major"}:
            raise GateError("pending change has unsupported kind")
        required_version = Version.parse(item["required_version"])
        if required_version > target:
            raise GateError("pending required_version cannot exceed target_version")
        if item["change_id"] in seen:
            raise GateError("pending change IDs must be unique")
        seen.add(item["change_id"])
    bug_ids = state["applied_bug_ids"]
    if any(not isinstance(item, str) for item in bug_ids) or len(set(bug_ids)) != len(bug_ids):
        raise GateError("applied_bug_ids must contain unique strings")
    for bug_id in bug_ids:
        _validate_change_id(bug_id)
    last_release = state["last_release"]
    if last_release is not None:
        if not _exact_keys(last_release, {"source_commit", "version"}):
            raise GateError("last_release has unexpected fields")
        released_version = Version.parse(last_release["version"])
        if released_version != base:
            raise GateError("last_release version must equal cycle_base_version")
        if not isinstance(last_release["source_commit"], str) or not SOURCE_COMMIT_PATTERN.fullmatch(last_release["source_commit"]):
            raise GateError("last_release source_commit must be 40 hexadecimal characters")
    return state


def _validate_change_id(change_id: Any) -> str:
    if not isinstance(change_id, str) or not CHANGE_ID_PATTERN.fullmatch(change_id):
        raise GateError(
            "change_id must be 1..128 characters using letters, digits, dot, underscore, colon, slash, or hyphen"
        )
    return change_id


def _consistent_context(root: Path) -> tuple[Path, Path, Version, str, dict[str, Any]]:
    cargo_path = root / "Cargo.toml"
    state_path = root / STATE_RELATIVE
    cargo_version, cargo_text = _cargo_version(cargo_path)
    state = _load_state(state_path)
    target = Version.parse(state["target_version"])
    if cargo_version != target:
        raise GateError(
            f"version drift: Cargo.toml={cargo_version}, state target={target}"
        )
    return cargo_path, state_path, cargo_version, cargo_text, state


def initialize(root: Path) -> dict[str, Any]:
    cargo_path = root / "Cargo.toml"
    state_path = root / STATE_RELATIVE
    if state_path.exists() or state_path.is_symlink():
        _, _, current, _, state = _consistent_context(root)
        return {
            "action": "init",
            "changed": False,
            "current_version": str(current),
            "state": str(STATE_RELATIVE),
            "pending_change_count": len(state["pending_changes"]),
        }
    version, _ = _cargo_version(cargo_path)
    if state_path.parent.exists() and state_path.parent.is_symlink():
        raise GateError(f"refusing symlink state directory: {state_path.parent}")
    state = _new_state(version)
    _atomic_write(state_path, _state_json(state))
    return {
        "action": "init",
        "changed": True,
        "current_version": str(version),
        "state": str(STATE_RELATIVE),
        "pending_change_count": 0,
    }

def _existing_change(state: dict[str, Any], change_id: str) -> dict[str, Any] | None:
    return next(
        (item for item in state["pending_changes"] if item["change_id"] == change_id),
        None,
    )


def _idempotent_result(required_version: str, reason: str) -> dict[str, Any]:
    return {
        "required_version": required_version,
        "version_bumped": False,
        "idempotent": True,
        "reason": reason,
    }


def _transition(
    state: dict[str, Any],
    current: Version,
    *,
    kind: str,
    change_id: str | None,
    major: int | None,
    user_approved: bool,
) -> tuple[Version, dict[str, Any], dict[str, Any]]:
    next_state = copy.deepcopy(state)
    if kind == "maintenance":
        return current, next_state, _idempotent_result(
            str(current), "maintenance-does-not-change-version"
        )
    stable_id = _validate_change_id(change_id)
    existing = _existing_change(state, stable_id)
    if existing:
        if existing["kind"] != kind:
            raise GateError(
                f"change_id {stable_id!r} is already used by kind {existing['kind']!r}"
            )
        return current, next_state, _idempotent_result(
            existing["required_version"], "change-already-applied"
        )

    next_version = current
    reason: str
    if kind == "feature":
        if state["feature_bump_applied"]:
            reason = "feature-bump-already-applied-in-release-cycle"
        else:
            if current.minor >= 100:
                raise GateError("Minor overflow at 100; user decision is required")
            next_version = Version(current.major, current.minor + 1, 0)
            next_state["feature_bump_applied"] = True
            reason = "first-feature-in-release-cycle"
    elif kind == "bug-fix":
        if stable_id in state["applied_bug_ids"]:
            return current, next_state, _idempotent_result(
                str(current), "bug-id-already-consumed"
            )
        if current.patch >= 100:
            raise GateError("Patch overflow at 100; user decision is required")
        next_version = Version(current.major, current.minor, current.patch + 1)
        next_state["applied_bug_ids"].append(stable_id)
        reason = "distinct-completed-bug-fix"
    elif kind == "major":
        if not user_approved:
            raise GateError("Major change requires explicit --user-approved")
        if major is None or not 0 <= major <= 100:
            raise GateError("approved Major must be inside inclusive range 0..100")
        if major <= current.major:
            raise GateError("approved Major must be greater than the current Major")
        next_version = Version(major, 0, 0)
        next_state["feature_bump_applied"] = True
        reason = "explicit-user-approved-major"
    else:
        raise GateError(f"unsupported change kind: {kind}")

    next_state["target_version"] = str(next_version)
    next_state["pending_changes"].append(
        {
            "change_id": stable_id,
            "kind": kind,
            "required_version": str(next_version),
        }
    )
    return next_version, next_state, {
        "required_version": str(next_version),
        "version_bumped": next_version != current,
        "idempotent": False,
        "reason": reason,
    }


def evaluate_change(
    root: Path,
    *,
    action: str,
    kind: str,
    change_id: str | None,
    major: int | None,
    user_approved: bool,
) -> dict[str, Any]:
    cargo_path, state_path, current, cargo_text, state = _consistent_context(root)
    next_version, next_state, result = _transition(
        state,
        current,
        kind=kind,
        change_id=change_id,
        major=major,
        user_approved=user_approved,
    )
    changed = next_state != state or next_version != current
    if action == "apply" and changed:
        old_cargo = cargo_text
        new_cargo = _replace_cargo_version(cargo_text, next_version)
        cargo_changed = new_cargo != old_cargo
        try:
            if cargo_changed:
                _atomic_write(cargo_path, new_cargo)
            _atomic_write(state_path, _state_json(next_state))
        except Exception:
            if cargo_changed:
                _atomic_write(cargo_path, old_cargo)
            raise
    return {
        "action": action,
        "kind": kind,
        "change_id": change_id,
        "before_version": str(current),
        "after_version": str(next_version),
        "changed": bool(action == "apply" and changed),
        "pending_change_count": len(next_state["pending_changes"]),
        **result,
    }

def check(root: Path, phase: str) -> dict[str, Any]:
    _, _, current, _, state = _consistent_context(root)
    return {
        "action": "check",
        "phase": phase,
        "current_version": str(current),
        "cycle_base_version": state["cycle_base_version"],
        "feature_bump_applied": state["feature_bump_applied"],
        "pending_change_count": len(state["pending_changes"]),
        "passed": True,
    }


def finalize_release(
    root: Path, released_version: str, source_commit: str
) -> dict[str, Any]:
    _, state_path, current, _, state = _consistent_context(root)
    released = Version.parse(released_version)
    if released != current:
        raise GateError(
            f"released version {released} does not equal current target {current}"
        )
    if not SOURCE_COMMIT_PATTERN.fullmatch(source_commit):
        raise GateError("source_commit must be exactly 40 hexadecimal characters")
    next_state = copy.deepcopy(state)
    next_state["cycle_base_version"] = str(released)
    next_state["feature_bump_applied"] = False
    next_state["pending_changes"] = []
    next_state["last_release"] = {
        "source_commit": source_commit.lower(),
        "version": str(released),
    }
    _atomic_write(state_path, _state_json(next_state))
    return {
        "action": "finalize-release",
        "changed": next_state != state,
        "released_version": str(released),
        "source_commit": source_commit.lower(),
        "retained_bug_id_count": len(next_state["applied_bug_ids"]),
        "pending_change_count": 0,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    init_parser = subparsers.add_parser("init")
    init_parser.add_argument("--project-root", required=True)
    for command in ("plan", "apply"):
        change_parser = subparsers.add_parser(command)
        change_parser.add_argument("--project-root", required=True)
        change_parser.add_argument("--kind", choices=KINDS, required=True)
        change_parser.add_argument("--change-id")
        change_parser.add_argument("--major", type=int)
        change_parser.add_argument("--user-approved", action="store_true")
    check_parser = subparsers.add_parser("check")
    check_parser.add_argument("--project-root", required=True)
    check_parser.add_argument(
        "--phase", choices=("development", "build", "release"), required=True
    )
    release_parser = subparsers.add_parser("finalize-release")
    release_parser.add_argument("--project-root", required=True)
    release_parser.add_argument("--released-version", required=True)
    release_parser.add_argument("--source-commit", required=True)
    release_parser.add_argument("--release-succeeded", action="store_true", required=True)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        root = (
            _initialization_root(args.project_root)
            if args.command == "init"
            else _project_root(args.project_root)
        )
        if args.command == "init":
            result = initialize(root)
        elif args.command in {"plan", "apply"}:
            result = evaluate_change(
                root,
                action=args.command,
                kind=args.kind,
                change_id=args.change_id,
                major=args.major,
                user_approved=args.user_approved,
            )
        elif args.command == "check":
            result = check(root, args.phase)
        else:
            result = finalize_release(root, args.released_version, args.source_commit)
    except (GateError, OSError) as exc:
        print(json.dumps({"error": str(exc)}, ensure_ascii=False), file=sys.stderr)
        return 2
    print(json.dumps(result, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
