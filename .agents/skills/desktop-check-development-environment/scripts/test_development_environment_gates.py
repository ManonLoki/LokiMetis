#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import os
import platform
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("development-environment-gates.sh")
WINDOWS_SCRIPT = Path(__file__).with_name("development-environment-gates.ps1")


def executable(path: Path, content: str) -> None:
    """创建隔离测试专用可执行文件，不写入真实用户工具目录。"""
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)


def fake_existing_tools(
    bin_dir: Path,
    *,
    rust: str = "1.95.0",
    git: str = "2.39.0",
    include_git: bool = True,
    python: bool = True,
    frontend: bool = True,
) -> None:
    """构造可控版本的既有工具，验证门禁不会重装或静默升级。"""
    bin_dir.mkdir(parents=True, exist_ok=True)
    executable(bin_dir / "rustc", f"#!/bin/sh\nprintf '%s\\n' 'rustc {rust} (test)'\n")
    executable(bin_dir / "cargo", f"#!/bin/sh\nprintf '%s\\n' 'cargo {rust} (test)'\n")
    executable(bin_dir / "rustup", "#!/bin/sh\nprintf '%s\\n' 'rustup 1.28.0 (test)'\n")
    if include_git:
        executable(bin_dir / "git", f"#!/bin/sh\nprintf '%s\\n' 'git version {git}'\n")
    if python:
        executable(bin_dir / "python3", "#!/bin/sh\nprintf '%s\\n' 'Python 3.12.0'\n")
    if frontend:
        fake_frontend_tools(bin_dir)


def fake_frontend_tools(bin_dir: Path, *, node: str = "24.15.0", pnpm: str = "11.24.0") -> None:
    """构造既有 Node.js 与 pnpm，验证前端门禁不会修改已满足的环境。"""
    executable(bin_dir / "node", f"#!/bin/sh\nprintf '%s\\n' 'v{node}'\n")
    executable(bin_dir / "pnpm", f"#!/bin/sh\nprintf '%s\\n' '{pnpm}'\n")


def fake_git_package_manager(bin_dir: Path) -> None:
    """构造当前 Unix 宿主的受管包管理器，使缺失 Git 安装后可在同一探测路径复探。"""
    git_body = "#!/bin/sh\nprintf '%s\\n' 'git version 2.51.0'\n"
    if platform.system() == "Darwin":
        executable(
            bin_dir / "brew",
            f"""#!/bin/sh
set -eu
if [ "$1" = "--prefix" ]; then printf '%s\\n' "$AFH_PREREQ_PATH"; exit 0; fi
[ "$1" = "install" ] && [ "$2" = "git" ]
printf '%b' {git_body!r} > "$AFH_PREREQ_PATH/git"
chmod +x "$AFH_PREREQ_PATH/git"
""",
        )
        return
    executable(
        bin_dir / "apt-get",
        f"""#!/bin/sh
set -eu
if [ "$1" = "update" ]; then exit 0; fi
[ "$1" = "install" ] && [ "$2" = "-y" ] && [ "$3" = "git" ]
printf '%b' {git_body!r} > "$AFH_PREREQ_PATH/git"
chmod +x "$AFH_PREREQ_PATH/git"
""",
    )
    executable(bin_dir / "sudo", '#!/bin/sh\nexec "$@"\n')


def node_tuple() -> tuple[str, str]:
    """把当前测试宿主映射为 Node 官方归档命名，确保测试夹具与真实分支一致。"""
    system = platform.system()
    machine = platform.machine().lower()
    node_platform = {"Darwin": "darwin", "Linux": "linux"}[system]
    node_arch = {"x86_64": "x64", "amd64": "x64", "arm64": "arm64", "aarch64": "arm64"}[machine]
    return node_platform, node_arch


def make_node_dist(root: Path, *, valid_checksum: bool = True) -> str:
    """生成最小本地 Node 镜像，验证跳过 25.x 并选择最新兼容稳定版。"""
    version = "v24.15.0"
    node_platform, node_arch = node_tuple()
    release = root / version
    release.mkdir(parents=True)
    index = root / "index.tab"
    index.write_text(
        "version\tdate\tfiles\tnpm\tv8\tuv\tzlib\topenssl\tmodules\tlts\tsecurity\n"
        "v25.9.0\t2026-02-01\ttest\t11\t1\t1\t1\t1\t1\t-\t-\n"
        f"{version}\t2026-01-01\ttest\t11\t1\t1\t1\t1\t1\tTestLTS\t-\n",
        encoding="utf-8",
    )
    archive_name = f"node-{version}-{node_platform}-{node_arch}.tar.gz"
    source_root = root / f"node-{version}-{node_platform}-{node_arch}"
    node_bin = source_root / "bin" / "node"
    node_bin.parent.mkdir(parents=True)
    executable(node_bin, f"#!/bin/sh\nprintf '%s\\n' '{version}'\n")
    executable(
        source_root / "bin" / "npm",
        """#!/bin/sh
set -eu
case "$*" in *"pnpm@>=11.24.0"*) ;; *) exit 8 ;; esac
prefix=
while [ "$#" -gt 0 ]; do
    if [ "$1" = "--prefix" ]; then prefix=$2; shift 2; else shift; fi
done
mkdir -p "$prefix/bin"
printf '#!/bin/sh\\nprintf "%%s\\\\n" "12.1.0"\\n' > "$prefix/bin/pnpm"
chmod +x "$prefix/bin/pnpm"
""",
    )
    archive = release / archive_name
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(source_root, arcname=source_root.name)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if not valid_checksum:
        digest = "0" * 64
    (release / "SHASUMS256.txt").write_text(f"{digest}  {archive_name}\n", encoding="utf-8")
    return root.as_uri()


def rustup_target() -> str:
    """把当前 Unix 测试宿主映射为官方 rustup-init target，覆盖 macOS 与常见 Linux libc。"""
    machine = platform.machine().lower()
    architecture = {"x86_64": "x86_64", "amd64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}[machine]
    if platform.system() == "Darwin":
        return f"{architecture}-apple-darwin"
    libc = "musl" if platform.libc_ver()[0].lower() == "musl" else "gnu"
    return f"{architecture}-unknown-linux-{libc}"


def make_rust_dist(root: Path, *, succeeds: bool = True, valid_checksum: bool = True) -> str:
    """生成带摘要的隔离 rustup 发行镜像，控制安装结果且只写测试工具目录。"""
    release = root / rustup_target()
    release.mkdir(parents=True)
    path = release / "rustup-init"
    if succeeds:
        body = """#!/bin/sh
set -eu
mkdir -p "$CARGO_HOME/bin" "$RUSTUP_HOME"
for tool in rustc cargo rustup; do
    case "$tool" in
        rustc) version='rustc 1.95.0 (test)' ;;
        cargo) version='cargo 1.95.0 (test)' ;;
        rustup) version='rustup 1.28.0 (test)' ;;
    esac
    printf '#!/bin/sh\\nprintf "%%s\\\\n" "%s"\\n' "$version" > "$CARGO_HOME/bin/$tool"
    chmod +x "$CARGO_HOME/bin/$tool"
done
"""
    else:
        body = "#!/bin/sh\nexit 9\n"
    executable(path, body)
    digest = hashlib.sha256(path.read_bytes()).hexdigest() if valid_checksum else "0" * 64
    (release / "rustup-init.sha256").write_text(f"{digest}  rustup-init\n", encoding="utf-8")
    return root.as_uri()


class PrerequisiteGateTests(unittest.TestCase):
    """验证开发环境门禁的无修改成功路径与最高风险供应链失败路径。"""

    def run_gate(self, root: Path, *args: str, probe: Path | None = None, **extra: str) -> subprocess.CompletedProcess[str]:
        """在独立 HOME 和探测路径运行门禁，禁止读取或修改机器真实环境。"""
        probe_path = probe or root / "probe"
        probe_path.mkdir(parents=True, exist_ok=True)
        env = os.environ.copy()
        env.update(
            {
                "AFH_PREREQ_PATH": str(probe_path),
                "HOME": str(root / "home"),
                "CARGO_HOME": str(root / "cargo"),
                "RUSTUP_HOME": str(root / "rustup"),
                "AFH_NODE_HOME": str(root / "node-home"),
            }
        )
        env.update(extra)
        return subprocess.run(
            ["/bin/sh", str(SCRIPT), *args],
            text=True,
            capture_output=True,
            env=env,
            timeout=30,
            check=False,
        )

    def test_gui_toolchain_is_always_required(self) -> None:
        """GUI-only 初始化必须同时检查 Rust、Node.js 与 pnpm。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe)
            result = self.run_gate(root, "--install-missing", probe=probe)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.rust.change=existing", result.stdout)
            self.assertIn("gate.git.status=passed", result.stdout)
            self.assertIn("gate.git.change=existing", result.stdout)
            self.assertIn("gate.node.status=passed", result.stdout)
            self.assertIn("gate.pnpm.status=passed", result.stdout)
            self.assertIn("gate.changed=false", result.stdout)

    def test_existing_tools_support_spaces_in_probe_path(self) -> None:
        """工具目录包含空格时仍应按完整路径执行，避免常见用户目录导致误判缺失。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe with spaces"
            fake_existing_tools(probe)
            result = self.run_gate(root, "--install-missing", probe=probe)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.changed=false", result.stdout)

    def test_newer_stable_rust_versions_satisfy_the_minimum(self) -> None:
        """高于 1.95.0 的稳定版本与未来主版本必须通过，避免把 MSRV 误作精确版本锁。"""
        for rust_version in ("1.96.0", "1.97.1", "2.0.0"):
            with self.subTest(rust_version=rust_version), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(probe, rust=rust_version)
                result = self.run_gate(root, "--check-only", probe=probe)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f"rustc {rust_version} (test)", result.stdout)

    def test_newer_git_versions_satisfy_the_minimum_without_replacement(self) -> None:
        """Git 兼容要求是最低要求，现有更高稳定版本必须原样复用。"""
        for git_version in ("2.39.0", "2.51.0", "3.0.0"):
            with self.subTest(git_version=git_version), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(probe, git=git_version)
                result = self.run_gate(root, "--check-only", probe=probe)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("gate.git.requirement=>=2.0.0", result.stdout)
                self.assertIn(f"gate.git.version=git version {git_version}", result.stdout)

    def test_missing_git_is_installed_and_reprobed(self) -> None:
        """初始化模式必须通过宿主受管包管理器安装缺失 Git 并输出 installed。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, include_git=False)
            fake_git_package_manager(probe)
            result = self.run_gate(root, "--install-missing", probe=probe)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.git.status=passed", result.stdout)
            self.assertIn("gate.git.version=git version 2.51.0", result.stdout)
            self.assertIn("gate.git.change=installed", result.stdout)
            self.assertTrue((probe / "git").is_file())

    def test_existing_gui_tools_are_not_modified(self) -> None:
        """GUI 项目已有 Rust、Node.js 与 pnpm 时应全部通过且不修改环境。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, python=False)
            fake_frontend_tools(probe)
            result = self.run_gate(root, "--install-missing", probe=probe)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.rust.status=passed", result.stdout)
            self.assertIn("gate.node.status=passed", result.stdout)
            self.assertIn("gate.node.requirement=^24.15.0 || >=26.0.0", result.stdout)
            self.assertIn("gate.pnpm.status=passed", result.stdout)
            self.assertIn("gate.pnpm.requirement=>=11.24.0", result.stdout)
            self.assertIn("gate.changed=false", result.stdout)

    def test_newer_compatible_frontend_tools_are_preserved(self) -> None:
        """高于下界的 Node.js 与 pnpm 仍应通过，兼容要求不能退化为精确版本锁。"""
        for node_version, pnpm_version in (("24.15.0", "11.24.0"), ("26.7.0", "12.3.0")):
            with self.subTest(node=node_version, pnpm=pnpm_version):
                with tempfile.TemporaryDirectory() as temporary:
                    root = Path(temporary)
                    probe = root / "probe"
                    fake_existing_tools(probe, python=False)
                    fake_frontend_tools(probe, node=node_version, pnpm=pnpm_version)
                    result = self.run_gate(
                        root,
                        "--install-missing",
                        probe=probe,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(f"gate.node.version=v{node_version}", result.stdout)
                    self.assertIn(f"gate.pnpm.version={pnpm_version}", result.stdout)
                    self.assertIn("gate.changed=false", result.stdout)

    def test_incompatible_node_versions_are_rejected(self) -> None:
        """低于 24.x 下界、23.x 或 25.x 空档版本必须失败关闭。"""
        for node_version in ("23.11.9", "24.14.9", "25.9.0"):
            with self.subTest(node_version=node_version), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(probe, python=False)
                fake_frontend_tools(probe, node=node_version)
                result = self.run_gate(root, "--check-only", probe=probe)
                self.assertEqual(result.returncode, 23)
                self.assertIn("不满足兼容范围", result.stderr)

    def test_incompatible_pnpm_is_rejected(self) -> None:
        """pnpm 低于 11.24.0 时不得因命令可调用而通过。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, python=False)
            fake_frontend_tools(probe, pnpm="11.23.9")
            result = self.run_gate(root, "--check-only", probe=probe)
            self.assertEqual(result.returncode, 28)
            self.assertIn("低于兼容下界", result.stderr)

    def test_missing_gui_toolchain_is_installed_in_isolation(self) -> None:
        """GUI 缺失 Git、Rust、Node.js 与 pnpm 时应全部安装并复探。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            rust_dist = make_rust_dist(root / "rustup-dist")
            node_dist = make_node_dist(root / "dist")
            probe = root / "probe"
            probe.mkdir()
            fake_git_package_manager(probe)
            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_RUSTUP_DIST_BASE=rust_dist,
                AFH_NODE_DIST_BASE=node_dist,
                AFH_ALLOW_FILE_URLS="1",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.git.change=installed", result.stdout)
            self.assertIn("gate.rust.change=installed", result.stdout)
            self.assertIn("gate.node.change=installed", result.stdout)
            self.assertIn("gate.pnpm.change=installed", result.stdout)
            self.assertIn("gate.changed=true", result.stdout)
            self.assertTrue((root / "cargo" / "bin" / "cargo").is_file())
            self.assertIn("gate.node.version=v24.15.0", result.stdout)
            self.assertIn("gate.pnpm.version=12.1.0", result.stdout)
            self.assertTrue((root / "node-home" / "v24.15.0" / "bin" / "node").is_file())
            self.assertTrue((root / "home" / ".local" / "share" / "agent-first-pnpm" / "bin" / "pnpm").is_file())

    def test_check_only_reports_missing_without_installing(self) -> None:
        """只读模式必须以退出码 20 报告缺失，且不得创建安装目录。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.run_gate(root, "--check-only")
            self.assertEqual(result.returncode, 20)
            self.assertIn("gate.rust.status=missing", result.stdout)
            self.assertIn("gate.git.status=missing", result.stdout)
            self.assertIn("gate.node.status=missing", result.stdout)
            self.assertIn("gate.pnpm.status=missing", result.stdout)
            self.assertFalse((root / "cargo").exists())

    def test_incompatible_existing_rust_is_not_replaced(self) -> None:
        """已有 Rust 低于 MSRV 时必须阻断，不能借自动安装进行静默替换。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, rust="1.94.9")
            result = self.run_gate(root, "--install-missing", probe=probe)
            self.assertEqual(result.returncode, 21)
            self.assertIn("低于 MSRV", result.stderr)

    def test_rust_installer_failure_blocks_the_gate(self) -> None:
        """Rust 安装器失败必须保留非零结论，不能继续生成项目。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe)
            (probe / "rustc").unlink()
            (probe / "cargo").unlink()
            rust_dist = make_rust_dist(root / "rustup-dist", succeeds=False)
            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_RUSTUP_DIST_BASE=rust_dist,
                AFH_ALLOW_FILE_URLS="1",
            )
            self.assertEqual(result.returncode, 22)
            self.assertIn("Rust 安装失败", result.stderr)

    def test_rust_checksum_mismatch_blocks_the_gate(self) -> None:
        """rustup-init 摘要不匹配时必须在执行安装器前阻断，防止未验证制品运行。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe)
            (probe / "rustc").unlink()
            (probe / "cargo").unlink()
            rust_dist = make_rust_dist(root / "rustup-dist", valid_checksum=False)
            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_RUSTUP_DIST_BASE=rust_dist,
                AFH_ALLOW_FILE_URLS="1",
            )
            self.assertEqual(result.returncode, 22)
            self.assertIn("rustup-init SHA-256 校验失败", result.stderr)
            self.assertFalse((root / "cargo").exists())

    def test_node_checksum_mismatch_blocks_the_gate(self) -> None:
        """Node 制品摘要不匹配时必须拒绝安装，防止不可信下载进入 PATH。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, python=False, frontend=False)
            node_dist = make_node_dist(root / "dist", valid_checksum=False)
            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_NODE_DIST_BASE=node_dist,
                AFH_ALLOW_FILE_URLS="1",
            )
            self.assertEqual(result.returncode, 26)
            self.assertIn("SHA-256 校验失败", result.stderr)

    def test_legacy_interface_argument_is_rejected(self) -> None:
        """已删除的接口选择参数必须被拒绝，避免恢复多接口分支。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.run_gate(root, "--check-only", "--interfaces", "GUI")
            self.assertEqual(result.returncode, 2)
            self.assertIn("未知参数：--interfaces", result.stderr)

    def test_windows_msvc_gate_installs_signed_build_tools(self) -> None:
        """Windows 脚本必须验证微软签名、安装 C++ 工作负载并在成功前重新探测。"""
        text = WINDOWS_SCRIPT.read_text(encoding="utf-8")
        required = (
            "https://aka.ms/vs/17/release/vs_BuildTools.exe",
            "Get-AuthenticodeSignature",
            "Microsoft Corporation",
            "Microsoft.VisualStudio.Workload.VCTools",
            "Install-MissingMsvc",
            "Install-MissingPnpm",
            "Install-MissingGit",
            "Test-GitVersion",
            "Git.Git",
            '"gate.git.status=passed"',
            '"gate.git.change=$GitChange"',
            "if (-not (Test-MsvcPrerequisite))",
            '"gate.msvc.status=passed"',
            '"gate.msvc.change=$MsvcChange"',
            "$MinimumRustMinor = 95",
            '$NodeRequirement = "^24.15.0 || >=26.0.0"',
            '$PnpmRequirement = ">=11.24.0"',
            '$PnpmInstallRequirement = "pnpm@>=11.24.0"',
            "Test-NodeVersion",
            "Test-PnpmVersion",
        )
        for fragment in required:
            self.assertIn(fragment, text)
        self.assertNotIn("$Interfaces", text)
        self.assertNotIn("$FrontendRequired", text)
        self.assertNotIn("pnpm@latest", text)


if __name__ == "__main__":
    unittest.main()
