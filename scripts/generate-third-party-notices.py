#!/usr/bin/env python3
"""从锁定依赖图生成可随安装包分发的第三方许可清单。"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import tempfile
import tomllib
from pathlib import Path
from typing import Any


class NoticeGenerationError(RuntimeError):
    """表示依赖事实不完整，不能生成可信的第三方许可清单。"""


def parse_args() -> argparse.Namespace:
    """解析项目根、输出文件和只读检查模式。"""

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--project-root", required=True, type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--check", action="store_true")
    return parser.parse_args()


def normalized_project_root(candidate: Path) -> Path:
    """解析并验证独立项目根，避免把生成物写到其他仓库。"""

    root = candidate.resolve(strict=True)
    result = subprocess.run(
        ["git", "-C", str(root), "rev-parse", "--show-toplevel"],
        check=True,
        capture_output=True,
        text=True,
    )
    git_root = Path(result.stdout.strip()).resolve(strict=True)
    if git_root != root:
        raise NoticeGenerationError("project root is not an independent Git root")
    return root


def file_sha256(path: Path) -> str:
    """计算锁文件摘要，使清单能够绑定精确依赖输入。"""

    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run_json(command: list[str], cwd: Path) -> Any:
    """运行项目已声明的依赖工具，并解析其 JSON 输出。"""

    result = subprocess.run(
        command,
        cwd=cwd,
        check=True,
        capture_output=True,
        text=True,
    )
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise NoticeGenerationError(f"invalid JSON from {command[0]}") from error


def clean_cell(value: object) -> str:
    """把外部包元数据规范为单行 Markdown 表格单元格。"""

    text = " ".join(str(value).split())
    return text.replace("|", "\\|")


def rust_dependencies(root: Path) -> list[tuple[str, str, str, str]]:
    """读取 Cargo.lock 对应的完整第三方包图，不包含工作区自有 crate。"""

    metadata = run_json(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--offline"],
        root,
    )
    rows: list[tuple[str, str, str, str]] = []
    for package in metadata.get("packages", []):
        if not package.get("source"):
            continue
        license_name = package.get("license") or package.get("license_file")
        if not license_name:
            raise NoticeGenerationError(
                f"Rust package lacks license metadata: {package.get('name')}"
            )
        source = package.get("repository") or package.get("homepage") or package["source"]
        rows.append(
            (
                clean_cell(package["name"]),
                clean_cell(package["version"]),
                clean_cell(license_name),
                clean_cell(source),
            )
        )
    return sorted(rows, key=lambda row: (row[0].casefold(), row[1]))


def frontend_dependencies(root: Path) -> list[tuple[str, str, str, str]]:
    """读取前端生产依赖的许可报告，排除不会进入安装包的开发依赖。"""

    gui_root = root / "loki_metis_gui"
    report = run_json(["pnpm", "licenses", "list", "--prod", "--json"], gui_root)
    if not isinstance(report, dict):
        raise NoticeGenerationError("pnpm license report must be an object")
    rows: list[tuple[str, str, str, str]] = []
    for license_name, packages in report.items():
        if not isinstance(packages, list):
            raise NoticeGenerationError("pnpm license group must be a list")
        for package in packages:
            versions = package.get("versions") or []
            if not versions:
                raise NoticeGenerationError(
                    f"frontend package lacks a version: {package.get('name')}"
                )
            source = package.get("homepage") or package.get("repository") or "Not provided"
            rows.append(
                (
                    clean_cell(package["name"]),
                    clean_cell(", ".join(sorted(str(version) for version in versions))),
                    clean_cell(license_name),
                    clean_cell(source),
                )
            )
    return sorted(rows, key=lambda row: (row[0].casefold(), row[1]))


def render_table(rows: list[tuple[str, str, str, str]]) -> list[str]:
    """把依赖记录渲染为稳定、便于人工审查的 Markdown 表格。"""

    rendered = [
        "| Package | Version | Declared license | Source |",
        "|---|---:|---|---|",
    ]
    rendered.extend(f"| {name} | {version} | {license_name} | {source} |" for name, version, license_name, source in rows)
    return rendered


def render_notice(root: Path) -> bytes:
    """从版本与两个锁定生态的依赖事实构造确定性 NOTICE。"""

    cargo_lock = root / "Cargo.lock"
    pnpm_lock = root / "loki_metis_gui" / "pnpm-lock.yaml"
    cargo_manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    version = cargo_manifest["workspace"]["package"]["version"]
    rust_rows = rust_dependencies(root)
    frontend_rows = frontend_dependencies(root)
    lines = [
        "# LokiMetis Third-Party Notices",
        "",
        f"This notice covers third-party dependencies resolved for LokiMetis v{version}.",
        "Copyright and license rights remain with their respective authors and projects.",
        "The declared license expression and source link below identify the governing terms for each package.",
        "",
        "锁定输入 / Locked inputs:",
        "",
        f"- `Cargo.lock` SHA-256: `{file_sha256(cargo_lock)}`",
        f"- `loki_metis_gui/pnpm-lock.yaml` SHA-256: `{file_sha256(pnpm_lock)}`",
        "",
        f"## Rust dependencies ({len(rust_rows)})",
        "",
        *render_table(rust_rows),
        "",
        f"## Frontend runtime dependencies ({len(frontend_rows)})",
        "",
        *render_table(frontend_rows),
        "",
    ]
    return "\n".join(lines).encode("utf-8")


def checked_output_path(root: Path, requested: Path | None) -> Path:
    """限制生成物位于当前项目根，且拒绝覆盖符号链接。"""

    output = requested or root / "THIRD_PARTY_NOTICES.md"
    output = output if output.is_absolute() else root / output
    output = output.resolve(strict=False)
    if output.parent != root or output.name != "THIRD_PARTY_NOTICES.md":
        raise NoticeGenerationError("output must be project-root THIRD_PARTY_NOTICES.md")
    if output.is_symlink():
        raise NoticeGenerationError("output must not be a symlink")
    return output


def atomic_write(path: Path, content: bytes) -> None:
    """在项目根同文件系统内原子替换生成物。"""

    descriptor, temporary_name = tempfile.mkstemp(prefix=".third-party-notices.", dir=path.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def main() -> int:
    """生成或只读校验当前版本的第三方许可清单。"""

    args = parse_args()
    root = normalized_project_root(args.project_root)
    output = checked_output_path(root, args.output)
    expected = render_notice(root)
    if args.check:
        if not output.is_file() or output.read_bytes() != expected:
            raise NoticeGenerationError("THIRD_PARTY_NOTICES.md is missing or stale")
        print("third_party_notices.status=valid")
        print(f"third_party_notices.sha256={hashlib.sha256(expected).hexdigest()}")
        return 0
    atomic_write(output, expected)
    print("third_party_notices.status=generated")
    print(f"third_party_notices.sha256={hashlib.sha256(expected).hexdigest()}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (NoticeGenerationError, KeyError, OSError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"third-party notice generation failed: {error}") from error
