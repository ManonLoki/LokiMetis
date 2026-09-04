"""覆盖可复用 core-first Cargo metadata 检查器的正负向行为。"""

from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock
from typing import Any

from check_core_first import main, validate_metadata

REGISTRY_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"


def dependency(
    name: str,
    *,
    kind: str | None = None,
    optional: bool = False,
    target: str | None = None,
    rename: str | None = None,
    path: str | None = None,
    source: str | None = REGISTRY_SOURCE,
) -> dict[str, Any]:
    """构造一个最小 Cargo metadata 依赖声明。"""

    return {
        "name": name,
        "kind": kind,
        "optional": optional,
        "target": target,
        "rename": rename,
        "path": path,
        "source": source,
    }


def workspace_dependency(name: str, **kwargs: Any) -> dict[str, Any]:
    """构造指向 fixture workspace package 的本地路径依赖。"""

    return dependency(
        name,
        path=f"/fixture/{name}",
        source=None,
        **kwargs,
    )


def package(
    name: str,
    *,
    dependencies: list[dict[str, Any]] | None = None,
    library: bool = False,
) -> dict[str, Any]:
    """构造一个最小 workspace package。"""

    return {
        "id": f"path+file:///fixture/{name}#0.1.0",
        "name": name,
        "manifest_path": f"/fixture/{name}/Cargo.toml",
        "dependencies": dependencies or [],
        "targets": [{"kind": ["lib" if library else "bin"]}],
    }


def metadata(*packages: dict[str, Any]) -> dict[str, Any]:
    """把 package 集合包装为 Cargo metadata workspace。"""

    return {
        "packages": list(packages),
        "workspace_members": [str(item["id"]) for item in packages],
    }


def run_main_json(*arguments: str) -> tuple[int, dict[str, Any], str, str]:
    """执行检查器 JSON 入口并返回退出码、对象和原始输出。"""

    stdout = io.StringIO()
    stderr = io.StringIO()
    with redirect_stdout(stdout), redirect_stderr(stderr):
        exit_code = main([*arguments, "--json"])
    output = stdout.getvalue()
    return exit_code, json.loads(output), output, stderr.getvalue()


class CoreFirstMetadataTests(unittest.TestCase):
    """验证依赖方向、接口框架和低误报边界。"""

    def test_accepts_core_gui_and_extra_infrastructure(self) -> None:
        """合法 GUI 直连 core，额外基础设施 crate 不应被误报。"""

        fixture = metadata(
            package(
                "sample_core",
                dependencies=[workspace_dependency("sample_infra")],
                library=True,
            ),
            package("sample_infra", library=True),
            package("sample_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        self.assertEqual(validate_metadata(fixture), [])

    def test_accepts_renamed_normal_core_dependency(self) -> None:
        """依赖别名不改变 adapter 已直接依赖 core 的事实。"""

        fixture = metadata(
            package("sample_core", library=True),
            package(
                "sample_gui",
                dependencies=[workspace_dependency("sample_core", rename="business")],
            ),
        )
        self.assertEqual(validate_metadata(fixture), [])

    def test_rejects_missing_unconditional_core_dependency(self) -> None:
        """仅 dev 或 target-specific 依赖不能代替普通直接 core 依赖。"""

        fixture = metadata(
            package("sample_core", library=True),
            package(
                "sample_gui",
                dependencies=[
                    workspace_dependency("sample_core", kind="dev"),
                    workspace_dependency(
                        "sample_core",
                        target="cfg(target_os = \"windows\")",
                    ),
                ],
            ),
        )
        errors = validate_metadata(fixture)
        self.assertTrue(any("必须以非可选" in error for error in errors), errors)

    def test_rejects_core_reverse_dependency_in_any_scope(self) -> None:
        """core 的 target-specific dev 依赖同样不得指向 adapter。"""

        fixture = metadata(
            package(
                "sample_core",
                dependencies=[
                    workspace_dependency(
                        "sample_gui",
                        kind="dev",
                        target="cfg(unix)",
                        rename="test_gui",
                    )
                ],
                library=True,
            ),
            package("sample_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        errors = validate_metadata(fixture)
        self.assertTrue(any("不得依赖 adapter sample_gui" in error for error in errors), errors)

    def test_rejects_adapter_to_adapter_dependency(self) -> None:
        """任何 normal/dev/build/target adapter 横向依赖都必须失败。"""

        fixture = metadata(
            package("sample_core", library=True),
            package(
                "sample_gui",
                dependencies=[
                    workspace_dependency("sample_core"),
                    workspace_dependency("other_gui", kind="dev"),
                ],
            ),
            package("other_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        errors = validate_metadata(fixture)
        self.assertTrue(any("不得依赖 adapter other_gui" in error for error in errors), errors)

    def test_rejects_interface_framework_in_core_production_or_build(self) -> None:
        """core 的普通或构建依赖不得包含接口框架和插件。"""

        fixture = metadata(
            package(
                "sample_core",
                dependencies=[
                    dependency("tauri"),
                    dependency("tauri-plugin-shell", kind="build"),
                    dependency("winit", kind="dev"),
                ],
                library=True,
            ),
            package("sample_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        errors = validate_metadata(fixture)
        self.assertTrue(any("接口框架 tauri" in error for error in errors), errors)
        self.assertTrue(
            any("接口框架 tauri-plugin-shell" in error for error in errors),
            errors,
        )
        self.assertTrue(any("接口框架 winit" in error for error in errors), errors)

    def test_rejects_interface_framework_in_core_dev_tests(self) -> None:
        """core 测试也不得借 dev 依赖耦合具体接口框架。"""

        fixture = metadata(
            package(
                "sample_core",
                dependencies=[dependency("tauri", kind="dev")],
                library=True,
            ),
            package("sample_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        errors = validate_metadata(fixture)
        self.assertTrue(any("接口框架 tauri" in error for error in errors), errors)

    def test_rejects_multiple_cores_and_foreign_adapter_prefix(self) -> None:
        """workspace 必须保持唯一 core 和同一确定性项目标识。"""

        multiple = metadata(
            package("sample_core", library=True),
            package("other_core", library=True),
            package("sample_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        self.assertTrue(any("只能包含一个" in error for error in validate_metadata(multiple)))

        foreign = metadata(
            package("sample_core", library=True),
            package("other_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        self.assertTrue(
            any("项目标识 sample 不一致" in error for error in validate_metadata(foreign))
        )

    def test_rejects_registry_crate_with_same_name_as_workspace_core(self) -> None:
        """registry 同名 crate 不能冒充 adapter 对本地共享 core 的直接依赖。"""

        fixture = metadata(
            package("sample_core", library=True),
            package("sample_gui", dependencies=[dependency("sample_core")]),
        )
        errors = validate_metadata(fixture)
        self.assertTrue(any("workspace 内的 sample_core" in error for error in errors), errors)

    def test_rejects_transitive_interface_framework_from_core(self) -> None:
        """core 不能通过无接口命名的 workspace infra 间接引入接口框架。"""

        fixture = metadata(
            package(
                "sample_core",
                dependencies=[workspace_dependency("sample_infra")],
                library=True,
            ),
            package("sample_infra", dependencies=[dependency("tauri")], library=True),
            package("sample_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        errors = validate_metadata(fixture)
        self.assertTrue(
            any(
                "sample_core -> sample_infra" in error and "接口框架 tauri" in error
                for error in errors
            ),
            errors,
        )

    def test_rejects_transitive_adapter_to_adapter_dependency(self) -> None:
        """adapter 不能通过普通 workspace bridge 间接到达另一 adapter。"""

        fixture = metadata(
            package("sample_core", library=True),
            package(
                "sample_bridge",
                dependencies=[workspace_dependency("other_gui")],
                library=True,
            ),
            package(
                "sample_gui",
                dependencies=[
                    workspace_dependency("sample_core"),
                    workspace_dependency("sample_bridge"),
                ],
            ),
            package("other_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        errors = validate_metadata(fixture)
        self.assertTrue(
            any(
                "sample_gui -> sample_bridge -> other_gui" in error
                and "不得依赖 adapter other_gui" in error
                for error in errors
            ),
            errors,
        )


class CoreFirstCommandTests(unittest.TestCase):
    """覆盖下游实际调用入口的参数、稳定 JSON 和 0/1/2 退出码。"""

    def test_command_returns_zero_for_mocked_cargo_metadata(self) -> None:
        """Cargo 成功返回合规 metadata 时必须稳定输出 ok 与退出码 0。"""

        fixture = metadata(
            package("sample_core", library=True),
            package("sample_gui", dependencies=[workspace_dependency("sample_core")]),
        )
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            completed = subprocess.CompletedProcess(
                args=[],
                returncode=0,
                stdout=json.dumps(fixture),
                stderr="",
            )
            with mock.patch("check_core_first.subprocess.run", return_value=completed) as run:
                code, payload, output, error_output = run_main_json(
                    "--workspace-root",
                    str(root),
                    "--timeout-seconds",
                    "7",
                )

        self.assertEqual(0, code)
        self.assertEqual({"errors": [], "ok": True, "toolError": None}, payload)
        self.assertEqual(json.dumps(payload, ensure_ascii=False, sort_keys=True) + "\n", output)
        self.assertEqual("", error_output)
        command = run.call_args.args[0]
        self.assertEqual("cargo", command[0])
        self.assertIn("--locked", command)
        self.assertEqual(7, run.call_args.kwargs["timeout"])

    def test_command_returns_one_for_architecture_violation(self) -> None:
        """metadata 合法但违反 core-first 时必须输出 errors 与退出码 1。"""

        fixture = metadata(
            package("sample_core", library=True),
            package("sample_gui", dependencies=[dependency("sample_core")]),
        )
        with tempfile.TemporaryDirectory() as tmp_dir:
            metadata_path = Path(tmp_dir) / "metadata.json"
            metadata_path.write_text(json.dumps(fixture), encoding="utf-8")
            code, payload, output, error_output = run_main_json(
                "--metadata-file",
                str(metadata_path),
            )

        self.assertEqual(1, code)
        self.assertFalse(payload["ok"])
        self.assertIsNone(payload["toolError"])
        self.assertTrue(any("workspace 内的 sample_core" in item for item in payload["errors"]))
        self.assertEqual(json.dumps(payload, ensure_ascii=False, sort_keys=True) + "\n", output)
        self.assertEqual("", error_output)

    def test_command_returns_two_for_json_and_cargo_tool_failures(self) -> None:
        """无效 JSON 与 Cargo 超时都必须归类为工具失败而非合规。"""

        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            malformed = root / "metadata.json"
            malformed.write_text("{", encoding="utf-8")
            code, payload, output, error_output = run_main_json(
                "--metadata-file",
                str(malformed),
            )
            self.assertEqual(2, code)
            self.assertFalse(payload["ok"])
            self.assertEqual([], payload["errors"])
            self.assertIn("Expecting property name", payload["toolError"])
            self.assertEqual(json.dumps(payload, ensure_ascii=False, sort_keys=True) + "\n", output)
            self.assertEqual("", error_output)

            code, payload, output, error_output = run_main_json(
                "--timeout-seconds",
                "0",
            )
            self.assertEqual(2, code)
            expected = {"errors": [], "ok": False, "toolError": "--timeout-seconds 必须为正整数"}
            self.assertEqual(expected, payload)
            self.assertEqual(json.dumps(payload, ensure_ascii=False, sort_keys=True) + "\n", output)
            self.assertEqual("", error_output)

            (root / "Cargo.toml").write_text("[workspace]\n", encoding="utf-8")
            timeout = subprocess.TimeoutExpired(cmd=["cargo", "metadata"], timeout=3)
            with mock.patch("check_core_first.subprocess.run", side_effect=timeout):
                code, payload, output, error_output = run_main_json(
                    "--workspace-root",
                    str(root),
                    "--timeout-seconds",
                    "3",
                )
            self.assertEqual(2, code)
            self.assertEqual(
                {"errors": [], "ok": False, "toolError": "cargo metadata 在 3 秒后超时"},
                payload,
            )
            self.assertEqual(json.dumps(payload, ensure_ascii=False, sort_keys=True) + "\n", output)
            self.assertEqual("", error_output)


if __name__ == "__main__":
    unittest.main()
