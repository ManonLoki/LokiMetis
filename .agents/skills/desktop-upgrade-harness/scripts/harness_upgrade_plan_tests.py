"""Harness 升级计划、所有权和 bootstrap 回归。"""

from __future__ import annotations

import subprocess

from harness_upgrade_test_support import (
    LEGACY_TOMBSTONES,
    MANAGED,
    MIXED,
    PROTECTED,
    REQUIRED_MANAGED_CHECKERS,
    TOMBSTONE,
    HarnessUpgradeTestCase,
)
from harness_upgrade_ownership import load_ownership, ownership_mode


class HarnessUpgradePlanTests(HarnessUpgradeTestCase):
    """验证只读计划和初始共同基线的安全边界。"""

    def test_upstream_only_change_can_apply_and_record(self) -> None:
        """上游单改可更新既有受管文件，并在重算计划后记录。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        self.bootstrap()
        self.write(self.candidate, MANAGED, "v2")
        plan, plan_path = self.create_plan()
        self.assertEqual("update", self.classification(plan, MANAGED))
        applied = self.run_tool(
            "apply",
            "--plan",
            str(plan_path),
            "--approval",
            "apply-managed-changes",
            "--path",
            MANAGED,
        )
        self.assertEqual([MANAGED], applied["applied"])
        self.assertEqual("v2", (self.target / MANAGED).read_text(encoding="utf-8"))
        _, converged_plan = self.create_plan(name="converged")
        self.record(converged_plan)

    def test_local_change_remains_protected_across_later_upstream_change(self) -> None:
        """preserve_local 记录后，上游下一次修改仍必须冲突而非覆盖。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        self.bootstrap()
        self.write(self.target, MANAGED, "local")
        plan, plan_path = self.create_plan()
        self.assertEqual("preserve_local", self.classification(plan, MANAGED))
        self.record(plan_path)
        self.write(self.candidate, MANAGED, "v2")
        later = self.plan(expected=2)
        self.assertEqual("conflict", self.classification(later, MANAGED))

    def test_local_delete_remains_protected_across_later_upstream_change(self) -> None:
        """本地删除不会在记录后被后续上游修改静默恢复。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        self.bootstrap()
        (self.target / MANAGED).unlink()
        plan, plan_path = self.create_plan()
        self.assertEqual("preserve_local", self.classification(plan, MANAGED))
        self.record(plan_path)
        self.write(self.candidate, MANAGED, "v2")
        later = self.plan(expected=2)
        self.assertEqual("conflict", self.classification(later, MANAGED))

    def test_both_sides_change_blocks(self) -> None:
        """候选和下游同时偏离共同基线时必须阻断。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        self.bootstrap()
        self.write(self.candidate, MANAGED, "upstream")
        self.write(self.target, MANAGED, "local")
        plan = self.plan(expected=2)
        self.assertEqual("conflict", self.classification(plan, MANAGED))

    def test_delete_requires_manual_resolution_before_record(self) -> None:
        """上游删除只生成手动动作，脚本不自动 unlink。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        self.bootstrap()
        (self.candidate / MANAGED).unlink()
        plan, plan_path = self.create_plan()
        self.assertEqual("delete", self.classification(plan, MANAGED))
        applied = self.run_tool(
            "apply",
            "--plan",
            str(plan_path),
            "--approval",
            "apply-managed-changes",
            "--path",
            MANAGED,
            expected=2,
        )
        self.assertFalse(applied["ok"])
        self.assertTrue((self.target / MANAGED).is_file())
        self.run_tool(
            "record",
            "--plan",
            str(plan_path),
            "--source-version",
            self.source_version,
            "--source-commit",
            self.source_commit,
            "--approval",
            "record-verified-baseline",
            expected=2,
        )
        (self.target / MANAGED).unlink()
        _, converged_plan = self.create_plan(name="delete-resolved")
        self.record(converged_plan)

    def test_new_managed_path_requires_manual_convergence(self) -> None:
        """新增文件先报告 add；目标逐项确认后相同才可记录。"""

        self.bootstrap()
        self.write(self.candidate, MANAGED, "new")
        plan = self.plan()
        self.assertEqual("add", self.classification(plan, MANAGED))
        self.write(self.target, MANAGED, "new")
        converged, plan_path = self.create_plan()
        self.assertEqual("converged", self.classification(converged, MANAGED))
        self.record(plan_path)

    def test_source_required_checkers_must_enter_rendered_candidate(self) -> None:
        """源 Harness 的必需检查器及测试不得在候选渲染时静默丢失。"""

        self.bootstrap()
        for required_path in REQUIRED_MANAGED_CHECKERS:
            self.write(self.source, required_path, f"source:{required_path}")
        subprocess.run(
            ["git", "-C", str(self.source), "add", *REQUIRED_MANAGED_CHECKERS],
            check=True,
            capture_output=True,
            text=True,
        )
        subprocess.run(
            [
                "git", "-C", str(self.source),
                "-c", "user.name=Harness Fixture",
                "-c", "user.email=harness-fixture@example.invalid",
                "commit", "-m", "add required checker",
            ],
            check=True,
            capture_output=True,
            text=True,
        )
        self.source_commit = subprocess.run(
            ["git", "-C", str(self.source), "rev-parse", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        ).stdout.strip()

        missing = self.plan(expected=2)
        for required_path in REQUIRED_MANAGED_CHECKERS:
            self.assertTrue(
                any(
                    required_path in item and "候选缺少源 Harness 必需 managed 路径" in item
                    for item in missing["problems"]
                ),
                missing,
            )
            self.write(self.candidate, required_path, f"stale:{required_path}")
        stale = self.plan(expected=2)
        for required_path in REQUIRED_MANAGED_CHECKERS:
            self.assertTrue(
                any(
                    required_path in item and "必需 managed 路径内容不匹配" in item
                    for item in stale["problems"]
                ),
                stale,
            )
            self.write(self.candidate, required_path, f"source:{required_path}")
        present = self.plan()
        for required_path in REQUIRED_MANAGED_CHECKERS:
            self.assertEqual("add", self.classification(present, required_path))

    def test_new_path_collision_blocks(self) -> None:
        """新增候选与同名本地文件字节不同时必须阻断。"""

        self.bootstrap()
        self.write(self.candidate, MANAGED, "upstream")
        self.write(self.target, MANAGED, "local")
        plan = self.plan(expected=2)
        self.assertEqual("collision", self.classification(plan, MANAGED))

    def test_protected_candidate_blocks_raw_source_tree(self) -> None:
        """候选包含策略/产品事实时不能被误当成已渲染工程候选。"""

        self.write(self.candidate, PROTECTED, "upstream")
        plan = self.plan(expected=2)
        self.assertEqual("protected_candidate", self.classification(plan, PROTECTED))

    def test_tombstone_in_candidate_or_target_blocks(self) -> None:
        """来源或目标出现终端下游禁回迁文件时都必须阻断。"""

        self.write(self.candidate, TOMBSTONE, "source")
        candidate_plan = self.plan(expected=2)
        self.assertEqual(
            "tombstone_candidate",
            self.classification(candidate_plan, TOMBSTONE),
        )
        (self.candidate / TOMBSTONE).unlink()
        self.write(self.target, TOMBSTONE, "target")
        target_plan = self.plan(expected=2)
        self.assertEqual("tombstone_present", self.classification(target_plan, TOMBSTONE))

    def test_gui_only_ownership_contract(self) -> None:
        """固定 GUI 工程层可传播，旧能力只能保留为 tombstone。"""

        default_mode, rules = load_ownership(self.ownership)
        for path in LEGACY_TOMBSTONES:
            self.assertEqual("tombstone", ownership_mode(path, default_mode, rules))
        self.assertEqual(
            "merge-sections",
            ownership_mode("docs/RUST_GUI_TEMPLATE.md", default_mode, rules),
        )
        for path in (
            ".agents/skills/desktop-add-gui-adapter/SKILL.md",
            ".agents/skills/desktop-prepare-gui-support-surfaces/SKILL.md",
            ".agents/skills/desktop-build-tauri-release/SKILL.md",
            ".agents/skills/desktop-test-gui-release-performance/SKILL.md",
        ):
            self.assertEqual("managed", ownership_mode(path, default_mode, rules))
        self.assertEqual(
            "conditional",
            ownership_mode(
                ".agents/skills/desktop-add-gui-system-tray/SKILL.md",
                default_mode,
                rules,
            ),
        )

    def test_legacy_tombstones_block_candidate_and_target(self) -> None:
        """旧能力路径出现在候选或下游时必须全部阻断。"""

        for path in LEGACY_TOMBSTONES:
            self.write(self.candidate, path, "legacy")
        candidate_plan = self.plan(expected=2)
        for path in LEGACY_TOMBSTONES:
            self.assertEqual(
                "tombstone_candidate",
                self.classification(candidate_plan, path),
            )
            (self.candidate / path).unlink()
            self.write(self.target, path, "legacy")
        target_plan = self.plan(expected=2)
        for path in LEGACY_TOMBSTONES:
            self.assertEqual(
                "tombstone_present",
                self.classification(target_plan, path),
            )

    def test_target_tombstone_symlink_and_empty_directory_block(self) -> None:
        """tombstone 即使不是普通文件也必须阻断 bootstrap。"""

        outside = self.root / "outside-version.txt"
        outside.write_text("outside", encoding="utf-8")
        (self.target / TOMBSTONE).symlink_to(outside)
        plan = self.plan(expected=2)
        self.assertTrue(
            any("目标包含 tombstone 路径：Version.md" in item for item in plan["problems"]),
            plan,
        )
        (self.target / TOMBSTONE).unlink()
        (self.target / ".agents/skills/desktop-instantiate-project").mkdir(parents=True)
        plan = self.plan(expected=2)
        self.assertTrue(
            any("desktop-instantiate-project" in item for item in plan["problems"]),
            plan,
        )

    def test_manual_add_cannot_be_recorded_without_creating_target(self) -> None:
        """仅声明 resolved-manual 不得把仍缺失的 mixed 文件写入基线。"""

        self.write(self.candidate, MIXED, "candidate")
        plan, plan_path = self.create_plan(expected=2)
        self.assertEqual("manual_add", self.classification(plan, MIXED))
        self.run_tool(
            "record",
            "--plan",
            str(plan_path),
            "--source-version",
            self.source_version,
            "--source-commit",
            self.source_commit,
            "--resolved-manual",
            MIXED,
            "--bootstrap",
            "--approval",
            "bootstrap-verified-baseline",
            expected=2,
        )
        self.assertFalse(self.lock.exists())

    def test_candidate_symlink_blocks_plan_and_bootstrap_record(self) -> None:
        """候选符号链接既阻断计划，也不能被 bootstrap 常量绕过。"""

        outside = self.root / "outside.txt"
        outside.write_text("secret", encoding="utf-8")
        path = self.candidate / MANAGED
        path.parent.mkdir(parents=True, exist_ok=True)
        path.symlink_to(outside)
        _, plan_path = self.create_plan(expected=2)
        self.run_tool(
            "record",
            "--plan",
            str(plan_path),
            "--source-version",
            self.source_version,
            "--source-commit",
            self.source_commit,
            "--bootstrap",
            "--approval",
            "bootstrap-verified-baseline",
            expected=2,
        )
        self.assertFalse(self.lock.exists())

    def test_bootstrap_rejects_divergent_managed_overlap(self) -> None:
        """bootstrap 只能记录已逐项收敛的 managed 重叠。"""

        self.write(self.candidate, MANAGED, "upstream")
        self.write(self.target, MANAGED, "local")
        _, plan_path = self.create_plan(expected=2)
        self.run_tool(
            "record",
            "--plan",
            str(plan_path),
            "--source-version",
            self.source_version,
            "--source-commit",
            self.source_commit,
            "--bootstrap",
            "--approval",
            "bootstrap-verified-baseline",
            expected=2,
        )
        self.assertFalse(self.lock.exists())

    def test_bootstrap_rejects_tombstone(self) -> None:
        """bootstrap 不得豁免 tombstone_present。"""

        self.write(self.target, TOMBSTONE, "forbidden")
        _, plan_path = self.create_plan(expected=2)
        self.run_tool(
            "record",
            "--plan",
            str(plan_path),
            "--source-version",
            self.source_version,
            "--source-commit",
            self.source_commit,
            "--bootstrap",
            "--approval",
            "bootstrap-verified-baseline",
            expected=2,
        )
        self.assertFalse(self.lock.exists())
