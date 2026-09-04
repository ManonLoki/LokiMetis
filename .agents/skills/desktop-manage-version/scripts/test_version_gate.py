#!/usr/bin/env python3
"""回归测试下游产品自动版本门禁。"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import version_gate


class VersionGateTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        subprocess.run(
            ["git", "init", "--initial-branch=main", "."],
            cwd=self.root,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self._write_cargo("0.1.4")
        version_gate.initialize(self.root)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def _write_cargo(self, version: str) -> None:
        (self.root / "Cargo.toml").write_text(
            "[workspace]\n"
            'members = []\n\n'
            "[workspace.package]\n"
            f'version = "{version}"\n'
            'edition = "2024"\n',
            encoding="utf-8",
        )

    def _apply(
        self,
        kind: str,
        change_id: str | None = None,
        *,
        major: int | None = None,
        user_approved: bool = False,
    ) -> dict[str, object]:
        return version_gate.evaluate_change(
            self.root,
            action="apply",
            kind=kind,
            change_id=change_id,
            major=major,
            user_approved=user_approved,
        )

    def _state(self) -> dict[str, object]:
        return json.loads(
            (self.root / version_gate.STATE_RELATIVE).read_text(encoding="utf-8")
        )

    def test_first_feature_bumps_minor_resets_patch_and_later_feature_does_not(self) -> None:
        first = self._apply("feature", "FEAT-1")
        state_before_build = (self.root / version_gate.STATE_RELATIVE).read_bytes()
        build_check = version_gate.check(self.root, "build")
        self.assertEqual(
            state_before_build,
            (self.root / version_gate.STATE_RELATIVE).read_bytes(),
        )
        second = self._apply("feature", "FEAT-2")

        self.assertEqual(first["after_version"], "0.2.0")
        self.assertTrue(first["version_bumped"])
        self.assertTrue(build_check["passed"])
        self.assertEqual(second["after_version"], "0.2.0")
        self.assertFalse(second["version_bumped"])
        self.assertEqual(self._state()["target_version"], "0.2.0")

    def test_distinct_bug_fixes_bump_patch_and_duplicate_id_is_idempotent(self) -> None:
        first = self._apply("bug-fix", "BUG-1")
        duplicate = self._apply("bug-fix", "BUG-1")
        second = self._apply("bug-fix", "BUG-2")

        self.assertEqual(first["after_version"], "0.1.5")
        self.assertEqual(duplicate["after_version"], "0.1.5")
        self.assertEqual(duplicate["reason"], "change-already-applied")
        self.assertEqual(second["after_version"], "0.1.6")

    def test_maintenance_and_plan_do_not_mutate_files(self) -> None:
        cargo = (self.root / "Cargo.toml").read_bytes()
        state = (self.root / version_gate.STATE_RELATIVE).read_bytes()

        planned = version_gate.evaluate_change(
            self.root,
            action="plan",
            kind="feature",
            change_id="FEAT-PLAN",
            major=None,
            user_approved=False,
        )
        maintenance = self._apply("maintenance", "QUERY-1")

        self.assertEqual(planned["required_version"], "0.2.0")
        self.assertFalse(planned["changed"])
        self.assertEqual(maintenance["reason"], "maintenance-does-not-change-version")
        self.assertEqual((self.root / "Cargo.toml").read_bytes(), cargo)
        self.assertEqual((self.root / version_gate.STATE_RELATIVE).read_bytes(), state)

    def test_successful_release_resets_feature_gate_but_retains_bug_deduplication(self) -> None:
        self._apply("feature", "FEAT-1")
        self._apply("bug-fix", "BUG-1")
        version_gate.finalize_release(self.root, "0.2.1", "a" * 40)

        old_bug = self._apply("bug-fix", "BUG-1")
        next_feature = self._apply("feature", "FEAT-2")
        regression = self._apply("bug-fix", "BUG-1-REGRESSION-1")

        self.assertEqual(old_bug["reason"], "bug-id-already-consumed")
        self.assertEqual(next_feature["after_version"], "0.3.0")
        self.assertEqual(regression["after_version"], "0.3.1")

    def test_major_requires_explicit_user_approval_and_resets_minor_patch(self) -> None:
        with self.assertRaisesRegex(version_gate.GateError, "explicit"):
            self._apply("major", "BREAK-1", major=2)

        result = self._apply("major", "BREAK-1", major=2, user_approved=True)
        later_feature = self._apply("feature", "FEAT-1")

        self.assertEqual(result["after_version"], "2.0.0")
        self.assertEqual(later_feature["after_version"], "2.0.0")

    def test_component_overflow_blocks_without_carry(self) -> None:
        overflow_root = self.root / "overflow"
        overflow_root.mkdir()
        subprocess.run(
            ["git", "init", "--initial-branch=main", "."],
            cwd=overflow_root,
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        (overflow_root / "Cargo.toml").write_text(
            '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.100.100"\n',
            encoding="utf-8",
        )
        version_gate.initialize(overflow_root)

        with self.assertRaisesRegex(version_gate.GateError, "Minor overflow"):
            version_gate.evaluate_change(
                overflow_root,
                action="apply",
                kind="feature",
                change_id="FEAT-OVERFLOW",
                major=None,
                user_approved=False,
            )
        with self.assertRaisesRegex(version_gate.GateError, "Patch overflow"):
            version_gate.evaluate_change(
                overflow_root,
                action="apply",
                kind="bug-fix",
                change_id="BUG-OVERFLOW",
                major=None,
                user_approved=False,
            )

    def test_version_drift_and_unstable_versions_fail_closed(self) -> None:
        self._write_cargo("0.1.5")
        with self.assertRaisesRegex(version_gate.GateError, "version drift"):
            version_gate.check(self.root, "build")
        self._write_cargo("0.1.0-beta.1")
        with self.assertRaisesRegex(version_gate.GateError, "stable"):
            version_gate._cargo_version(self.root / "Cargo.toml")

    def test_corrupt_state_version_type_fails_as_gate_error(self) -> None:
        state = self._state()
        state["target_version"] = 15
        (self.root / version_gate.STATE_RELATIVE).write_text(
            json.dumps(state), encoding="utf-8"
        )

        with self.assertRaisesRegex(version_gate.GateError, "must be a string"):
            version_gate.check(self.root, "development")

    def test_command_apply_emits_stable_json_and_updates_cargo(self) -> None:
        result = subprocess.run(
            [
                sys.executable,
                str(Path(version_gate.__file__)),
                "apply",
                "--project-root",
                str(self.root),
                "--kind",
                "bug-fix",
                "--change-id",
                "BUG-GUI-1",
            ],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        self.assertEqual(result.returncode, 0, result.stderr)
        payload = json.loads(result.stdout)
        self.assertEqual(payload["after_version"], "0.1.5")
        self.assertEqual(payload["required_version"], "0.1.5")
        self.assertIn('version = "0.1.5"', (self.root / "Cargo.toml").read_text())

    def test_command_init_is_the_only_operation_allowed_before_independent_git(self) -> None:
        """初始化可先建立受保护状态，其余命令仍拒绝父仓库边界。"""

        nested = self.root / "pre-git-project"
        nested.mkdir()
        (nested / "Cargo.toml").write_text(
            "[workspace]\n"
            "members = []\n\n"
            "[workspace.package]\n"
            'version = "0.1.0"\n'
            'edition = "2024"\n',
            encoding="utf-8",
        )
        helper = str(Path(version_gate.__file__))
        initialized = subprocess.run(
            [
                sys.executable,
                helper,
                "init",
                "--project-root",
                str(nested),
            ],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        checked = subprocess.run(
            [
                sys.executable,
                helper,
                "check",
                "--project-root",
                str(nested),
                "--phase",
                "development",
            ],
            check=False,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        self.assertEqual(initialized.returncode, 0, initialized.stderr)
        self.assertTrue((nested / version_gate.STATE_RELATIVE).is_file())
        self.assertNotEqual(checked.returncode, 0)
        self.assertIn("does not equal project root", checked.stderr)

    def test_symlink_project_root_fails_closed(self) -> None:
        linked_root = self.root.parent / f"{self.root.name}-link"
        linked_root.symlink_to(self.root, target_is_directory=True)
        self.addCleanup(linked_root.unlink)

        with self.assertRaisesRegex(version_gate.GateError, "must not be a symlink"):
            version_gate._project_root(str(linked_root))


if __name__ == "__main__":
    unittest.main()
