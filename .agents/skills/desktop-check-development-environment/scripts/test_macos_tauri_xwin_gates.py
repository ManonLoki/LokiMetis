#!/usr/bin/env python3
from __future__ import annotations

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("macos-tauri-xwin-gates.sh")


def posix_shell() -> str | None:
    """解析测试可用的 sh；Windows 优先复用已安装 Git for Windows 的 shell。"""
    shell = shutil.which("sh")
    if shell is not None or os.name != "nt":
        return shell
    program_files = os.environ.get("ProgramFiles")
    if program_files:
        candidate = Path(program_files) / "Git" / "bin" / "sh.exe"
        if candidate.is_file():
            return str(candidate)
    return None


def shell_path(path: Path) -> str:
    """把测试路径转换为当前 sh 可识别且不会被盘符冒号拆分的形式。"""
    resolved = path.resolve().as_posix()
    if os.name == "nt" and len(resolved) >= 3 and resolved[1:3] == ":/":
        return f"/{resolved[0].lower()}{resolved[2:]}"
    return resolved


def executable(path: Path, content: str) -> None:
    """创建隔离测试命令，不接触真实 Homebrew、Rust target 或 Cargo 安装目录。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)


def add_existing_environment(
    root: Path,
    *,
    include_cross_tools: bool,
    cargo_xwin_version: str = "0.23.1",
    installed_cargo_xwin_version: str = "0.23.1",
    cargo_install_exit: int = 0,
) -> Path:
    """建立常规 GUI 门禁已通过的最小工具集，并按场景加入现成交叉工具。"""
    probe = root / "probe"
    state = root / "state"
    state.mkdir(parents=True)
    executable(
        probe / "rustup",
        f"""#!/bin/sh
set -eu
if [ "$1 $2 $3" = "target list --installed" ]; then
  [ -f "{shell_path(state / 'target')}" ] && printf '%s\n' 'x86_64-pc-windows-msvc'
elif [ "$1 $2" = "target add" ]; then
  : > "{shell_path(state / 'target')}"
else
  exit 2
fi
""",
    )
    executable(
        probe / "cargo",
        f"""#!/bin/sh
set -eu
if [ "$1 $2 $3 $4 $5" = "install --locked --version >=0.23.1, <0.24.0 cargo-xwin" ]; then
  [ {cargo_install_exit} -eq 0 ] || exit {cargo_install_exit}
  mkdir -p "$CARGO_HOME/bin"
  printf '#!/bin/sh\nprintf "%%s\\n" "cargo-xwin {installed_cargo_xwin_version}"\n' > "$CARGO_HOME/bin/cargo-xwin"
  chmod +x "$CARGO_HOME/bin/cargo-xwin"
else
  exit 2
fi
""",
    )
    executable(probe / "pnpm", "#!/bin/sh\nprintf '%s\n' '11.24.0'\n")
    if include_cross_tools:
        (state / "target").touch()
        executable(probe / "llvm-rc", "#!/bin/sh\nprintf '%s\n' 'llvm-rc test'\n")
        executable(probe / "lld-link", "#!/bin/sh\nprintf '%s\n' 'lld-link test'\n")
        executable(probe / "makensis", "#!/bin/sh\nprintf '%s\n' 'NSIS test'\n")
        executable(
            probe / "cargo-xwin",
            f"#!/bin/sh\nprintf '%s\\n' 'cargo-xwin {cargo_xwin_version}'\n",
        )
    return probe


def add_fake_formula_tool(root: Path, formula: str, tool: str) -> None:
    """预置单个 Homebrew formula 工具，用于覆盖 LLVM 与 LLD 拆包场景。"""
    executable(root / "brew" / formula / "bin" / tool, "#!/bin/sh\nexit 0\n")


def add_fake_brew(root: Path, probe: Path, *, fail_formula: str | None = None) -> None:
    """提供只写隔离 prefix 的 Homebrew 替身，验证安装、失败和复探分支。"""
    brew_root = root / "brew"
    executable(
        probe / "brew",
        f"""#!/bin/sh
set -eu
root='{shell_path(brew_root)}'
if [ "$1" = "--prefix" ]; then
  [ -d "$root/$2" ] || exit 1
  printf '%s\n' "$root/$2"
elif [ "$1" = "install" ]; then
  formula=$2
  [ "$formula" != "{fail_formula or ''}" ] || exit 9
  mkdir -p "$root/$formula/bin"
    case "$formula" in
    llvm)
      printf '#!/bin/sh\nprintf "%%s\\n" "Exactly one input file should be provided." >&2\nexit 1\n' > "$root/$formula/bin/llvm-rc"
      chmod +x "$root/$formula/bin/llvm-rc"
      ;;
    lld)
      printf '#!/bin/sh\nexit 0\n' > "$root/$formula/bin/lld-link"
      chmod +x "$root/$formula/bin/lld-link"
      ;;
    nsis)
      printf '#!/bin/sh\nexit 0\n' > "$root/$formula/bin/makensis"
      chmod +x "$root/$formula/bin/makensis"
      ;;
  esac
else
  exit 2
fi
""",
    )


class MacosTauriXwinGateTests(unittest.TestCase):
    """覆盖 macOS Tauri xwin 门禁的成功路径和最高风险安装失败路径。"""

    def run_gate(
        self,
        root: Path,
        probe: Path,
        *args: str,
        test_mode: bool = True,
        **extra: str,
    ) -> subprocess.CompletedProcess[str]:
        """在伪造 macOS 与独立 HOME/Cargo 目录运行门禁，禁止真实环境修改。"""
        shell = posix_shell()
        if shell is None:
            self.skipTest("当前测试宿主没有可执行的 POSIX sh")
        env = os.environ.copy()
        env.update(
            {
                "AFH_PREREQ_PATH": shell_path(probe),
                "AFH_TEST_PLATFORM": "Darwin",
                "AFH_ALLOW_TEST_OVERRIDES": "1",
                "AFH_TEST_MODE": "1",
                "HOME": shell_path(root / "home"),
                "CARGO_HOME": shell_path(root / "project" / ".cargo"),
                "RUSTUP_HOME": shell_path(root / "project" / ".rustup"),
                "AFH_MANAGED_CARGO_HOME": shell_path(root / "cargo"),
                "AFH_MANAGED_RUSTUP_HOME": shell_path(root / "rustup"),
            }
        )
        if not test_mode:
            env.pop("AFH_TEST_MODE", None)
        env.update(extra)
        return subprocess.run(
            [shell, shell_path(SCRIPT), *args],
            text=True,
            capture_output=True,
            cwd=root,
            env=env,
            timeout=30,
            check=False,
        )

    def test_test_overrides_require_explicit_test_mode(self) -> None:
        """伪造宿主或探测路径必须同时显式启用统一测试模式。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            probe.mkdir()
            result = self.run_gate(root, probe, "--check-only", test_mode=False)
            self.assertEqual(result.returncode, 2)
            self.assertIn("仅在 AFH_TEST_MODE=1", result.stderr)

    def test_existing_environment_passes_without_installing(self) -> None:
        """全部工具与 target 已存在时必须无写入通过。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=True)
            result = self.run_gate(root, probe, "--install-missing")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.tauri_windows_cross.status=passed", result.stdout)
            self.assertIn(
                "gate.cargo_xwin.requirement=>=0.23.1, <0.24.0",
                result.stdout,
            )
            self.assertIn("gate.cargo_xwin.version=0.23.1", result.stdout)
            self.assertIn("gate.changed=false", result.stdout)
            self.assertFalse((root / "cargo").exists())

    def test_missing_environment_is_installed_and_reprobed(self) -> None:
        """缺失交叉工具时通过既有包管理器安装，复探全部成功后才通过。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=False)
            add_fake_brew(root, probe)
            result = self.run_gate(root, probe, "--install-missing")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.llvm.change=installed", result.stdout)
            self.assertIn("gate.lld.change=installed", result.stdout)
            self.assertIn("gate.nsis.change=installed", result.stdout)
            self.assertIn("gate.rust_target.change=installed", result.stdout)
            self.assertIn("gate.cargo_xwin.change=installed", result.stdout)
            self.assertIn(
                "gate.cargo_xwin.requirement=>=0.23.1, <0.24.0",
                result.stdout,
            )
            self.assertIn("gate.cargo_xwin.version=0.23.1", result.stdout)
            self.assertIn("gate.changed=true", result.stdout)
            self.assertTrue((root / "cargo" / "bin" / "cargo-xwin").is_file())
            self.assertFalse((root / "project").exists())

    def test_install_path_output_drops_empty_and_duplicate_probe_segments(self) -> None:
        """安装分支必须清除探测 PATH 空段/重复项，输出也不得重新引入 cwd 语义。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=False)
            add_fake_brew(root, probe)
            probe_value = shell_path(probe)
            result = self.run_gate(
                root,
                probe,
                "--install-missing",
                AFH_PREREQ_PATH=f":{probe_value}::{probe_value}:",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            path_line = next(
                line for line in result.stdout.splitlines() if line.startswith("gate.path.prepend=")
            )
            entries = path_line.removeprefix("gate.path.prepend=").split(":")
            self.assertNotIn("", entries)
            self.assertEqual(len(entries), len(dict.fromkeys(entries)))

    def test_literal_glob_probe_entry_is_never_expanded(self) -> None:
        """绝对 PATH 字面 glob 必须保持字面量，不能扫描 cwd 或匹配目录中的 shim。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=True)
            (probe / "cargo-xwin").unlink()
            glob_match = root / "glob-match"
            glob_match.mkdir()
            executable(glob_match / "cargo-xwin", '#!/bin/sh\nprintf "%s\\n" "cargo-xwin 0.23.1"\n')

            result = self.run_gate(
                root,
                probe,
                "--check-only",
                AFH_PREREQ_PATH=f"{shell_path(root)}/*:{shell_path(probe)}",
            )

            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.cargo_xwin.status=missing", result.stdout)

    def test_higher_compatible_cargo_xwin_is_preserved(self) -> None:
        """0.23 系列中高于下界的稳定版本必须直接通过且不静默替换。"""

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(
                root, include_cross_tools=True, cargo_xwin_version="0.23.9"
            )
            result = self.run_gate(root, probe, "--install-missing")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.cargo_xwin.version=0.23.9", result.stdout)
            self.assertIn("gate.cargo_xwin.change=existing", result.stdout)

    def test_outdated_cargo_xwin_is_upgraded_and_reprobed(self) -> None:
        """明确低于 0.23.1 的稳定版本必须安装受限新版，复探后标记 upgraded。"""
        for version in ("0.22.9", "0.23.0"):
            with self.subTest(version=version), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = add_existing_environment(
                    root,
                    include_cross_tools=True,
                    cargo_xwin_version=version,
                )
                result = self.run_gate(root, probe, "--install-missing")
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("gate.tauri_windows_cross.status=passed", result.stdout)
                self.assertIn("gate.cargo_xwin.status=passed", result.stdout)
                self.assertIn("gate.cargo_xwin.version=0.23.1", result.stdout)
                self.assertIn("gate.cargo_xwin.change=upgraded", result.stdout)
                self.assertIn("gate.changed=true", result.stdout)
                self.assertTrue((root / "cargo" / "bin" / "cargo-xwin").is_file())

    def test_check_only_reports_outdated_cargo_xwin_without_writes(self) -> None:
        """只读模式必须结构化报告 upgrade-required 并以 20 结束，不能写 Cargo 目录。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(
                root,
                include_cross_tools=True,
                cargo_xwin_version="0.23.0",
            )
            result = self.run_gate(root, probe, "--check-only")
            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.tauri_windows_cross.status=upgrade-required", result.stdout)
            self.assertIn("gate.cargo_xwin.status=upgrade-required", result.stdout)
            self.assertIn("gate.cargo_xwin.version=0.23.0", result.stdout)
            self.assertFalse((root / "cargo").exists())

    def test_upgrade_failure_does_not_claim_success(self) -> None:
        """旧版本升级命令失败时必须保留安装错误，不能输出通过或 changed=true。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(
                root,
                include_cross_tools=True,
                cargo_xwin_version="0.23.0",
                cargo_install_exit=9,
            )
            result = self.run_gate(root, probe, "--install-missing")
            self.assertEqual(result.returncode, 36)
            self.assertIn("cargo-xwin 安装失败", result.stderr)
            self.assertNotIn("gate.tauri_windows_cross.status=passed", result.stdout)
            self.assertNotIn("gate.changed=true", result.stdout)

    def test_upgrade_reprobe_rejects_still_outdated_cargo_xwin(self) -> None:
        """升级命令即使返回成功，复探仍低于下界也必须阻断并报告 upgrade-required。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(
                root,
                include_cross_tools=True,
                cargo_xwin_version="0.22.9",
                installed_cargo_xwin_version="0.23.0",
            )
            result = self.run_gate(root, probe, "--install-missing")
            self.assertEqual(result.returncode, 37)
            self.assertIn("gate.cargo_xwin.status=upgrade-required", result.stdout)
            self.assertIn("gate.cargo_xwin.version=0.23.0", result.stdout)
            self.assertIn("安装后复探仍失败", result.stderr)

    def test_non_upgradeable_cargo_xwin_is_not_replaced(self) -> None:
        """过高、预发布或不可解析版本不是可证明的旧版，必须失败且不得降级。"""
        for version in ("0.24.0", "0.23.1-beta.1", "invalid"):
            with self.subTest(version=version), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = add_existing_environment(
                    root,
                    include_cross_tools=True,
                    cargo_xwin_version=version,
                )
                result = self.run_gate(root, probe, "--install-missing")
                self.assertEqual(result.returncode, 36)
                self.assertIn("不满足兼容范围 >=0.23.1, <0.24.0", result.stderr)
                self.assertFalse((root / "cargo").exists())

    def test_check_only_reports_missing_without_writes(self) -> None:
        """只读模式必须报告缺失且不调用 Homebrew、rustup target add 或 cargo install。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=False)
            add_fake_brew(root, probe)
            result = self.run_gate(root, probe, "--check-only")
            self.assertEqual(result.returncode, 20)
            self.assertIn("gate.tauri_windows_cross.status=missing", result.stdout)
            self.assertFalse((root / "brew" / "llvm").exists())
            self.assertFalse((root / "state" / "target").exists())
            self.assertFalse((root / "cargo").exists())

    def test_missing_homebrew_blocks_install(self) -> None:
        """LLVM/NSIS 缺失且没有既有 Homebrew 时必须阻断，不能执行远程 shell 安装器。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=False)
            result = self.run_gate(root, probe, "--install-missing")
            self.assertEqual(result.returncode, 32)
            self.assertIn("不自动安装 Homebrew", result.stderr)

    def test_formula_install_failure_does_not_claim_success(self) -> None:
        """Homebrew 安装失败必须保留非零状态，不能跳过复探或继续构建。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=False)
            add_fake_brew(root, probe, fail_formula="llvm")
            result = self.run_gate(root, probe, "--install-missing")
            self.assertEqual(result.returncode, 33)
            self.assertIn("LLVM 安装失败", result.stderr)

    def test_split_llvm_install_adds_missing_lld_formula(self) -> None:
        """LLVM 已存在但 LLD 已拆包时必须安装独立 lld，并返回完整 PATH。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=False)
            add_fake_brew(root, probe)
            add_fake_formula_tool(root, "llvm", "llvm-rc")
            add_fake_formula_tool(root, "nsis", "makensis")
            (root / "state" / "target").touch()
            executable(
                probe / "cargo-xwin",
                "#!/bin/sh\nprintf '%s\\n' 'cargo-xwin 0.23.1'\n",
            )

            result = self.run_gate(root, probe, "--install-missing")

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.llvm.change=existing", result.stdout)
            self.assertIn("gate.lld.change=installed", result.stdout)
            self.assertIn(shell_path(root / "brew" / "lld" / "bin"), result.stdout)

    def test_damaged_existing_lld_formula_is_not_silently_reinstalled(self) -> None:
        """已有 lld formula 缺少 lld-link 时必须失败关闭，避免静默改写用户工具链。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=False)
            add_fake_brew(root, probe)
            add_fake_formula_tool(root, "llvm", "llvm-rc")
            add_fake_formula_tool(root, "nsis", "makensis")
            (root / "brew" / "lld" / "bin").mkdir(parents=True)

            result = self.run_gate(root, probe, "--install-missing")

            self.assertEqual(result.returncode, 33)
            self.assertIn("既有 LLD 缺少 lld-link", result.stderr)

    def test_non_macos_host_is_rejected(self) -> None:
        """专用门禁不得在非 macOS 宿主伪装可用。"""
        shell = posix_shell()
        if shell is None:
            self.skipTest("当前测试宿主没有可执行的 POSIX sh")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=True)
            env = os.environ.copy()
            env.update(
                {
                    "AFH_PREREQ_PATH": shell_path(probe),
                    "AFH_TEST_PLATFORM": "Linux",
                    "AFH_ALLOW_TEST_OVERRIDES": "1",
                    "AFH_TEST_MODE": "1",
                }
            )
            result = subprocess.run(
                [shell, shell_path(SCRIPT), "--check-only"],
                text=True,
                capture_output=True,
                env=env,
                timeout=30,
                check=False,
            )
            self.assertEqual(result.returncode, 30)
            self.assertIn("requires-macos-host", result.stdout)

    def test_unsupported_target_is_rejected(self) -> None:
        """当前范围只允许 Windows x64 MSVC，其他目标不能被静默接受。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = add_existing_environment(root, include_cross_tools=True)
            result = self.run_gate(
                root,
                probe,
                "--check-only",
                "--target",
                "aarch64-pc-windows-msvc",
            )
            self.assertEqual(result.returncode, 2)
            self.assertIn("不支持的 Tauri xwin 目标", result.stderr)


if __name__ == "__main__":
    unittest.main()
