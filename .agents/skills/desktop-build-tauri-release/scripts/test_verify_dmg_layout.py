#!/usr/bin/env python3
from __future__ import annotations

import os
import subprocess
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("verify-dmg-layout.sh")


def executable(path: Path, content: str) -> None:
    """创建隔离 hdiutil 替身，不读取或挂载真实磁盘镜像。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)


def fake_hdiutil(root: Path) -> Path:
    """按环境变量生成完整或故障卷，并记录每次 detach。"""
    probe = root / "probe"
    detached = root / "detached"
    executable(
        probe / "hdiutil",
        f"""#!/bin/sh
set -eu
if [ "$1" = attach ]; then
  shift
  mount=
  while [ "$#" -gt 0 ]; do
    if [ "$1" = -mountpoint ]; then
      shift
      mount=$1
    fi
    shift
  done
  [ -n "$mount" ] || exit 2
  mkdir -p "$mount/Test App.app/Contents/Resources" "$mount/.background"
  mode=${{AFH_DMG_FIXTURE_MODE:-valid}}
  [ "$mode" = missing-ds-store ] || printf '%s' finder > "$mount/.DS_Store"
  [ "$mode" = missing-background ] || printf '%s' png > "$mount/.background/background.png"
  if [ "$mode" = wrong-applications ]; then
    ln -s /tmp "$mount/Applications"
  else
    ln -s /Applications "$mount/Applications"
  fi
  [ "$mode" = multiple-apps ] && mkdir "$mount/Second App.app"
  if [ "$mode" != missing-release-notes ]; then
    if [ "$mode" = mismatched-release-notes ]; then
      printf '%s' mismatch > "$mount/Test App.app/Contents/Resources/release-notes.json"
    else
      cp "$AFH_DMG_RELEASE_NOTES_SOURCE" "$mount/Test App.app/Contents/Resources/release-notes.json"
    fi
  fi
  printf '%s\n' '/dev/disk-test Apple_HFS Test'
elif [ "$1" = detach ]; then
  : > '{detached}'
else
  exit 2
fi
""",
    )
    return probe


class VerifyDmgLayoutTests(unittest.TestCase):
    """覆盖最终 DMG 布局成功路径与最危险的空白 Finder 窗口失败。"""

    def run_check(
        self,
        root: Path,
        dmg: Path,
        *,
        mode: str = "valid",
        platform: str = "Darwin",
    ) -> subprocess.CompletedProcess[str]:
        """在伪 macOS 与隔离 hdiutil 下运行只读检查。"""
        env = os.environ.copy()
        release_notes = root / "release-notes.json"
        if not release_notes.exists():
            release_notes.write_text(
                '{"schemaVersion":2,"releases":[]}\n', encoding="utf-8"
            )
        env.update(
            {
                "AFH_PREREQ_PATH": str(fake_hdiutil(root)),
                "AFH_TEST_PLATFORM": platform,
                "AFH_ALLOW_TEST_OVERRIDES": "1",
                "AFH_DMG_FIXTURE_MODE": mode,
                "AFH_DMG_RELEASE_NOTES_SOURCE": str(release_notes),
            }
        )
        return subprocess.run(
            ["/bin/sh", str(SCRIPT), str(dmg), str(release_notes)],
            text=True,
            capture_output=True,
            env=env,
            timeout=10,
            check=False,
        )

    def test_complete_readonly_volume_layout_passes_and_detaches(self) -> None:
        """最终卷具备 Finder 状态、背景、应用和 Applications 落点时应通过并卸载。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            result = self.run_check(root, dmg)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.macos_dmg_layout.status=passed", result.stdout)
            self.assertIn("app_count=1", result.stdout)
            self.assertIn("release_notes=byte-identical", result.stdout)
            self.assertTrue((root / "detached").is_file())

    def test_missing_ds_store_fails_closed_and_detaches(self) -> None:
        """只有背景图而没有 .DS_Store 时必须拒绝，复现 CI 下的空白 Finder 窗口。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            result = self.run_check(root, dmg, mode="missing-ds-store")
            self.assertEqual(result.returncode, 41)
            self.assertIn("finder-ds-store-missing", result.stdout)
            self.assertTrue((root / "detached").is_file())

    def test_wrong_applications_link_and_multiple_apps_are_rejected(self) -> None:
        """拖拽目标错误或候选内含多个应用时都不能形成确定安装布局。"""
        for mode, reason in (
            ("wrong-applications", "applications-link-target-invalid"),
            ("multiple-apps", "app-bundle-count-invalid"),
        ):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                dmg = root / "candidate.dmg"
                dmg.write_bytes(b"dmg")
                result = self.run_check(root, dmg, mode=mode)
                self.assertEqual(result.returncode, 41)
                self.assertIn(reason, result.stdout)

    def test_symlinked_dmg_is_rejected_before_mount(self) -> None:
        """输入 DMG 为符号链接时不得跟随到未审查路径。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / "real.dmg"
            target.write_bytes(b"dmg")
            link = root / "candidate.dmg"
            link.symlink_to(target)
            result = self.run_check(root, link)
            self.assertEqual(result.returncode, 41)
            self.assertIn("dmg-not-regular-file", result.stdout)
            self.assertFalse((root / "detached").exists())

    def test_missing_or_mismatched_release_notes_resource_is_rejected(self) -> None:
        """最终 DMG 中缺少或修改更新日志时不得形成候选。"""

        for mode, reason in (
            ("missing-release-notes", "release-notes-resource-missing"),
            ("mismatched-release-notes", "release-notes-resource-mismatch"),
        ):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                dmg = root / "candidate.dmg"
                dmg.write_bytes(b"dmg")
                result = self.run_check(root, dmg, mode=mode)
                self.assertEqual(result.returncode, 41)
                self.assertIn(reason, result.stdout)

    def test_symlinked_release_notes_source_is_rejected_before_mount(self) -> None:
        """根更新日志为符号链接时不得与候选资源比较。"""

        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            target = root / "notes-target.json"
            target.write_text('{"schemaVersion":2,"releases":[]}\n', encoding="utf-8")
            (root / "release-notes.json").symlink_to(target)
            result = self.run_check(root, dmg)
            self.assertEqual(result.returncode, 41)
            self.assertIn("release-notes-source-not-regular-file", result.stdout)
            self.assertFalse((root / "detached").exists())

    def test_non_macos_host_is_not_applicable(self) -> None:
        """非 macOS 宿主不得伪装已检查 Finder DMG 布局。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            result = self.run_check(root, dmg, platform="Linux")
            self.assertEqual(result.returncode, 30)
            self.assertIn("requires-macos-host", result.stdout)


if __name__ == "__main__":
    unittest.main()
