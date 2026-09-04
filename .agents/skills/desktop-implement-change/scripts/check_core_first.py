#!/usr/bin/env python3
"""检查 Rust workspace 的 core-first 确定性依赖边界。"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any

ADAPTER_SUFFIXES = ("_gui",)
INTERFACE_CRATE_NAMES = frozenset(
    {
        "muda",
        "tauri",
        "tauri-build",
        "tray-icon",
        "winit",
    }
)
INTERFACE_CRATE_PREFIXES = ("tauri-plugin-",)


def _workspace_packages(metadata: dict[str, Any]) -> tuple[list[dict[str, Any]], list[str]]:
    """从 Cargo metadata 中提取当前 workspace 成员，并报告结构错误。"""

    packages = metadata.get("packages")
    workspace_members = metadata.get("workspace_members")
    if not isinstance(packages, list):
        return [], ["cargo metadata 缺少 packages 数组"]
    if not isinstance(workspace_members, list) or not workspace_members:
        return [], ["cargo metadata 缺少非空 workspace_members 数组"]

    packages_by_id: dict[str, dict[str, Any]] = {}
    errors: list[str] = []
    for package in packages:
        if not isinstance(package, dict):
            errors.append("cargo metadata packages 包含非对象成员")
            continue
        package_id = package.get("id")
        if not isinstance(package_id, str) or not package_id:
            errors.append("cargo metadata package 缺少稳定 id")
            continue
        packages_by_id[package_id] = package

    members: list[dict[str, Any]] = []
    for member_id in workspace_members:
        if not isinstance(member_id, str) or member_id not in packages_by_id:
            errors.append(f"workspace member 未出现在 packages 中: {member_id!r}")
            continue
        members.append(packages_by_id[member_id])
    return sorted(members, key=lambda package: str(package.get("name", ""))), errors


def _dependencies(package: dict[str, Any]) -> list[dict[str, Any]]:
    """返回按稳定字段排序的 Cargo 依赖声明。"""

    dependencies = package.get("dependencies")
    if not isinstance(dependencies, list):
        return []
    valid = [dependency for dependency in dependencies if isinstance(dependency, dict)]
    return sorted(
        valid,
        key=lambda dependency: (
            str(dependency.get("name", "")),
            str(dependency.get("kind") or ""),
            str(dependency.get("target") or ""),
            str(dependency.get("rename") or ""),
        ),
    )


def _is_interface_crate(package_name: str) -> bool:
    """判断一个精确依赖名是否属于已知接口框架。"""

    return package_name in INTERFACE_CRATE_NAMES or package_name.startswith(
        INTERFACE_CRATE_PREFIXES
    )


def _dependency_scope(dependency: dict[str, Any]) -> str:
    """把依赖种类和目标整理为稳定诊断文本。"""

    kind = dependency.get("kind") or "normal"
    target = dependency.get("target") or "all-targets"
    alias = dependency.get("rename")
    alias_text = f", alias={alias}" if alias else ""
    return f"kind={kind}, target={target}{alias_text}"


def _has_library_target(package: dict[str, Any]) -> bool:
    """确认核心 package 至少公开一个普通 Rust library target。"""

    targets = package.get("targets")
    if not isinstance(targets, list):
        return False
    for target in targets:
        if not isinstance(target, dict):
            continue
        kinds = target.get("kind")
        if isinstance(kinds, list) and "lib" in kinds:
            return True
    return False


def _package_directory(package: dict[str, Any]) -> Path | None:
    """返回 workspace package 的规范化目录；缺失事实时拒绝猜测。"""

    manifest_path = package.get("manifest_path")
    if not isinstance(manifest_path, str) or not manifest_path:
        return None
    return Path(manifest_path).resolve(strict=False).parent


def _points_to_workspace_package(
    dependency: dict[str, Any],
    package: dict[str, Any],
) -> bool:
    """确认依赖确实指向给定 workspace package，而非 registry 同名 crate。"""

    if dependency.get("source") is not None:
        return False
    dependency_path = dependency.get("path")
    package_directory = _package_directory(package)
    if (
        not isinstance(dependency_path, str)
        or not dependency_path
        or package_directory is None
    ):
        return False
    return Path(dependency_path).resolve(strict=False) == package_directory


def _local_dependency_graph(
    packages_by_name: dict[str, dict[str, Any]],
) -> dict[str, tuple[str, ...]]:
    """建立只包含真实 workspace path 依赖的稳定有向图。"""

    graph: dict[str, tuple[str, ...]] = {}
    for package_name, package in packages_by_name.items():
        targets = {
            dependency_name
            for dependency in _dependencies(package)
            if isinstance((dependency_name := dependency.get("name")), str)
            and dependency_name in packages_by_name
            and _points_to_workspace_package(dependency, packages_by_name[dependency_name])
        }
        graph[package_name] = tuple(sorted(targets))
    return graph


def _reachable_paths(
    start: str,
    graph: dict[str, tuple[str, ...]],
) -> dict[str, tuple[str, ...]]:
    """以最短稳定路径枚举一个 workspace package 可达的本地 package。"""

    paths = {start: (start,)}
    pending = [start]
    for package_name in pending:
        for target in graph.get(package_name, ()):
            if target not in paths:
                paths[target] = (*paths[package_name], target)
                pending.append(target)
    return paths


def validate_metadata(metadata: dict[str, Any]) -> list[str]:
    """验证单 core、adapter 直连和禁止反向/横向依赖等硬边界。"""

    if not isinstance(metadata, dict):
        return ["cargo metadata 根必须是 JSON 对象"]

    packages, errors = _workspace_packages(metadata)
    names = [package.get("name") for package in packages]
    if any(not isinstance(name, str) or not name for name in names):
        errors.append("workspace package 缺少非空 name")
        return sorted(set(errors))
    package_names = [str(name) for name in names]
    if len(package_names) != len(set(package_names)):
        errors.append("workspace package name 必须唯一")

    core_packages = [
        package for package in packages if str(package.get("name", "")).endswith("_core")
    ]
    if len(core_packages) != 1:
        errors.append(
            "workspace 必须且只能包含一个以 _core 结尾的共享核心 package，"
            f"实际为 {len(core_packages)}"
        )
        return sorted(set(errors))

    core_package = core_packages[0]
    core_name = str(core_package["name"])
    project_id = core_name[: -len("_core")]
    if not project_id:
        errors.append("共享核心 package 缺少项目标识前缀")
    if not _has_library_target(core_package):
        errors.append(f"共享核心 {core_name} 必须公开 lib target")
    if _package_directory(core_package) is None:
        errors.append(f"共享核心 {core_name} 缺少可解析的 manifest_path")

    adapter_packages = [
        package
        for package in packages
        if any(str(package.get("name", "")).endswith(suffix) for suffix in ADAPTER_SUFFIXES)
    ]
    if not adapter_packages:
        errors.append("workspace 至少需要一个 _gui adapter package")
        return sorted(set(errors))

    adapter_names = {str(package["name"]) for package in adapter_packages}
    expected_adapter_names = {f"{project_id}{suffix}" for suffix in ADAPTER_SUFFIXES}
    for adapter_name in sorted(adapter_names):
        if adapter_name not in expected_adapter_names:
            errors.append(
                f"adapter package {adapter_name} 与共享核心项目标识 {project_id} 不一致"
            )

    packages_by_name = {str(package["name"]): package for package in packages}
    dependency_graph = _local_dependency_graph(packages_by_name)
    core_paths = _reachable_paths(core_name, dependency_graph)
    for package_name, path in sorted(core_paths.items()):
        path_text = " -> ".join(path)
        if package_name != core_name and package_name in adapter_names:
            errors.append(
                f"共享核心 {core_name} 不得依赖 adapter {package_name} "
                f"(workspace 路径: {path_text})"
            )
            continue
        for dependency in _dependencies(packages_by_name[package_name]):
            dependency_name = dependency.get("name")
            if not isinstance(dependency_name, str):
                errors.append(f"workspace package {package_name} 含缺少 name 的依赖")
            elif _is_interface_crate(dependency_name):
                errors.append(
                    f"共享核心 {core_name} 不得经 workspace 路径 {path_text} "
                    f"引入接口框架 {dependency_name} ({_dependency_scope(dependency)})"
                )

    for adapter_package in adapter_packages:
        adapter_name = str(adapter_package["name"])
        dependencies = _dependencies(adapter_package)
        direct_core_dependencies = [
            dependency
            for dependency in dependencies
            if dependency.get("name") == core_name
            and dependency.get("kind") is None
            and dependency.get("target") is None
            and not bool(dependency.get("optional", False))
            and _points_to_workspace_package(dependency, core_package)
        ]
        if not direct_core_dependencies:
            errors.append(
                f"adapter {adapter_name} 必须以非可选、非 target-specific 的普通依赖"
                f"直接依赖 workspace 内的 {core_name}"
            )
        for package_name, path in sorted(
            _reachable_paths(adapter_name, dependency_graph).items()
        ):
            if package_name in adapter_names and package_name != adapter_name:
                errors.append(
                    f"adapter {adapter_name} 不得依赖 adapter {package_name} "
                    f"(workspace 路径: {' -> '.join(path)})"
                )

    return sorted(set(errors))


def load_cargo_metadata(
    workspace_root: Path,
    *,
    cargo: str = "cargo",
    timeout_seconds: int = 60,
) -> dict[str, Any]:
    """只读运行 cargo metadata，并返回可验证的 JSON 对象。"""

    root = workspace_root.resolve(strict=True)
    manifest = root / "Cargo.toml"
    if not manifest.is_file():
        raise RuntimeError(f"workspace 根缺少 Cargo.toml: {root}")
    command = [
        cargo,
        "metadata",
        "--no-deps",
        "--locked",
        "--format-version",
        "1",
        "--manifest-path",
        str(manifest),
    ]
    try:
        result = subprocess.run(
            command,
            cwd=root,
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
            timeout=timeout_seconds,
        )
    except FileNotFoundError as error:
        raise RuntimeError(f"无法执行 Cargo: {cargo}") from error
    except subprocess.TimeoutExpired as error:
        raise RuntimeError(f"cargo metadata 在 {timeout_seconds} 秒后超时") from error
    if result.returncode != 0:
        diagnostic = result.stderr.strip() or result.stdout.strip() or "无诊断"
        raise RuntimeError(f"cargo metadata 失败: {diagnostic}")
    try:
        metadata = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"cargo metadata 输出不是合法 JSON: {error}") from error
    if not isinstance(metadata, dict):
        raise RuntimeError("cargo metadata 输出根必须是 JSON 对象")
    return metadata


def _parse_args(argv: list[str] | None) -> argparse.Namespace:
    """解析只读检查入口的参数。"""

    parser = argparse.ArgumentParser(description="检查 Rust workspace 的 core-first 确定性依赖边界。")
    parser.add_argument(
        "--workspace-root", type=Path, default=Path.cwd(), help="包含根 Cargo.toml 的项目目录；默认当前目录。"
    )
    parser.add_argument(
        "--metadata-file", type=Path, help="读取既有 Cargo metadata JSON；用于隔离测试或审计。"
    )
    parser.add_argument("--cargo", default="cargo", help="Cargo 可执行文件名或路径。")
    parser.add_argument(
        "--timeout-seconds", type=int, default=60, help="cargo metadata 的正整数超时秒数。"
    )
    parser.add_argument("--json", action="store_true", help="输出稳定 JSON 结果。")
    return parser.parse_args(argv)


def _emit_tool_error(message: str, json_output: bool) -> None:
    """按调用方选择输出稳定 JSON 或人类可读工具错误。"""

    if json_output:
        value = {"ok": False, "toolError": message, "errors": []}
        print(json.dumps(value, ensure_ascii=False, sort_keys=True))
    else:
        print(f"ERROR: {message}", file=sys.stderr)


def main(argv: list[str] | None = None) -> int:
    """执行只读依赖检查，并以 0/1/2 区分通过、违规和工具失败。"""

    args = _parse_args(argv)
    if args.timeout_seconds <= 0:
        _emit_tool_error("--timeout-seconds 必须为正整数", args.json)
        return 2
    try:
        if args.metadata_file is not None:
            metadata_path = args.metadata_file.resolve(strict=True)
            metadata = json.loads(metadata_path.read_text(encoding="utf-8"))
            if not isinstance(metadata, dict):
                raise RuntimeError("metadata 文件根必须是 JSON 对象")
        else:
            metadata = load_cargo_metadata(
                args.workspace_root,
                cargo=args.cargo,
                timeout_seconds=args.timeout_seconds,
            )
    except (OSError, RuntimeError, json.JSONDecodeError) as error:
        _emit_tool_error(str(error), args.json)
        return 2

    errors = validate_metadata(metadata)
    if args.json:
        print(
            json.dumps(
                {"ok": not errors, "toolError": None, "errors": errors},
                ensure_ascii=False,
                sort_keys=True,
            )
        )
    elif errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
    else:
        print("core-first dependency check passed")
    return 1 if errors else 0


if __name__ == "__main__":
    raise SystemExit(main())
