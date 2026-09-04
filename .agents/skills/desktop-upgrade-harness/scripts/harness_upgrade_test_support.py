#!/usr/bin/env python3
"""Harness 升级测试的隔离 Git 夹具与共享断言。"""

from __future__ import annotations

import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest



SCRIPT = Path(__file__).with_name("harness_upgrade.py")
PRODUCTION_OWNERSHIP = SCRIPT.parents[1] / "references" / "ownership-manifest.json"
MANAGED = ".agents/skills/desktop-define-product/SKILL.md"
MANAGED_SECOND = ".agents/skills/desktop-plan-change/SKILL.md"
MANAGED_SELF = ".agents/skills/desktop-upgrade-harness/scripts/harness_upgrade.py"
CORE_FIRST_CHECKER = ".agents/skills/desktop-implement-change/scripts/check_core_first.py"
REQUIRED_MANAGED_CHECKERS = (
    ".agents/skills/desktop-implement-change/scripts/check_file_line_limits.py",
    ".agents/skills/desktop-implement-change/scripts/test_check_file_line_limits.py",
    CORE_FIRST_CHECKER,
    ".agents/skills/desktop-implement-change/scripts/test_check_core_first.py",
    ".agents/skills/desktop-implement-change/scripts/check_rust_chinese_comments.py",
    ".agents/skills/desktop-implement-change/scripts/test_check_rust_chinese_comments.py",
)
MIXED = "AGENTS.md"
PROTECTED = "docs/AGENT_POLICY.md"
TOMBSTONE = "Version.md"
LEGACY_TOMBSTONES = (
    "docs/RUST_CLI_TEMPLATE.md",
    "docs/CLI_CONTRACT.md",
    ".agents/skills/desktop-add-cli-adapter/SKILL.md",
    ".agents/skills/desktop-add-tui-adapter/SKILL.md",
    ".agents/skills/desktop-add-mcp-adapter/SKILL.md",
    ".agents/skills/desktop-build-rust-release/SKILL.md",
    ".agents/skills/desktop-prepare-cross-platform-release/SKILL.md",
    ".agents/skills/desktop-run-parallel-worktrees/SKILL.md",
    ".agents/skills/desktop-add-gui-updater/SKILL.md",
    ".agents/skills/desktop-add-gui-system-locale/SKILL.md",
    ".agents/skills/desktop-add-gui-window-state/SKILL.md",
    ".agents/skills/desktop-add-gui-dialog/SKILL.md",
    ".agents/skills/desktop-curate-harness-memory/SKILL.md",
    "scripts/harness_validation_test_support.py",
)


class HarnessUpgradeTestCase(unittest.TestCase):
    """每个测试使用生产所有权清单和独立候选/下游 Git 根。"""

    def setUp(self) -> None:
        """初始化隔离目录、真实控制路径和空来源锁。"""

        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.candidate = self.root / "candidate"
        self.target = self.root / "target"
        self.source.mkdir()
        self.candidate.mkdir()
        self.target.mkdir()
        for repository in (self.source, self.target):
            subprocess.run(
                ["git", "init", "--initial-branch=main", str(repository)],
                check=True,
                capture_output=True,
                text=True,
            )
        self.source_version = "202607310001"
        self.write(
            self.source,
            "Version.md",
            f"# Harness 版本\n\n- 当前版本：`{self.source_version}`\n",
        )
        subprocess.run(
            ["git", "-C", str(self.source), "add", "Version.md"],
            check=True,
            capture_output=True,
            text=True,
        )
        for repository in (self.source, self.target):
            subprocess.run(
                [
                    "git",
                    "-C",
                    str(repository),
                    "-c",
                    "user.name=Harness Fixture",
                    "-c",
                    "user.email=harness-fixture@example.invalid",
                    "commit",
                    "--allow-empty",
                    "-m",
                    "fixture baseline",
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
        self.ownership = (
            self.target
            / ".agents/skills/desktop-upgrade-harness/references/ownership-manifest.json"
        )
        candidate_ownership = self.candidate / self.ownership.relative_to(self.target)
        self.ownership.parent.mkdir(parents=True)
        candidate_ownership.parent.mkdir(parents=True)
        shutil.copy2(PRODUCTION_OWNERSHIP, self.ownership)
        shutil.copy2(PRODUCTION_OWNERSHIP, candidate_ownership)
        self.lock = self.target / ".harness/upstream-lock.json"
        self.plan_counter = 0

    def tearDown(self) -> None:
        """清理测试独占目录，不接触真实工作区。"""

        self.temporary.cleanup()

    def write(self, root: Path, relative: str, content: str) -> None:
        """在测试夹具根目录内写普通文件。"""

        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")

    def run_tool(self, *arguments: str, expected: int = 0) -> dict:
        """执行真实脚本、断言退出码并解析结构化输出。"""

        result = subprocess.run(
            [sys.executable, str(SCRIPT), *arguments],
            check=False,
            capture_output=True,
            text=True,
        )
        self.assertEqual(expected, result.returncode, result.stderr or result.stdout)
        payload = result.stdout if result.stdout.strip() else result.stderr
        return json.loads(payload)

    def shared_arguments(self, *, lock: Path | None = None) -> list[str]:
        """返回 plan 使用的显式候选、目标、生产清单和精确 lock 参数。"""

        return [
            "--source-root",
            str(self.source),
            "--source-version",
            self.source_version,
            "--source-commit",
            self.source_commit,
            "--candidate-root",
            str(self.candidate),
            "--target-root",
            str(self.target),
            "--ownership",
            str(self.ownership),
            "--lock",
            str(self.lock if lock is None else lock),
        ]

    def create_plan(self, *, expected: int = 0, name: str = "plan") -> tuple[dict, Path]:
        """在候选/目标之外独占创建计划。"""

        self.plan_counter += 1
        plan_path = self.root / f"{name}-{self.plan_counter}.json"
        plan = self.run_tool(
            "plan",
            *self.shared_arguments(),
            "--output",
            str(plan_path),
            expected=expected,
        )
        return plan, plan_path

    def bootstrap(self) -> None:
        """经显式审核计划建立测试夹具的首份共同基线。"""

        _, plan_path = self.create_plan(expected=2, name="bootstrap")
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
        )

    def record(
        self,
        plan_path: Path,
        *,
        resolved_manual: tuple[str, ...] = (),
    ) -> dict:
        """记录一个非 bootstrap 受审计划。"""

        arguments = [
            "record",
            "--plan",
            str(plan_path),
            "--source-version",
            self.source_version,
            "--source-commit",
            self.source_commit,
            "--approval",
            "record-verified-baseline",
        ]
        for path in resolved_manual:
            arguments.extend(["--resolved-manual", path])
        return self.run_tool(*arguments)

    def plan(self, expected: int = 0) -> dict:
        """只在 stdout 生成计划。"""

        return self.run_tool("plan", *self.shared_arguments(), expected=expected)

    def classification(self, plan: dict, relative: str) -> str:
        """取得指定路径的唯一分类。"""

        matches = [item for item in plan["actions"] if item["path"] == relative]
        self.assertEqual(1, len(matches))
        return matches[0]["classification"]
