"""Harness 升级写入、漂移检测和记录回归。"""

from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import stat

from harness_upgrade_test_support import (
    LEGACY_TOMBSTONES,
    MANAGED,
    MANAGED_SECOND,
    MANAGED_SELF,
    MIXED,
    PROTECTED,
    HarnessUpgradeTestCase,
)


class HarnessUpgradeMutationTests(HarnessUpgradeTestCase):
    """验证受审计划后的单文件写入和共同基线记录。"""

    def test_tampered_plan_cannot_overwrite_protected_file(self) -> None:
        """手工把策略伪装为 managed/update 时 apply 必须在写入前拒绝。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        self.write(self.target, PROTECTED, "keep")
        self.bootstrap()
        self.write(self.candidate, MANAGED, "v2")
        self.write(self.candidate, PROTECTED, "malicious")
        _, plan_path = self.create_plan(expected=2)
        payload = json.loads(plan_path.read_text(encoding="utf-8"))
        managed_item = next(item for item in payload["actions"] if item["path"] == MANAGED)
        managed_item["path"] = PROTECTED
        plan_path.write_text(json.dumps(payload), encoding="utf-8")
        self.run_tool(
            "apply",
            "--plan",
            str(plan_path),
            "--approval",
            "apply-managed-changes",
            "--path",
            PROTECTED,
            expected=2,
        )
        self.assertEqual("keep", (self.target / PROTECTED).read_text(encoding="utf-8"))

    def test_target_parent_symlink_after_plan_cannot_escape(self) -> None:
        """plan 后父目录被换成外链时，apply 不得修改仓库外文件。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        self.bootstrap()
        self.write(self.candidate, MANAGED, "v2")
        _, plan_path = self.create_plan()

        managed_parent = (self.target / MANAGED).parent
        shutil.rmtree(managed_parent)
        outside_parent = self.root / "outside-managed"
        outside_parent.mkdir()
        outside_file = outside_parent / Path(MANAGED).name
        outside_file.write_text("v1", encoding="utf-8")
        managed_parent.symlink_to(outside_parent, target_is_directory=True)

        self.run_tool(
            "apply",
            "--plan",
            str(plan_path),
            "--approval",
            "apply-managed-changes",
            "--path",
            MANAGED,
            expected=2,
        )
        self.assertEqual("v1", outside_file.read_text(encoding="utf-8"))

    def test_plan_output_cannot_overwrite_target_or_existing_file(self) -> None:
        """只读 plan 的输出必须在两棵树外独占创建。"""

        self.write(self.target, PROTECTED, "keep")
        self.run_tool(
            "plan",
            *self.shared_arguments(),
            "--output",
            str(self.target / PROTECTED),
            expected=2,
        )
        self.assertEqual("keep", (self.target / PROTECTED).read_text(encoding="utf-8"))
        existing = self.root / "existing.json"
        existing.write_text("keep", encoding="utf-8")
        self.run_tool(
            "plan",
            *self.shared_arguments(),
            "--output",
            str(existing),
            expected=2,
        )
        self.assertEqual("keep", existing.read_text(encoding="utf-8"))

    def test_lock_path_must_be_exact_control_path(self) -> None:
        """仓库外或产品目录内的伪 lock 都不能参与计划。"""

        self.run_tool(
            "plan",
            *self.shared_arguments(lock=self.root / "forged-lock.json"),
            expected=2,
        )
        self.run_tool(
            "plan",
            *self.shared_arguments(lock=self.target / "src/provenance.json"),
            expected=2,
        )

    def test_ownership_mode_drift_blocks(self) -> None:
        """lock 的旧 mode 与当前清单不一致时必须人工迁移。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        self.bootstrap()
        lock = json.loads(self.lock.read_text(encoding="utf-8"))
        lock["entries"][MANAGED]["mode"] = "merge-sections"
        self.lock.write_text(json.dumps(lock), encoding="utf-8")
        plan = self.plan(expected=2)
        self.assertTrue(any("所有权 mode 已变化" in item for item in plan["problems"]))

    def test_weakened_ownership_manifest_is_rejected(self) -> None:
        """生产最小 tombstone/protected/self-update 规则不得被下游清单弱化。"""

        manifest = json.loads(self.ownership.read_text(encoding="utf-8"))
        manifest["rules"] = [
            item for item in manifest["rules"] if item["pattern"] != "Version.md"
        ]
        self.ownership.write_text(json.dumps(manifest), encoding="utf-8")
        result = self.run_tool("plan", *self.shared_arguments(), expected=2)
        self.assertIn("削弱了必需保护", result["error"])

    def test_shadowed_legacy_tombstones_are_rejected(self) -> None:
        """通用 managed 规则不得通过提前排序遮蔽旧能力 tombstone。"""

        manifest = json.loads(self.ownership.read_text(encoding="utf-8"))
        generic = next(
            item
            for item in manifest["rules"]
            if item["pattern"] == ".agents/skills/**"
        )
        manifest["rules"].remove(generic)
        first_legacy_index = next(
            index
            for index, item in enumerate(manifest["rules"])
            if item["pattern"] == LEGACY_TOMBSTONES[2].removesuffix("/SKILL.md") + "/**"
        )
        manifest["rules"].insert(first_legacy_index, generic)
        self.ownership.write_text(json.dumps(manifest), encoding="utf-8")
        result = self.run_tool("plan", *self.shared_arguments(), expected=2)
        self.assertIn("规则顺序削弱了必需保护", result["error"])

    def test_source_provenance_is_bound_to_clean_git_head(self) -> None:
        """plan 绑定真实来源 HEAD，record 不能改写，来源漂移会使计划失效。"""

        self.bootstrap()
        _, plan_path = self.create_plan()
        result = self.run_tool(
            "record",
            "--plan",
            str(plan_path),
            "--source-version",
            self.source_version,
            "--source-commit",
            "0" * 40,
            "--approval",
            "record-verified-baseline",
            expected=2,
        )
        self.assertIn("必须与已复核 plan 精确匹配", result["error"])
        self.write(self.source, "untracked.txt", "drift")
        result = self.run_tool("plan", *self.shared_arguments(), expected=2)
        self.assertIn("必须保持干净", result["error"])

    def test_control_file_drift_blocks_apply_before_write(self) -> None:
        """受审后 ownership 字节变化必须使计划失效且不修改目标。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        self.bootstrap()
        self.write(self.candidate, MANAGED, "v2")
        _, plan_path = self.create_plan()
        manifest = json.loads(self.ownership.read_text(encoding="utf-8"))
        self.ownership.write_text(
            json.dumps(manifest, indent=4) + "\n",
            encoding="utf-8",
        )
        self.run_tool(
            "apply",
            "--plan",
            str(plan_path),
            "--approval",
            "apply-managed-changes",
            "--path",
            MANAGED,
            expected=2,
        )
        self.assertEqual("v1", (self.target / MANAGED).read_text(encoding="utf-8"))

    def test_permission_mode_changes_follow_three_way_rules(self) -> None:
        """上游 mode 单改可应用，本地 mode 单改必须保留。"""

        self.write(self.candidate, MANAGED, "v1")
        self.write(self.target, MANAGED, "v1")
        os.chmod(self.candidate / MANAGED, 0o644)
        os.chmod(self.target / MANAGED, 0o644)
        self.bootstrap()
        os.chmod(self.candidate / MANAGED, 0o755)
        plan, plan_path = self.create_plan()
        self.assertEqual("update", self.classification(plan, MANAGED))
        self.run_tool(
            "apply",
            "--plan",
            str(plan_path),
            "--approval",
            "apply-managed-changes",
            "--path",
            MANAGED,
        )
        self.assertEqual(0o755, (self.target / MANAGED).stat().st_mode & 0o777)
        _, converged_plan = self.create_plan(name="mode-converged")
        self.record(converged_plan)
        os.chmod(self.target / MANAGED, 0o700)
        plan = self.plan()
        self.assertEqual("preserve_local", self.classification(plan, MANAGED))

    def test_special_permission_bits_are_rejected(self) -> None:
        """setuid/setgid/sticky 等特殊位不进入可移植来源锁。"""

        self.write(self.candidate, MANAGED, "v1")
        os.chmod(self.candidate / MANAGED, 0o1755)
        observed_mode = stat.S_IMODE((self.candidate / MANAGED).stat().st_mode)
        if not observed_mode & ~0o777:
            self.skipTest("文件系统会清除特殊权限位")
        result = self.run_tool("plan", *self.shared_arguments(), expected=2)
        self.assertIn("不受支持的特殊权限位", result["error"])

    def test_full_preflight_prevents_partial_apply(self) -> None:
        """后项在 plan 后变化时，前项不得先被更新。"""

        for relative in (MANAGED, MANAGED_SECOND):
            self.write(self.candidate, relative, "v1")
            self.write(self.target, relative, "v1")
        self.bootstrap()
        for relative in (MANAGED, MANAGED_SECOND):
            self.write(self.candidate, relative, "v2")
        _, plan_path = self.create_plan()
        self.write(self.target, MANAGED_SECOND, "changed-after-plan")
        self.run_tool(
            "apply",
            "--plan",
            str(plan_path),
            "--approval",
            "apply-managed-changes",
            "--path",
            MANAGED,
            expected=2,
        )
        self.assertEqual("v1", (self.target / MANAGED).read_text(encoding="utf-8"))

    def test_manual_merge_must_be_named_before_record(self) -> None:
        """mixed-ownership 差异不能用固定 approval 静默吞掉。"""

        self.write(self.candidate, MIXED, "v1")
        self.write(self.target, MIXED, "v1")
        self.bootstrap()
        self.write(self.candidate, MIXED, "upstream")
        self.write(self.target, MIXED, "reviewed-local-merge")
        plan, plan_path = self.create_plan()
        self.assertEqual("manual_merge", self.classification(plan, MIXED))
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
        result = self.record(plan_path, resolved_manual=(MIXED,))
        self.assertGreater(result["entries"], 0)

    def test_managed_self_updates_after_other_managed_files(self) -> None:
        """自更新组必须排在普通 managed 更新之后。"""

        for relative in (MANAGED, MANAGED_SELF):
            self.write(self.candidate, relative, "v1")
            self.write(self.target, relative, "v1")
        self.bootstrap()
        for relative in (MANAGED, MANAGED_SELF):
            self.write(self.candidate, relative, "v2")
        _, plan_path = self.create_plan()
        self.run_tool(
            "apply",
            "--plan",
            str(plan_path),
            "--approval",
            "apply-managed-changes",
            "--path",
            MANAGED_SELF,
            expected=2,
        )
        result = self.run_tool(
            "apply",
            "--plan",
            str(plan_path),
            "--approval",
            "apply-managed-changes",
            "--path",
            MANAGED,
        )
        self.assertEqual([MANAGED], result["applied"])
        _, self_plan = self.create_plan(name="managed-self")
        result = self.run_tool(
            "apply",
            "--plan",
            str(self_plan),
            "--approval",
            "apply-managed-changes",
            "--path",
            MANAGED_SELF,
        )
        self.assertEqual([MANAGED_SELF], result["applied"])
