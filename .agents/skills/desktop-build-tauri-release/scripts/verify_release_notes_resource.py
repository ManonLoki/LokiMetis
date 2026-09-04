#!/usr/bin/env python3
"""验证 Tauri 发布配置和候选内更新日志与根事实逐字节一致。"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path
from typing import Any, Sequence


RELEASE_CONFIG = Path("src-tauri/tauri.release.conf.json")
ROOT_CARGO_SOURCE_MAPPING = "../release-notes.json"
CONVENTIONAL_CARGO_SOURCE_MAPPING = "../../release-notes.json"
RESOURCE_TARGET = "release-notes.json"


class ResourceVerificationError(ValueError):
    """表示发布配置或候选资源未满足确定性嵌入契约。"""


def _require_regular_file(path: Path, label: str) -> None:
    """拒绝缺失、符号链接和非普通文件。"""

    if path.is_symlink() or not path.is_file():
        raise ResourceVerificationError(f"{label} must be a regular non-symlink file")


def _require_directory(path: Path, label: str) -> None:
    """拒绝符号链接目录和不存在的工作根。"""

    if path.is_symlink() or not path.is_dir():
        raise ResourceVerificationError(f"{label} must be a regular directory")


def _sha256(data: bytes) -> str:
    """计算稳定的小写 SHA-256。"""

    return hashlib.sha256(data).hexdigest()


def _load_json_object(path: Path) -> dict[str, Any]:
    """读取严格 UTF-8 JSON 对象。"""

    _require_regular_file(path, "release config")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise ResourceVerificationError("release config must be valid UTF-8 JSON") from error
    if not isinstance(value, dict):
        raise ResourceVerificationError("release config root must be an object")
    return value


def _cargo_root_and_source_mapping(gui_root: Path) -> tuple[Path, str]:
    """按真实 Cargo manifest 根返回 Tauri 解析资源时使用的固定相对路径。"""

    root_manifest = gui_root / "Cargo.toml"
    conventional_manifest = gui_root / "src-tauri" / "Cargo.toml"
    root_exists = root_manifest.is_file() and not root_manifest.is_symlink()
    conventional_exists = (
        conventional_manifest.is_file() and not conventional_manifest.is_symlink()
    )
    if root_exists == conventional_exists:
        raise ResourceVerificationError(
            "GUI root must contain exactly one supported Cargo manifest location"
        )
    if root_exists:
        return gui_root, ROOT_CARGO_SOURCE_MAPPING
    return conventional_manifest.parent, CONVENTIONAL_CARGO_SOURCE_MAPPING


def verify_config(project_root: Path, gui_root: Path) -> str:
    """确认发布专用合并配置把根日志映射到固定资源逻辑路径。"""

    _require_directory(project_root, "project root")
    _require_directory(gui_root, "GUI root")
    canonical_project = project_root.resolve()
    canonical_gui = gui_root.resolve()
    try:
        canonical_gui.relative_to(canonical_project)
    except ValueError as error:
        raise ResourceVerificationError("GUI root must stay inside project root") from error

    config_path = canonical_gui / RELEASE_CONFIG
    config = _load_json_object(config_path)
    cargo_root, source_mapping = _cargo_root_and_source_mapping(canonical_gui)
    bundle = config.get("bundle")
    resources = bundle.get("resources") if isinstance(bundle, dict) else None
    if not isinstance(resources, dict) or resources != {
        source_mapping: RESOURCE_TARGET
    }:
        raise ResourceVerificationError(
            "release config resources must contain only the fixed release-notes mapping"
        )

    source = Path(os.path.abspath(cargo_root / source_mapping))
    expected_source = canonical_project / RESOURCE_TARGET
    if source != expected_source:
        raise ResourceVerificationError("release config source must resolve to project release-notes.json")
    _require_regular_file(source, "source release notes")
    return _sha256(source.read_bytes())


def verify_bytes(source: Path, bundled: Path) -> str:
    """比较根更新日志与已构建资源的完整字节。"""

    _require_regular_file(source, "source release notes")
    _require_regular_file(bundled, "bundled release notes")
    source_bytes = source.read_bytes()
    bundled_bytes = bundled.read_bytes()
    if source_bytes != bundled_bytes:
        raise ResourceVerificationError("bundled release notes bytes do not match the source")
    return _sha256(source_bytes)


def _build_parser() -> argparse.ArgumentParser:
    """建立配置与候选字节两个只读子命令。"""

    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    config = subparsers.add_parser("config")
    config.add_argument("--project-root", required=True, type=Path)
    config.add_argument("--gui-root", required=True, type=Path)

    byte_check = subparsers.add_parser("bytes")
    byte_check.add_argument("--source", required=True, type=Path)
    byte_check.add_argument("--bundled", required=True, type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    """执行只读门禁并输出稳定摘要，不泄露文件正文。"""

    args = _build_parser().parse_args(argv)
    try:
        if args.command == "config":
            digest = verify_config(args.project_root, args.gui_root)
            check = "config"
        else:
            digest = verify_bytes(args.source, args.bundled)
            check = "bytes"
        print(
            f"release-notes.resource.valid=true check={check} "
            f"sha256={digest} path={RESOURCE_TARGET}"
        )
        return 0
    except (OSError, ResourceVerificationError) as error:
        print(f"release-notes.resource.error={error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
