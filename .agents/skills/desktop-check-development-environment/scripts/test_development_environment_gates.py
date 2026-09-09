#!/usr/bin/env python3
from __future__ import annotations

import hashlib
import os
import platform
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("development-environment-gates.sh")
WINDOWS_SCRIPT = Path(__file__).with_name("development-environment-gates.ps1")
POSIX_SHELL = shutil.which("sh")
GIT_BASH = next(
    (
        candidate
        for candidate in (
            Path(r"C:\Program Files\Git\bin\sh.exe"),
            Path(r"C:\Program Files\Git\usr\bin\sh.exe"),
        )
        if candidate.is_file()
    ),
    None,
)


def shell_path(path: Path) -> str:
    """把 Windows 绝对路径转换为 Git Bash 可用形式，POSIX 宿主保持原样。"""
    resolved = path.resolve()
    if os.name != "nt":
        return str(resolved)
    return f"/{resolved.drive[0].lower()}{resolved.as_posix()[2:]}"


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
    executable(
        bin_dir / "rustc",
        f"#!/bin/sh\n"
        f"if [ \"${{1:-}}\" = -vV ]; then\n"
        f"  printf '%s\\n' 'rustc {rust} (test)' 'host: x86_64-unknown-linux-gnu' 'release: {rust}'\n"
        f"else\n"
        f"  printf '%s\\n' 'rustc {rust} (test)'\n"
        f"fi\n",
    )
    executable(bin_dir / "cargo", f"#!/bin/sh\nprintf '%s\\n' 'cargo {rust} (test)'\n")
    executable(bin_dir / "rustup", "#!/bin/sh\nprintf '%s\\n' 'rustup 1.28.0 (test)'\n")
    if include_git:
        executable(bin_dir / "git", f"#!/bin/sh\nprintf '%s\\n' 'git version {git}'\n")
    if python:
        executable(bin_dir / "python3", "#!/bin/sh\nprintf '%s\\n' 'Python 3.12.0'\n")
    if frontend:
        fake_frontend_tools(bin_dir)


def fake_npm_installer(bin_dir: Path, *, pnpm: str = "12.1.0", succeeds: bool = True) -> None:
    """构造 npm 用户级安装器，使 pnpm 安装与升级都只写入测试目录。"""
    if not succeeds:
        executable(
            bin_dir / "npm",
            "#!/bin/sh\n"
            "if [ \"${1:-}\" = --version ]; then printf '%s\\n' '11.6.0'; exit 0; fi\n"
            "exit 9\n",
        )
        return
    executable(
        bin_dir / "npm",
        f"""#!/bin/sh
set -eu
if [ "${1:-}" = --version ]; then printf '%s\n' '11.6.0'; exit 0; fi
case "$*" in *"pnpm@>=11.24.0"*) ;; *) exit 8 ;; esac
prefix=
registry=
ignore_scripts=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --prefix) prefix=$2; shift 2 ;;
    --registry) registry=$2; shift 2 ;;
    --ignore-scripts) ignore_scripts=1; shift ;;
    *) shift ;;
  esac
done
[ "$registry" = "${{AFH_PNPM_REGISTRY:-https://registry.npmjs.org/}}" ] || exit 7
[ "$ignore_scripts" -eq 1 ] || exit 6
mkdir -p "$prefix/bin"
printf '#!/bin/sh\\nprintf "%%s\\\\n" "{pnpm}"\\n' > "$prefix/bin/pnpm"
chmod +x "$prefix/bin/pnpm"
""",
    )


def fake_frontend_tools(bin_dir: Path, *, node: str = "24.15.0", pnpm: str = "11.24.0") -> None:
    """构造既有 Node.js 与 pnpm，验证前端门禁不会修改已满足的环境。"""
    executable(bin_dir / "node", f"#!/bin/sh\nprintf '%s\\n' 'v{node}'\n")
    executable(bin_dir / "pnpm", f"#!/bin/sh\nprintf '%s\\n' '{pnpm}'\n")
    fake_npm_installer(bin_dir)


def fake_git_package_manager(bin_dir: Path, *, git: str = "2.51.0") -> None:
    """构造当前 Unix 宿主的受管包管理器，使 Git 安装或升级后可复探。"""
    git_body = f"#!/bin/sh\nprintf '%s\\n' 'git version {git}'\n"
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


def make_node_dist(
    root: Path,
    *,
    valid_checksum: bool = True,
    forced_tuple: tuple[str, str] | None = None,
    version: str = "v24.15.0",
    reported_version: str | None = None,
) -> str:
    """生成最小本地 Node 镜像，验证跳过 25.x 并选择最新兼容稳定版。"""
    reported_version = reported_version or version
    node_platform, node_arch = forced_tuple or node_tuple()
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
    executable(node_bin, f"#!/bin/sh\nprintf '%s\\n' '{reported_version}'\n")
    fake_npm_installer(source_root / "bin")
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


def make_rust_dist(
    root: Path,
    *,
    installed_version: str = "1.95.0",
    succeeds: bool = True,
    valid_checksum: bool = True,
    forced_target: str | None = None,
) -> str:
    """生成带摘要的隔离 rustup 发行镜像，控制安装结果且只写测试工具目录。"""
    release = root / (forced_target or rustup_target())
    release.mkdir(parents=True)
    path = release / "rustup-init"
    if succeeds:
        body = f"""#!/bin/sh
set -eu
mkdir -p "$CARGO_HOME/bin" "$RUSTUP_HOME"
for tool in rustc cargo rustup; do
    case "$tool" in
        rustc)
            printf '#!/bin/sh\nif [ "${{1:-}}" = -vV ]; then\n  printf "%%s\\n" "rustc {installed_version} (test)" "host: x86_64-unknown-linux-gnu" "release: {installed_version}"\nelse\n  printf "%%s\\n" "rustc {installed_version} (test)"\nfi\n' > "$CARGO_HOME/bin/$tool"
            chmod +x "$CARGO_HOME/bin/$tool"
            continue
            ;;
        cargo) version='cargo {installed_version} (test)' ;;
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

    def setUp(self) -> None:
        """Windows 或缺少 POSIX shell 时跳过 Shell 行为用例，保留 PowerShell 静态契约。"""
        platform_independent_tests = {
            "test_windows_msvc_gate_installs_signed_build_tools",
            "test_persistence_temp_files_use_unpredictable_mktemp_names",
            "test_git_bash_full_install_uses_managed_global_roots",
        }
        if self._testMethodName not in platform_independent_tests and (
            os.name == "nt" or POSIX_SHELL is None
        ):
            self.skipTest("POSIX gate behavior requires a non-Windows host with sh")

    def run_gate(
        self,
        root: Path,
        *args: str,
        probe: Path | None = None,
        test_mode: bool = True,
        **extra: str,
    ) -> subprocess.CompletedProcess[str]:
        """在独立 HOME 和探测路径运行门禁，禁止读取或修改机器真实环境。"""
        probe_path = probe or root / "probe"
        probe_path.mkdir(parents=True, exist_ok=True)
        (root / "home").mkdir(parents=True, exist_ok=True)
        login_shell = root / "login-shell" / "sh"
        login_shell.parent.mkdir(parents=True, exist_ok=True)
        executable(
            login_shell,
            "#!/bin/sh\n"
            "[ \"${1:-}\" = -l ] && [ \"${2:-}\" = -c ] || exit 90\n"
            "[ ! -f \"$HOME/.profile\" ] || . \"$HOME/.profile\"\n"
            f'exec "{POSIX_SHELL}" -c "$3"\n',
        )
        env = os.environ.copy()
        env.update(
            {
                "AFH_PREREQ_PATH": str(probe_path),
                "HOME": str(root / "home"),
                "CARGO_HOME": str(root / "project" / ".cargo"),
                "RUSTUP_HOME": str(root / "project" / ".rustup"),
                "AFH_MANAGED_CARGO_HOME": str(root / "cargo"),
                "AFH_MANAGED_RUSTUP_HOME": str(root / "rustup"),
                "AFH_NODE_HOME": str(root / "node-home"),
                "SHELL": str(login_shell),
                "PATH": f"{probe_path}:{env.get('PATH', '')}",
            }
        )
        if test_mode:
            env["AFH_TEST_MODE"] = "1"
        else:
            env.pop("AFH_TEST_MODE", None)
        env.update(extra)
        return subprocess.run(
            [str(POSIX_SHELL), str(SCRIPT), *args],
            text=True,
            capture_output=True,
            cwd=root,
            env=env,
            timeout=30,
            check=False,
        )

    def test_missing_rustup_blocks_even_when_rustc_and_cargo_exist(self) -> None:
        """rustup 缺失时不得仅凭 rustc/cargo 误报 Rust 门禁通过。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe)
            (probe / "rustup").unlink()
            result = self.run_gate(root, "--check-only", probe=probe)
            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.rust.status=missing", result.stdout)
            self.assertIn("gate.rust.rustup_version=Missing", result.stdout)

    def test_rustc_verbose_release_mismatch_fails_closed(self) -> None:
        """rustc -vV 与简版版本不一致时必须按损坏工具链失败关闭。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe)
            executable(
                probe / "rustc",
                "#!/bin/sh\n"
                "if [ \"${1:-}\" = -vV ]; then\n"
                "  printf '%s\\n' 'rustc 1.95.0 (test)' 'host: x86_64-unknown-linux-gnu' 'release: 1.94.0'\n"
                "else\n"
                "  printf '%s\\n' 'rustc 1.95.0 (test)'\n"
                "fi\n",
            )
            result = self.run_gate(root, "--check-only", probe=probe)
            self.assertEqual(result.returncode, 21)
            self.assertIn("release 与 rustc --version 不一致", result.stderr)

    def test_rust_tool_names_and_verbose_host_are_strict(self) -> None:
        """Rust 三工具必须报告自身名称，且 rustc -vV 只能含一个可解析 host。"""
        cases = ("wrong-tool", "duplicate-host")
        for case in cases:
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(probe)
                if case == "wrong-tool":
                    executable(probe / "rustup", "#!/bin/sh\nprintf '%s\\n' 'cargo 1.28.0 (test)'\n")
                else:
                    executable(
                        probe / "rustc",
                        "#!/bin/sh\n"
                        "if [ \"${1:-}\" = -vV ]; then\n"
                        "  printf '%s\\n' 'rustc 1.95.0 (test)' 'host: x86_64-unknown-linux-gnu' "
                        "'host: injected' 'release: 1.95.0'\n"
                        "else\n  printf '%s\\n' 'rustc 1.95.0 (test)'\nfi\n",
                    )
                result = self.run_gate(root, "--check-only", probe=probe)
                self.assertEqual(result.returncode, 21)
                self.assertIn("错误：", result.stderr)

    def test_cargo_must_match_the_rustc_stable_line(self) -> None:
        """不同 stable minor 的 rustc/cargo 混合 PATH 必须按损坏工具链失败关闭。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, rust="1.96.0")
            executable(probe / "cargo", "#!/bin/sh\nprintf '%s\\n' 'cargo 1.95.0 (test)'\n")

            result = self.run_gate(root, "--check-only", probe=probe)

            self.assertEqual(result.returncode, 21, result.stderr)
            self.assertIn("不属于同一 stable 工具链", result.stderr)

    def test_unsupported_login_shell_fails_before_installation(self) -> None:
        """未知登录 shell 必须在 Git/Rust/Node 写入发生前失败，不能留下半配置工具链。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            probe.mkdir()
            unsupported_shell = root / "unsupported-shell"
            executable(unsupported_shell, "#!/bin/sh\nexit 0\n")
            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                SHELL=str(unsupported_shell),
            )
            self.assertEqual(result.returncode, 24)
            self.assertIn("不支持自动持久化 PATH", result.stderr)
            self.assertFalse((root / "cargo").exists())
            self.assertFalse((root / "node-home").exists())

    def test_user_tool_directory_symlink_component_fails_before_installation(self) -> None:
        """稳定用户 bin 的任一路径组件为符号链接时必须在下载前失败。"""

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            home = root / "home"
            home.mkdir()
            redirected = root / "redirected-local"
            redirected.mkdir()
            (home / ".local").symlink_to(redirected, target_is_directory=True)
            probe = root / "probe"
            fake_existing_tools(probe)
            (probe / "rustup").unlink()

            result = self.run_gate(root, "--install-missing", probe=probe)

            self.assertEqual(result.returncode, 24, result.stderr)
            self.assertIn("工具目录组件不能是符号链接", result.stderr)
            self.assertFalse((root / "cargo").exists())
            self.assertEqual(list(redirected.iterdir()), [])

    def test_unmanaged_user_tool_symlink_is_never_replaced(self) -> None:
        """同名链接只有指向当前受管安装根时才允许原子更新。"""

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, python=False, frontend=False)
            fake_npm_installer(probe)
            node_dist = make_node_dist(root / "dist")
            user_bin = root / "home" / ".local" / "share" / "agent-first-harness" / "bin"
            user_bin.mkdir(parents=True)
            outside = root / "outside-node"
            executable(outside, "#!/bin/sh\nprintf '%s\\n' 'v99.0.0'\n")
            destination = user_bin / "node"
            destination.symlink_to(outside)
            (root / "node-home").mkdir()

            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_NODE_DIST_BASE=node_dist,
                AFH_ALLOW_FILE_URLS="1",
            )

            self.assertEqual(result.returncode, 24, result.stderr)
            self.assertIn("不属于当前受管安装根", result.stderr)
            self.assertTrue(destination.is_symlink())
            self.assertEqual(destination.resolve(), outside.resolve())
            self.assertEqual(list((root / "node-home").iterdir()), [])

    def test_profile_and_managed_config_conflicts_fail_before_download(self) -> None:
        """所有将写入的 profile/env/fish 路径必须在下载前完成形态与 marker 预检。"""
        for case in ("env-marker", "profile-symlink", "fish-marker"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                home = root / "home"
                home.mkdir()
                probe = root / "probe"
                fake_existing_tools(probe)
                (probe / "rustup").unlink()
                shell_override = str(POSIX_SHELL)
                if case == "env-marker":
                    env_dir = home / ".config" / "agent-first-harness"
                    env_dir.mkdir(parents=True)
                    (env_dir / "env.sh").write_text("unmanaged\n", encoding="utf-8")
                elif case == "profile-symlink":
                    outside = root / "outside-profile"
                    outside.write_text("unchanged\n", encoding="utf-8")
                    (home / ".profile").symlink_to(outside)
                else:
                    fake_fish = root / "fish"
                    executable(fake_fish, "#!/bin/sh\nexit 0\n")
                    shell_override = str(fake_fish)
                    fish_dir = home / ".config" / "fish" / "conf.d"
                    fish_dir.mkdir(parents=True)
                    (fish_dir / "agent-first-harness.fish").write_text("unmanaged\n", encoding="utf-8")

                result = self.run_gate(
                    root,
                    "--install-missing",
                    probe=probe,
                    SHELL=shell_override,
                    AFH_RUSTUP_DIST_BASE=(root / "unreachable-rust-dist").as_uri(),
                    AFH_ALLOW_FILE_URLS="1",
                )

                self.assertEqual(result.returncode, 24, result.stderr)
                self.assertFalse((root / "cargo").exists())

    def test_managed_rust_root_symlink_fails_before_download(self) -> None:
        """受管 Rust 根自身或中间组件为 symlink 时不得启动下载器或安装器。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "home").mkdir()
            outside = root / "outside-cargo"
            outside.mkdir()
            (root / "cargo").symlink_to(outside, target_is_directory=True)
            probe = root / "probe"
            fake_existing_tools(probe)
            (probe / "rustup").unlink()

            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_RUSTUP_DIST_BASE=(root / "unreachable-rust-dist").as_uri(),
                AFH_ALLOW_FILE_URLS="1",
            )

            self.assertEqual(result.returncode, 24, result.stderr)
            self.assertIn("路径组件不能是符号链接", result.stderr)
            self.assertEqual(list(outside.iterdir()), [])

    def test_existing_node_version_requires_marker_and_contained_executables(self) -> None:
        """既有同版本目录只有带受管 marker 且最终可执行目标仍在同根内时才可复用。"""
        for case in ("missing-marker", "escaping-node", "wrong-version"):
            with self.subTest(case=case), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(probe)
                fake_frontend_tools(probe)
                (probe / "node").unlink()
                (probe / "npm").unlink()
                selected_version = "v26.7.0" if case == "wrong-version" else "v24.15.0"
                node_dist = make_node_dist(root / "dist", version=selected_version)
                archive = next((root / "dist" / selected_version).glob("*.tar.gz"))
                archive_digest = hashlib.sha256(archive.read_bytes()).hexdigest()
                archive.unlink()
                install_bin = root / "node-home" / selected_version / "bin"
                install_bin.mkdir(parents=True)
                installed_report = "v24.15.0" if case == "wrong-version" else selected_version
                executable(install_bin / "node", f"#!/bin/sh\nprintf '%s\\n' '{installed_report}'\n")
                fake_npm_installer(install_bin)
                if case != "missing-marker":
                    (root / "node-home" / selected_version / ".agent-first-harness-managed").write_text(
                        "# managed by agent-first-harness development environment gate\n"
                        f"node.version={selected_version}\n"
                        f"node.archive.sha256={archive_digest}\n",
                        encoding="utf-8",
                    )
                if case == "escaping-node":
                    outside = root / "outside-node"
                    executable(outside, f"#!/bin/sh\nprintf '%s\\n' '{selected_version}'\n")
                    (install_bin / "node").unlink()
                    (install_bin / "node").symlink_to(outside)

                result = self.run_gate(
                    root,
                    "--install-missing",
                    probe=probe,
                    AFH_NODE_DIST_BASE=node_dist,
                    AFH_ALLOW_FILE_URLS="1",
                )

                self.assertIn(result.returncode, (24, 26), result.stderr)
                if case == "wrong-version":
                    self.assertIn("版本与已选择稳定版不一致", result.stderr)
                self.assertFalse((root / "home" / ".local" / "share" / "agent-first-harness" / "bin" / "node").exists())

    def test_persistence_temp_files_use_unpredictable_mktemp_names(self) -> None:
        """env/fish/verifier/link 临时对象不得使用可预置并跟随的 PID 文件名。"""
        source = SCRIPT.read_text(encoding="utf-8")
        self.assertNotIn("tmp.$$", source)
        for fragment in (
            '.link-$tool_name.XXXXXX',
            '.agent-first-harness.fish.tmp.XXXXXX',
            '.env.sh.tmp.XXXXXX',
            '.$profile_name.agent-first-harness.tmp.XXXXXX',
            '.$profile_name.agent-first-harness.snapshot.XXXXXX',
            'mktemp -d',
            '$FRESH_VERIFY_DIR/check.XXXXXX',
        ):
            self.assertIn(fragment, source)
        self.assertNotIn('>> "$profile_path"', source)
        self.assertIn('cmp -s "$profile_path" "$profile_snapshot"', source)

    def test_profile_atomic_update_does_not_overwrite_concurrent_change(self) -> None:
        """profile 快照后若原文件变化，门禁必须保留并发字节并拒绝原子替换。"""
        real_cmp = shutil.which("cmp")
        if real_cmp is None:
            self.skipTest("原子 profile 并发夹具需要 cmp")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            home = root / "home"
            home.mkdir()
            profile = home / ".profile"
            profile.write_text("owner\n", encoding="utf-8")
            probe = root / "probe"
            fake_existing_tools(probe)
            for tool in ("rustup", "rustc", "cargo"):
                (probe / tool).unlink()
            rust_dist = make_rust_dist(root / "rustup-dist")
            path_shims = root / "path-shims"
            path_shims.mkdir()
            executable(
                path_shims / "cmp",
                "#!/bin/sh\n"
                "printf '%s\\n' concurrent >> \"$AFH_CONCURRENT_PROFILE\"\n"
                f'exec "{real_cmp}" "$@"\n',
            )

            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_RUSTUP_DIST_BASE=rust_dist,
                AFH_ALLOW_FILE_URLS="1",
                AFH_CONCURRENT_PROFILE=str(profile),
                PATH=f"{path_shims}:{os.environ.get('PATH', '')}",
            )

            self.assertEqual(result.returncode, 24, result.stderr)
            self.assertIn("并发修改", result.stderr)
            self.assertEqual(profile.read_text(encoding="utf-8"), "owner\nconcurrent\n")
            self.assertNotIn("agent-first-harness/env.sh", profile.read_text(encoding="utf-8"))

    def test_only_git_change_requires_exact_fresh_login_shell_discovery(self) -> None:
        """仅 Git 改变也必须从新 login shell 执行同一路径和版本；临时探测路径不可冒充持久环境。"""
        for probe_is_persistent in (True, False):
            with self.subTest(probe_is_persistent=probe_is_persistent), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(probe, git="1.99.9")
                fake_git_package_manager(probe)
                login_dir = root / "login"
                login_dir.mkdir()
                login_shell = login_dir / "sh"
                executable(
                    login_shell,
                    "#!/bin/sh\n"
                    "[ \"${1:-}\" = -l ] && [ \"${2:-}\" = -c ] || exit 90\n"
                    f'exec "{POSIX_SHELL}" -c "$3"\n',
                )
                base_path = os.environ.get("PATH", "")
                if probe_is_persistent:
                    base_path = f"{probe}:{base_path}"

                result = self.run_gate(
                    root,
                    "--install-missing",
                    probe=probe,
                    SHELL=str(login_shell),
                    PATH=base_path,
                )

                if probe_is_persistent:
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn("gate.git.change=upgraded", result.stdout)
                    self.assertIn("gate.fresh_shell.status=passed", result.stdout)
                    self.assertFalse((root / "home" / ".config").exists())
                else:
                    self.assertEqual(result.returncode, 24, result.stderr)
                    self.assertIn("新 shell 无法从持久 PATH", result.stderr)

    def test_git_bash_full_install_uses_managed_global_roots(self) -> None:
        """Windows CI 通过 Git Bash 真运行 Unix 安装、持久 PATH 与新 login shell 复探闭环。"""
        if os.name != "nt" or GIT_BASH is None:
            self.skipTest("仅用于 Windows 上的 Git Bash Unix 门禁回归")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            home = root / "home"
            home.mkdir()
            probe = root / "probe"
            fake_existing_tools(probe)
            for tool in ("git", "rustup", "rustc", "cargo"):
                (probe / tool).unlink()
            executable(
                probe / "uname",
                "#!/bin/sh\n"
                "case \"${1:-}\" in -s) printf '%s\\n' Linux ;; -m) printf '%s\\n' x86_64 ;; *) printf '%s\\n' Linux ;; esac\n",
            )
            executable(probe / "getconf", "#!/bin/sh\nprintf '%s\\n' 'glibc 2.39'\n")
            rust_dist = make_rust_dist(
                root / "rustup-dist",
                forced_target="x86_64-unknown-linux-gnu",
            )
            node_dist = make_node_dist(root / "node-dist", forced_tuple=("linux", "x64"))
            git_usr_bin = Path(r"C:\Program Files\Git\usr\bin")
            git_mingw_bin = Path(r"C:\Program Files\Git\mingw64\bin")
            environment = os.environ.copy()
            environment.update(
                {
                    "AFH_TEST_MODE": "1",
                    "AFH_PREREQ_PATH": f"{shell_path(probe)}:/mingw64/bin:/usr/bin",
                    "AFH_ALLOW_FILE_URLS": "1",
                    "AFH_RUSTUP_DIST_BASE": rust_dist,
                    "AFH_NODE_DIST_BASE": node_dist,
                    "AFH_MANAGED_CARGO_HOME": shell_path(root / "cargo"),
                    "AFH_MANAGED_RUSTUP_HOME": shell_path(root / "rustup"),
                    "AFH_NODE_HOME": shell_path(root / "node-home"),
                    "AFH_PNPM_HOME": shell_path(root / "pnpm-home"),
                    "AFH_TEST_HOST_OS": "Linux",
                    "AFH_TEST_HOST_ARCH": "x86_64",
                    "AFH_TEST_RUST_LIBC": "gnu",
                    "HOME": shell_path(home),
                    "CARGO_HOME": shell_path(root / "project" / ".cargo"),
                    "RUSTUP_HOME": shell_path(root / "project" / ".rustup"),
                    "SHELL": "/bin/sh",
                    "PATH": os.pathsep.join((str(probe), str(git_usr_bin), str(git_mingw_bin))),
                }
            )

            result = subprocess.run(
                [str(GIT_BASH), shell_path(SCRIPT), "--install-missing"],
                text=True,
                capture_output=True,
                cwd=root,
                env=environment,
                timeout=30,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.rust.change=installed", result.stdout)
            self.assertIn("gate.node.change=installed", result.stdout)
            self.assertIn("gate.pnpm.change=installed", result.stdout)
            self.assertTrue((root / "cargo" / "bin" / "cargo").is_file())
            self.assertTrue((root / "node-home" / "v24.15.0" / "bin" / "node").is_file())
            self.assertTrue((root / "pnpm-home" / "bin" / "pnpm").is_file())
            self.assertFalse((root / "project").exists())

    def test_empty_probe_path_segment_never_resolves_cwd_shim(self) -> None:
        """探测 PATH 的空段必须被丢弃，不能执行当前项目目录中的同名 shim。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, include_git=False)
            executable(root / "git", "#!/bin/sh\nprintf '%s\\n' 'git version 9.9.9'\n")
            result = self.run_gate(
                root,
                "--check-only",
                probe=probe,
                AFH_PREREQ_PATH=f":{probe}",
            )
            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.git.status=missing", result.stdout)

    def test_literal_glob_probe_entry_is_not_expanded(self) -> None:
        """绝对 PATH 字面 glob 不能扫描匹配目录并执行其中的项目 shim。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, include_git=False)
            glob_match = root / "glob-match"
            glob_match.mkdir()
            executable(glob_match / "git", "#!/bin/sh\nprintf '%s\\n' 'git version 9.9.9'\n")

            result = self.run_gate(
                root,
                "--check-only",
                probe=probe,
                AFH_PREREQ_PATH=f"{root}/*:{probe}",
            )

            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.git.status=missing", result.stdout)

    def test_test_overrides_require_explicit_test_mode(self) -> None:
        """任何探测或下载覆盖在未显式启用测试模式时都必须先失败关闭。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.run_gate(root, "--check-only", test_mode=False)
            self.assertEqual(result.returncode, 2)
            self.assertIn("仅在 AFH_TEST_MODE=1", result.stderr)

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
        for git_version in ("2.0.0", "2.39.0", "2.51.0", "3.0.0"):
            with self.subTest(git_version=git_version), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(probe, git=git_version)
                result = self.run_gate(root, "--check-only", probe=probe)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("gate.git.requirement=>=2.0.0", result.stdout)
                self.assertIn(f"gate.git.version=git version {git_version}", result.stdout)

    def test_git_below_supported_floor_requires_upgrade_in_check_only(self) -> None:
        """Git 1.99.9 低于 GUI Harness 保留的兼容下界，只读门禁必须要求升级。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, git="1.99.9")
            result = self.run_gate(root, "--check-only", probe=probe)
            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.git.status=upgrade-required", result.stdout)
            self.assertIn("gate.git.requirement=>=2.0.0", result.stdout)
            self.assertIn("gate.git.version=git version 1.99.9", result.stdout)
            self.assertEqual(result.stderr, "")

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

    def test_lower_node_versions_require_upgrade_in_check_only(self) -> None:
        """低于 24.15.0 与 25.x 空档版本应报告升级需求，且只读模式不写入。"""
        for node_version in ("23.11.9", "24.14.9", "25.9.0"):
            with self.subTest(node_version=node_version), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(probe, python=False)
                fake_frontend_tools(probe, node=node_version)
                result = self.run_gate(root, "--check-only", probe=probe)
                self.assertEqual(result.returncode, 20, result.stderr)
                self.assertIn("gate.node.status=upgrade-required", result.stdout)
                self.assertIn(f"gate.node.version=v{node_version}", result.stdout)
                self.assertIn("gate.pnpm.status=passed", result.stdout)
                self.assertEqual(result.stderr, "")
                self.assertFalse((root / "node-home").exists())

    def test_lower_pnpm_requires_upgrade_in_check_only(self) -> None:
        """pnpm 低于 11.24.0 时应报告升级需求，且只读模式不得安装。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, python=False)
            fake_frontend_tools(probe, pnpm="11.23.9")
            result = self.run_gate(root, "--check-only", probe=probe)
            self.assertEqual(result.returncode, 20, result.stderr)
            self.assertIn("gate.node.status=passed", result.stdout)
            self.assertIn("gate.pnpm.status=upgrade-required", result.stdout)
            self.assertIn("gate.pnpm.version=11.23.9", result.stdout)
            self.assertEqual(result.stderr, "")
            self.assertFalse((root / "home" / ".local" / "share" / "agent-first-pnpm").exists())

    def test_check_only_reports_all_lower_versions_without_installing(self) -> None:
        """只读模式必须汇总全部可证明的低版本，并以 20 退出且保持零写入。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, rust="1.94.9", git="1.99.9", python=False)
            fake_frontend_tools(probe, node="25.9.0", pnpm="11.23.9")
            result = self.run_gate(root, "--check-only", probe=probe)
            self.assertEqual(result.returncode, 20, result.stderr)
            for tool in ("git", "rust", "node", "pnpm"):
                self.assertIn(f"gate.{tool}.status=upgrade-required", result.stdout)
            self.assertEqual(result.stderr, "")
            self.assertFalse((root / "cargo").exists())
            self.assertFalse((root / "node-home").exists())
            self.assertEqual(list((root / "home").iterdir()), [])

    def test_lower_gui_toolchain_is_upgraded_and_reprobed(self) -> None:
        """初始化模式应沿既有受管路径升级全部低版本，并逐项输出 upgraded。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, rust="1.94.9", git="1.99.9", python=False)
            fake_frontend_tools(probe, node="25.9.0", pnpm="11.23.9")
            fake_git_package_manager(probe)
            rust_dist = make_rust_dist(root / "rustup-dist")
            node_dist = make_node_dist(root / "dist")
            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_RUSTUP_DIST_BASE=rust_dist,
                AFH_NODE_DIST_BASE=node_dist,
                AFH_PNPM_REGISTRY="https://registry.example.invalid/",
                AFH_ALLOW_FILE_URLS="1",
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            for tool in ("git", "rust", "node", "pnpm"):
                self.assertIn(f"gate.{tool}.status=passed", result.stdout)
                self.assertIn(f"gate.{tool}.change=upgraded", result.stdout)
            self.assertIn("gate.git.version=git version 2.51.0", result.stdout)
            self.assertIn("gate.rust.version=rustc 1.95.0 (test)", result.stdout)
            self.assertIn("gate.node.version=v24.15.0", result.stdout)
            self.assertIn("gate.pnpm.version=12.1.0", result.stdout)
            self.assertIn("gate.changed=true", result.stdout)

    def test_prerelease_versions_are_rejected_without_upgrade(self) -> None:
        """预发布版本仍必须直接失败，不能被归类为可自动升级的低版本。"""
        cases = (
            ("git", "2.50.0.rc1", 29),
            ("rust", "1.95.0-nightly", 21),
            ("node", "24.15.0-rc.1", 23),
            ("pnpm", "11.24.0-beta.1", 28),
        )
        for tool, version, exit_code in cases:
            with self.subTest(tool=tool), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(
                    probe,
                    rust=version if tool == "rust" else "1.95.0",
                    git=version if tool == "git" else "2.39.0",
                    python=False,
                )
                args = ["--install-missing"]
                if tool in ("node", "pnpm"):
                    fake_frontend_tools(
                        probe,
                        node=version if tool == "node" else "24.15.0",
                        pnpm=version if tool == "pnpm" else "11.24.0",
                    )

                result = self.run_gate(root, *args, probe=probe)
                self.assertEqual(result.returncode, exit_code)
                self.assertIn("错误：", result.stderr)
                self.assertFalse((root / "cargo").exists())
                self.assertFalse((root / "node-home").exists())

    def test_unparseable_versions_are_rejected_without_upgrade(self) -> None:
        """不可解析版本仍必须失败，不能触发任何安装或兼容回退。"""
        cases = (("git", 29), ("rust", 21), ("node", 23), ("pnpm", 28))
        for tool, exit_code in cases:
            with self.subTest(tool=tool), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                probe = root / "probe"
                fake_existing_tools(
                    probe,
                    rust="unknown" if tool == "rust" else "1.95.0",
                    git="unknown" if tool == "git" else "2.39.0",
                    python=False,
                )
                args = ["--install-missing"]
                if tool in ("node", "pnpm"):
                    fake_frontend_tools(
                        probe,
                        node="unknown" if tool == "node" else "24.15.0",
                        pnpm="unknown" if tool == "pnpm" else "11.24.0",
                    )

                result = self.run_gate(root, *args, probe=probe)
                self.assertEqual(result.returncode, exit_code)
                self.assertIn("识别", result.stderr)
                self.assertFalse((root / "cargo").exists())
                self.assertFalse((root / "node-home").exists())

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
            self.assertTrue((root / "rustup").is_dir())
            self.assertFalse((root / "project").exists())
            self.assertIn("gate.node.version=v24.15.0", result.stdout)
            self.assertIn("gate.pnpm.version=12.1.0", result.stdout)
            self.assertTrue((root / "node-home" / "v24.15.0" / "bin" / "node").is_file())
            self.assertTrue((root / "home" / ".local" / "share" / "agent-first-pnpm" / "bin" / "pnpm").is_file())
            session_env = os.environ.copy()
            session_env.update(
                {
                    "HOME": str(root / "home"),
                    "PATH": f"{root}/literal-*:/usr/bin::/bin:/usr/bin",
                    "SHELL": str(POSIX_SHELL),
                }
            )
            (root / "literal-match").mkdir()
            session = subprocess.run(
                [
                    str(POSIX_SHELL),
                    "-l",
                    "-c",
                    'printf "PATH=%s\\n" "$PATH"; '
                    "node --version; npm --version; pnpm --version; rustup --version; rustc --version; cargo --version; "
                    'case $- in *f*) printf "GLOB=disabled\\n" ;; *) printf "GLOB=enabled\\n" ;; esac',
                ],
                text=True,
                capture_output=True,
                cwd=root,
                env=session_env,
                timeout=30,
                check=False,
            )
            self.assertEqual(session.returncode, 0, session.stderr)
            persisted_path = session.stdout.splitlines()[0].removeprefix("PATH=").split(":")
            self.assertNotIn("", persisted_path)
            self.assertEqual(len(persisted_path), len(dict.fromkeys(persisted_path)))
            self.assertIn(f"{root}/literal-*", persisted_path)
            self.assertNotIn(f"{root}/literal-match", persisted_path)
            self.assertIn("v24.15.0", session.stdout)
            self.assertIn("12.1.0", session.stdout)
            self.assertIn("rustup 1.28.0", session.stdout)
            self.assertIn("rustc 1.95.0", session.stdout)
            self.assertIn("cargo 1.95.0", session.stdout)
            self.assertIn("GLOB=enabled", session.stdout)

    def test_project_rust_homes_cannot_redirect_managed_install(self) -> None:
        """继承的项目内 CARGO_HOME/RUSTUP_HOME 不得改变 Rust 的当前用户受管安装目标。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe)
            for tool in ("rustup", "rustc", "cargo"):
                (probe / tool).unlink()
            rust_dist = make_rust_dist(root / "rustup-dist")

            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_RUSTUP_DIST_BASE=rust_dist,
                AFH_ALLOW_FILE_URLS="1",
                AFH_SKIP_PERSIST_PATH="1",
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertTrue((root / "cargo" / "bin" / "cargo").is_file())
            self.assertTrue((root / "rustup").is_dir())
            self.assertFalse((root / "project").exists())

    def test_fresh_shell_rechecks_unchanged_frontend_tools_after_rust_upgrade(self) -> None:
        """只升级 Rust 时，临时探测 PATH 中的既有 Node/npm/pnpm 不能让新会话闭环误报成功。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, rust="1.94.9")
            fake_frontend_tools(probe)
            rust_dist = make_rust_dist(root / "rustup-dist", installed_version="1.95.0")

            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_RUSTUP_DIST_BASE=rust_dist,
                AFH_ALLOW_FILE_URLS="1",
                PATH=os.environ.get("PATH", ""),
            )

            self.assertEqual(result.returncode, 24, result.stderr)
            self.assertIn("新 shell 无法从持久 PATH", result.stderr)

    def test_check_only_reports_missing_without_installing(self) -> None:
        """只读模式必须以退出码 20 报告缺失，且不得创建安装目录。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.run_gate(root, "--check-only")
            self.assertEqual(result.returncode, 20)
            self.assertIn("gate.rust.status=missing", result.stdout)
            self.assertIn("gate.git.status=missing", result.stdout)
            self.assertIn("gate.node.status=missing", result.stdout)
            self.assertFalse((root / "cargo").exists())

    def test_rust_upgrade_that_remains_below_msrv_fails_closed(self) -> None:
        """Rust 升级后复探仍低于 MSRV 时必须失败，不能降低项目门禁。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, rust="1.94.9")
            rust_dist = make_rust_dist(root / "rustup-dist", installed_version="1.94.10")
            result = self.run_gate(
                root,
                "--install-missing",
                probe=probe,
                AFH_RUSTUP_DIST_BASE=rust_dist,
                AFH_ALLOW_FILE_URLS="1",
            )
            self.assertEqual(result.returncode, 22)
            self.assertIn("升级后仍低于 MSRV 1.95.0", result.stderr)

    def test_rust_installer_failure_blocks_the_gate(self) -> None:
        """Rust 安装器失败必须保留非零结论，不能继续生成项目。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, rust="1.94.9")
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
        """Node 升级制品摘要不匹配时必须拒绝，防止不可信下载进入 PATH。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, python=False)
            fake_frontend_tools(probe, node="22.16.0")
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

    def test_pnpm_upgrade_failure_blocks_the_gate(self) -> None:
        """低版本 pnpm 的受管升级失败时必须保留非零结论。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            probe = root / "probe"
            fake_existing_tools(probe, python=False)
            fake_frontend_tools(probe, pnpm="11.23.9")
            fake_npm_installer(probe, succeeds=False)
            result = self.run_gate(root, "--install-missing", probe=probe)
            self.assertEqual(result.returncode, 28)
            self.assertIn("pnpm 安装失败", result.stderr)

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
            '"gate.node.status=passed"',
            "$MinimumRustMinor = 95",
            '$GitRequirement = ">=2.0.0"',
            '$NodeRequirement = "^24.15.0 || >=26.0.0"',
            '$PnpmRequirement = ">=11.24.0"',
            '$PnpmInstallRequirement = "pnpm@>=11.24.0"',
            "Test-NodeVersion",
            "Test-PnpmVersion",
        )
        for fragment in required:
            self.assertIn(fragment, text)
        self.assertNotIn("pnpm@latest", text)


if __name__ == "__main__":
    unittest.main()
