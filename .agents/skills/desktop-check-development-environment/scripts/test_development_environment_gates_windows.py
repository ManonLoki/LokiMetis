#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest
import zipfile
from pathlib import Path


SCRIPT = Path(__file__).with_name("development-environment-gates.ps1")
POWERSHELL = shutil.which("powershell") or shutil.which("pwsh")


def command(path: Path, body: str) -> None:
    """写入隔离测试命令，使 PowerShell 门禁不读取真实宿主工具。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"@echo off\r\n{body}\r\n", encoding="utf-8")


def add_base_tools(
    probe: Path,
    *,
    git: str = "2.50.0",
    rust: str = "1.95.0",
    node: str | None = "26.0.0",
    pnpm: str | None = "11.24.0",
) -> None:
    """建立满足门禁或由状态标记升级的 Windows 工具夹具。"""
    command(probe / "cl.cmd", "exit /b 0")
    command(probe / "git.cmd", f"echo git version {git}")
    command(
        probe / "rustc.cmd",
        f'if "%1"=="-vV" (echo rustc {rust} ^(test^) & echo host: x86_64-pc-windows-msvc & echo release: {rust} & exit /b 0)\r\n'
        f"echo rustc {rust} (test)",
    )
    command(probe / "cargo.cmd", f"echo cargo {rust} (test)")
    command(probe / "rustup.cmd", "echo rustup 1.28.2 (test)")
    if node is not None:
        command(probe / "node.cmd", f"echo v{node}")
        command(probe / "npm.cmd", "echo 11.6.0")
    if pnpm is not None:
        command(probe / "pnpm.cmd", f"echo {pnpm}")


def make_node_dist(root: Path) -> str:
    """生成带摘要的 Windows Node 本地镜像，验证低版本升级与复探。"""
    version = "v26.0.0"
    archive_name = f"node-{version}-win-x64.zip"
    release = root / version
    release.mkdir(parents=True)
    (root / "index.json").write_text(
        json.dumps([{"version": version}]), encoding="utf-8"
    )
    archive = release / archive_name
    with zipfile.ZipFile(archive, "w") as bundle:
        bundle.writestr(f"node-{version}-win-x64/node.cmd", "@echo off\r\necho v26.0.0\r\n")
        bundle.writestr(
            f"node-{version}-win-x64/npm.cmd",
            "@echo off\r\n"
            "if \"%~1\"==\"--version\" echo 11.6.0\r\n"
            "if \"%~1\"==\"--version\" exit /b 0\r\n"
            "if not \"%~1\"==\"install\" exit /b 91\r\n"
            "if not \"%~3\"==\"--prefix\" exit /b 92\r\n"
            "if not \"%~5\"==\"pnpm@>=11.24.0\" exit /b 93\r\n"
            "if not \"%~6\"==\"--registry\" exit /b 94\r\n"
            "set \"expected_registry=https://registry.npmjs.org/\"\r\n"
            "if defined AFH_PNPM_REGISTRY set \"expected_registry=%AFH_PNPM_REGISTRY%\"\r\n"
            "if not \"%~7\"==\"%expected_registry%\" exit /b 95\r\n"
            "if not \"%~8\"==\"--ignore-scripts\" exit /b 96\r\n"
            "if not exist \"%~4\" mkdir \"%~4\"\r\n"
            "> \"%~4\\pnpm.cmd\" echo @echo off\r\n"
            ">> \"%~4\\pnpm.cmd\" echo echo 12.1.0\r\n"
            "exit /b 0\r\n",
        )
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    (release / "SHASUMS256.txt").write_text(
        f"{digest}  {archive_name}\n", encoding="utf-8"
    )
    return root.as_uri()


@unittest.skipUnless(os.name == "nt" and POWERSHELL, "requires Windows PowerShell")
class WindowsPrerequisiteGateTests(unittest.TestCase):
    """验证 Windows 门禁会升级低版本并在复探失败时阻断。"""

    def run_gate(
        self,
        root: Path,
        probe: Path,
        *arguments: str,
        test_mode: bool = True,
        **extra: str,
    ) -> subprocess.CompletedProcess[str]:
        """在隔离路径运行 PowerShell 门禁并禁用用户 PATH 持久化。"""
        (root / "home").mkdir(parents=True, exist_ok=True)
        environment = os.environ.copy()
        environment.update(
            {
                "AFH_PREREQ_PATH": str(probe),
                "AFH_NODE_HOME": str(root / "node-home"),
                "AFH_PNPM_HOME": str(root / "pnpm-home"),
                "AFH_SKIP_PERSIST_PATH": "1",
                "AFH_TEST_MODE": "1",
                "AFH_TEST_MACHINE_PATH": str(probe),
                "CARGO_HOME": str(root / "project" / ".cargo"),
                "RUSTUP_HOME": str(root / "project" / ".rustup"),
                "AFH_MANAGED_CARGO_HOME": str(root / "cargo"),
                "AFH_MANAGED_RUSTUP_HOME": str(root / "rustup"),
                "USERPROFILE": str(root / "home"),
            }
        )
        if not test_mode:
            environment.pop("AFH_TEST_MODE", None)
        environment.update(extra)
        return subprocess.run(
            [
                str(POWERSHELL),
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                str(SCRIPT),
                *arguments,
            ],
            text=True,
            capture_output=True,
            cwd=root,
            env=environment,
            timeout=30,
            check=False,
        )

    def test_check_only_reports_upgrade_required_without_writes(self) -> None:
        """只读检查发现旧 Git 时报告待升级，且不调用 winget。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, git="1.99.9")
            command(probe / "winget.cmd", 'type nul > "%~dp0winget-called"')

            result = self.run_gate(root, probe, "-CheckOnly")

            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.git.status=upgrade-required", result.stdout)
            self.assertIn("gate.git.version=git version 1.99.9", result.stdout)
            self.assertFalse((probe / "winget-called").exists())

    def test_missing_rustup_blocks_even_when_rustc_and_cargo_exist(self) -> None:
        """Windows 门禁必须把 rustup 作为 Rust 工具链的必需组成，而不是只看编译器。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe)
            (probe / "rustup.cmd").unlink()
            result = self.run_gate(root, probe, "-CheckOnly")
            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.rust.status=missing", result.stdout)
            self.assertIn("gate.rust.rustup_version=Missing", result.stdout)

    def test_rustc_verbose_release_mismatch_fails_closed(self) -> None:
        """Windows rustc -vV 的 release 不一致时必须按损坏工具链阻断。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe)
            command(
                probe / "rustc.cmd",
                'if "%1"=="-vV" (echo rustc 1.95.0 ^(test^) & echo host: x86_64-pc-windows-msvc & echo release: 1.94.0 & exit /b 0)\r\n'
                "echo rustc 1.95.0 (test)",
            )
            result = self.run_gate(root, probe, "-CheckOnly")
            self.assertEqual(result.returncode, 21)
            self.assertIn("release 与 rustc --version 不一致", result.stderr)

    def test_cargo_below_msrv_cannot_pass_with_new_rustc(self) -> None:
        """新 rustc 混入旧 cargo 时必须按损坏工具链失败关闭，不能判为通过。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, rust="1.96.0")
            command(probe / "cargo.cmd", "echo cargo 1.94.9 (test)")

            result = self.run_gate(root, probe, "-CheckOnly")

            self.assertEqual(result.returncode, 21, result.stderr)
            self.assertIn("不属于同一 stable 工具链", result.stderr)

    def test_inherited_rust_homes_cannot_redirect_managed_operations(self) -> None:
        """项目内标准 Rust 环境变量不得改变探测、升级或持久化使用的受管根。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            log = root / "rust-env.log"
            add_base_tools(probe, rust="1.94.9")
            prefix = f'>> "{log}" echo %CARGO_HOME%^|%RUSTUP_HOME%\r\n'
            command(
                probe / "rustup.cmd",
                prefix
                + 'if "%1"=="toolchain" (type nul > "%~dp0rust-upgraded" & exit /b 0)\r\n'
                + 'if "%1"=="default" exit /b 0\r\n'
                + "echo rustup 1.28.2 (test)",
            )
            command(
                probe / "rustc.cmd",
                prefix
                + 'set "rust_version=1.94.9"\r\n'
                + 'if exist "%~dp0rust-upgraded" set "rust_version=1.96.0"\r\n'
                + 'if "%1"=="-vV" (echo rustc %rust_version% ^(test^) & echo host: x86_64-pc-windows-msvc & echo release: %rust_version% & exit /b 0)\r\n'
                + "echo rustc %rust_version% (test)",
            )
            command(
                probe / "cargo.cmd",
                prefix
                + 'if exist "%~dp0rust-upgraded" (echo cargo 1.96.0 (test^)) else (echo cargo 1.94.9 (test^))',
            )

            result = self.run_gate(root, probe)

            self.assertEqual(result.returncode, 0, result.stderr)
            expected = f"{root / 'cargo'}|{root / 'rustup'}".casefold()
            rows = [row.strip().casefold() for row in log.read_text(encoding="utf-8").splitlines() if row.strip()]
            self.assertTrue(rows)
            self.assertTrue(all(row == expected for row in rows), rows)
            self.assertTrue((root / "cargo").is_dir())
            self.assertTrue((root / "rustup").is_dir())
            self.assertFalse((root / "project").exists())

    def test_below_minimum_git_is_upgraded_and_reprobed(self) -> None:
        """旧 Git 通过唯一 winget 路线升级，复探达标后才返回成功。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, git="1.99.9")
            command(
                probe / "git.cmd",
                'if exist "%~dp0git-upgraded" (echo git version 2.51.0) else (echo git version 1.99.9)',
            )
            command(probe / "winget.cmd", 'type nul > "%~dp0git-upgraded"')

            result = self.run_gate(root, probe)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.git.version=git version 2.51.0", result.stdout)
            self.assertIn("gate.git.change=upgraded", result.stdout)
            self.assertIn("gate.changed=true", result.stdout)

    def test_git_upgrade_that_remains_old_is_rejected(self) -> None:
        """升级命令返回成功但版本仍低于下界时不得宣称门禁通过。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, git="1.99.9")
            command(probe / "winget.cmd", "exit /b 0")

            result = self.run_gate(root, probe)

            self.assertEqual(result.returncode, 29)
            self.assertIn("安装或升级后仍不满足", result.stderr)
            self.assertNotIn("gate.git.status=passed", result.stdout)

    def test_below_msrv_rust_is_upgraded_with_existing_rustup(self) -> None:
        """低于 MSRV 的 Rust 通过既有 rustup 升级 stable 并重新探测。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, rust="1.94.9")
            command(
                probe / "rustc.cmd",
                'set "rust_version=1.94.9"\r\n'
                'if exist "%~dp0rust-upgraded" set "rust_version=1.96.0"\r\n'
                'if "%1"=="-vV" (echo rustc %rust_version% ^(test^) & echo host: x86_64-pc-windows-msvc & echo release: %rust_version% & exit /b 0)\r\n'
                "echo rustc %rust_version% (test)",
            )
            command(
                probe / "cargo.cmd",
                'if exist "%~dp0rust-upgraded" (echo cargo 1.96.0 (test^)) else (echo cargo 1.94.9 (test^))',
            )
            command(
                probe / "rustup.cmd",
                'if "%1"=="toolchain" (type nul > "%~dp0rust-upgraded" & exit /b 0)\r\n'
                'if "%1"=="default" exit /b 0\r\n'
                "echo rustup 1.28.2 (test)",
            )

            user_path_file = root / "user-path.txt"
            drive_root = Path(root.anchor)
            unc_root = "\\\\server\\share\\"
            user_path_file.write_text(
                f";bin;..\\evil;.;C:relative;{drive_root};{probe};{unc_root};{drive_root};{probe};;",
                encoding="utf-8",
            )
            result = self.run_gate(
                root,
                probe,
                AFH_SKIP_PERSIST_PATH="0",
                AFH_TEST_USER_PATH_FILE=str(user_path_file),
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.rust.version=rustc 1.96.0 (test)", result.stdout)
            self.assertIn("gate.rust.change=upgraded", result.stdout)
            self.assertIn("gate.changed=true", result.stdout)
            persisted_entries = [entry for entry in user_path_file.read_text(encoding="utf-8").split(";") if entry]
            self.assertEqual(persisted_entries, [str(probe), str(drive_root), unc_root])
            self.assertIn("gate.fresh_shell.status=passed", result.stdout)

    def test_only_git_change_requires_exact_fresh_powershell_discovery(self) -> None:
        """仅 Git 改变也必须从持久 User/Machine PATH 执行同一路径与版本。"""
        for globally_visible in (True, False):
            with self.subTest(globally_visible=globally_visible), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                add_base_tools(probe, git="1.99.9")
                command(
                    probe / "git.cmd",
                    'if exist "%~dp0git-upgraded" (echo git version 2.51.0) else (echo git version 1.99.9)',
                )
                command(probe / "winget.cmd", 'type nul > "%~dp0git-upgraded"')
                user_path_file = root / "user-path.txt"
                user_path_file.write_text("", encoding="utf-8")

                result = self.run_gate(
                    root,
                    probe,
                    AFH_SKIP_PERSIST_PATH="0",
                    AFH_TEST_USER_PATH_FILE=str(user_path_file),
                    AFH_TEST_MACHINE_PATH=str(probe) if globally_visible else "relative\\probe",
                )

                if globally_visible:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("gate.git.change=upgraded", result.stdout)
                    self.assertIn("gate.fresh_shell.status=passed", result.stdout)
                else:
                    self.assertEqual(result.returncode, 24, result.stderr)
                    self.assertIn("新的 PowerShell 无法从持久 User/Machine PATH", result.stderr)

    def test_rust_upgrade_persists_all_split_tool_directories(self) -> None:
        """rustup、rustc、cargo 分处不同目录时也必须全部进入 User PATH 并可由新进程发现。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe)
            for name in ("rustup.cmd", "rustc.cmd", "cargo.cmd"):
                (probe / name).unlink()
            rustup_bin = root / "rustup-bin"
            rustc_bin = root / "rustc-bin"
            cargo_bin = root / "cargo-bin"
            state = root / "rust-upgraded"
            command(
                rustup_bin / "rustup.cmd",
                f'if "%1"=="toolchain" (type nul > "{state}" & exit /b 0)\r\n'
                'if "%1"=="default" exit /b 0\r\n'
                "echo rustup 1.28.2 (test)",
            )
            command(
                rustc_bin / "rustc.cmd",
                'set "rust_version=1.94.9"\r\n'
                f'if exist "{state}" set "rust_version=1.96.0"\r\n'
                'if "%1"=="-vV" (echo rustc %rust_version% ^(test^) & echo host: x86_64-pc-windows-msvc & echo release: %rust_version% & exit /b 0)\r\n'
                "echo rustc %rust_version% (test)",
            )
            command(
                cargo_bin / "cargo.cmd",
                f'if exist "{state}" (echo cargo 1.96.0 (test^)) else (echo cargo 1.94.9 (test^))',
            )
            user_path_file = root / "user-path.txt"
            user_path_file.write_text(";;", encoding="utf-8")
            tool_probe = ";".join(map(str, (probe, rustup_bin, rustc_bin, cargo_bin)))
            result = self.run_gate(
                root,
                probe,
                AFH_PREREQ_PATH=tool_probe,
                AFH_SKIP_PERSIST_PATH="0",
                AFH_TEST_USER_PATH_FILE=str(user_path_file),
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            persisted = user_path_file.read_text(encoding="utf-8").split(";")
            self.assertEqual(persisted, [str(rustup_bin), str(rustc_bin), str(cargo_bin)])
            self.assertIn("gate.fresh_shell.status=passed", result.stdout)

    def test_below_minimum_node_is_upgraded_from_verified_archive(self) -> None:
        """GUI 的旧 Node 使用当前 Windows 官方形态制品升级并通过摘要复探。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, node="22.16.0", pnpm="11.24.0")
            node_dist = make_node_dist(root / "node-dist")

            result = self.run_gate(
                root,
                probe,
                AFH_ALLOW_FILE_URLS="1",
                AFH_NODE_DIST_BASE=node_dist,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.node.version=v26.0.0", result.stdout)
            self.assertIn("gate.node.change=upgraded", result.stdout)
            self.assertIn("gate.changed=true", result.stdout)

    def test_existing_selected_node_directory_must_match_verified_version(self) -> None:
        """带匹配摘要 marker 的 v26 目录若实际只运行 v24，也必须在链接或持久化前失败。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, node="22.16.0", pnpm="11.24.0")
            dist_root = root / "node-dist"
            node_dist = make_node_dist(dist_root)
            archive = next((dist_root / "v26.0.0").glob("*.zip"))
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            archive.unlink()
            install = root / "node-home" / "v26.0.0"
            command(install / "node.cmd", "echo v24.15.0")
            command(install / "npm.cmd", "echo 11.6.0")
            (install / ".agent-first-harness-managed").write_text(
                "managed by agent-first-harness development environment gate\n"
                "node.version=v26.0.0\n"
                f"node.archive.sha256={digest}\n",
                encoding="utf-8",
            )

            result = self.run_gate(
                root,
                probe,
                AFH_ALLOW_FILE_URLS="1",
                AFH_NODE_DIST_BASE=node_dist,
            )

            self.assertEqual(result.returncode, 26, result.stderr)
            self.assertIn("版本与已选择稳定版不一致", result.stderr)

    def test_below_minimum_pnpm_is_upgraded_and_reprobed(self) -> None:
        """GUI 的旧 pnpm 通过当前 Node 的 npm 升级并重新探测。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, node="26.0.0", pnpm="11.23.9")
            command(
                probe / "pnpm.cmd",
                'if exist "%~dp0pnpm-upgraded" (echo 12.1.0) else (echo 11.23.9)',
            )
            command(
                probe / "npm.cmd",
                'if "%~1"=="--version" echo 11.6.0\r\n'
                'if "%~1"=="--version" exit /b 0\r\n'
                'if not "%~5"=="pnpm@>=11.24.0" exit /b 91\r\n'
                'if not "%~6"=="--registry" exit /b 92\r\n'
                'if not "%~7"=="%AFH_PNPM_REGISTRY%" exit /b 93\r\n'
                'if not "%~8"=="--ignore-scripts" exit /b 94\r\n'
                'if not exist "%~4" mkdir "%~4"\r\n'
                '> "%~4\\pnpm.cmd" echo @echo off\r\n'
                '>> "%~4\\pnpm.cmd" echo echo 12.1.0\r\n'
                'type nul > "%~dp0pnpm-upgraded"',
            )

            result = self.run_gate(
                root,
                probe,
                AFH_PNPM_REGISTRY="https://registry.example.invalid/",
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.pnpm.version=12.1.0", result.stdout)
            self.assertIn("gate.pnpm.change=upgraded", result.stdout)
            self.assertIn("gate.changed=true", result.stdout)
            self.assertFalse((root / "11.24.0").exists())
            self.assertFalse((root / "=11.24.0").exists())

    def test_node_and_pnpm_are_discoverable_from_persisted_user_path(self) -> None:
        """缺失前端工具安装后，新 PowerShell 会话必须仅靠持久用户 PATH 发现它们。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, node=None, pnpm=None)
            node_dist = make_node_dist(root / "node-dist")
            user_path_file = root / "user-path.txt"
            user_path_file.write_text(";;", encoding="utf-8")

            result = self.run_gate(
                root,
                probe,
                AFH_ALLOW_FILE_URLS="1",
                AFH_NODE_DIST_BASE=node_dist,
                AFH_SKIP_PERSIST_PATH="0",
                AFH_TEST_USER_PATH_FILE=str(user_path_file),
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            persisted_entries = [entry for entry in user_path_file.read_text(encoding="utf-8").split(";") if entry]
            self.assertEqual(len(persisted_entries), len({entry.casefold() for entry in persisted_entries}))
            self.assertTrue(any(entry.endswith("node-home\\v26.0.0") for entry in persisted_entries))
            self.assertTrue(any(entry.endswith("pnpm-home") for entry in persisted_entries))
            self.assertIn("gate.fresh_shell.status=passed", result.stdout)

    def test_node_reparse_root_fails_before_archive_download(self) -> None:
        """Node 受管根若是 junction，必须在任何发布索引或归档读取前失败关闭。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, node="22.0.0", pnpm="11.24.0")
            outside = root / "outside"
            outside.mkdir()
            junction = root / "node-home"
            created = subprocess.run(
                ["cmd", "/c", "mklink", "/J", str(junction), str(outside)],
                text=True,
                capture_output=True,
                check=False,
            )
            if created.returncode != 0:
                self.skipTest("当前 Windows 宿主不能建立测试 junction")

            result = self.run_gate(
                root,
                probe,
                AFH_NODE_DIST_BASE=(root / "unreachable-node-dist").as_uri(),
                AFH_ALLOW_FILE_URLS="1",
            )

            self.assertEqual(result.returncode, 24, result.stderr)
            self.assertIn("reparse point", result.stderr)
            self.assertEqual(list(outside.iterdir()), [])

    def test_user_path_write_broadcasts_windows_environment_change(self) -> None:
        """真实 User PATH 写入必须广播 WM_SETTINGCHANGE，避免新桌面会话继承旧 PATH。"""
        source = SCRIPT.read_text(encoding="utf-8")
        self.assertIn("SendMessageTimeout", source)
        self.assertIn('"Environment"', source)
        self.assertIn("0x001A", source)
        self.assertIn("function Test-FreshPowerShellToolDiscovery", source)
        self.assertIn('[Environment]::GetEnvironmentVariable("Path", "Machine")', source)
        self.assertIn("-EncodedCommand", source)
        self.assertIn("-not [IO.Path]::IsPathRooted", source)

    def test_test_overrides_require_explicit_test_mode(self) -> None:
        """未显式启用 AFH_TEST_MODE 时，探测/下载覆盖必须在执行工具前被拒绝。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            result = self.run_gate(root, probe, "-CheckOnly", test_mode=False)
            self.assertEqual(result.returncode, 2)
            self.assertIn("仅在 AFH_TEST_MODE=1", result.stderr)

    def test_empty_probe_path_segment_never_resolves_cwd_shim(self) -> None:
        """Windows 探测 PATH 的空段不得执行当前目录同名包装器，且重复项会被消除。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe)
            (probe / "git.cmd").unlink()
            command(root / "git.cmd", "echo git version 2.50.0")
            result = self.run_gate(
                root,
                probe,
                "-CheckOnly",
                AFH_PREREQ_PATH=f";{probe};;{probe};",
            )
            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.git.status=missing", result.stdout)

    def test_windows_wrapper_precedes_extensionless_posix_shim(self) -> None:
        """npm/pnpm 同目录含 POSIX shim 时，Windows 门禁仍必须选择 cmd 包装器。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            add_base_tools(probe, node="26.0.0", pnpm="11.24.0")
            (probe / "pnpm").write_text("#!/bin/sh\nexit 99\n", encoding="utf-8")

            result = self.run_gate(root, probe, "-CheckOnly")

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.pnpm.status=passed", result.stdout)
            self.assertIn("gate.pnpm.version=11.24.0", result.stdout)


if __name__ == "__main__":
    unittest.main()
