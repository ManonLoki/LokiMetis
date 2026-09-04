#!/usr/bin/env python3
"""集中维护下游 Harness 升级器不可弱化的协议常量。"""

from __future__ import annotations

from pathlib import Path
import re


SCHEMA_VERSION = 2
OWNERSHIP_RELATIVE = Path(
    ".agents/skills/desktop-upgrade-harness/references/ownership-manifest.json"
)
LOCK_RELATIVE = Path(".harness/upstream-lock.json")
VALID_MODES = {
    "managed",
    "managed-self",
    "merge-sections",
    "conditional",
    "protected",
    "tombstone",
}
AUTO_MODES = {"managed", "managed-self"}
BLOCKING_CLASSES = {
    "bootstrap_conflict",
    "collision",
    "conflict",
    "protected_candidate",
    "tombstone_candidate",
    "tombstone_present",
}
MANUAL_CLASSES = {"add", "delete", "manual_add", "manual_merge"}
REQUIRED_MANAGED_SOURCE_PATHS = (
    ".agents/skills/desktop-implement-change/scripts/check_file_line_limits.py",
    ".agents/skills/desktop-implement-change/scripts/test_check_file_line_limits.py",
    ".agents/skills/desktop-implement-change/scripts/check_core_first.py",
    ".agents/skills/desktop-implement-change/scripts/test_check_core_first.py",
    ".agents/skills/desktop-implement-change/scripts/check_rust_chinese_comments.py",
    ".agents/skills/desktop-implement-change/scripts/test_check_rust_chinese_comments.py",
)
MINIMUM_OWNERSHIP_RULES = {
    "Version.md": "tombstone",
    ".agents/skills/desktop-instantiate-project/**": "tombstone",
    ".agents/skills/desktop-initialize-rust-project/**": "tombstone",
    ".agents/skills/desktop-test-gui-initialization-e2e/**": "tombstone",
    ".agents/skills/desktop-add-cli-adapter/**": "tombstone",
    ".agents/skills/desktop-add-tui-adapter/**": "tombstone",
    ".agents/skills/desktop-add-mcp-adapter/**": "tombstone",
    ".agents/skills/desktop-build-rust-release/**": "tombstone",
    ".agents/skills/desktop-prepare-cross-platform-release/**": "tombstone",
    ".agents/skills/desktop-run-parallel-worktrees/**": "tombstone",
    ".agents/skills/desktop-add-gui-updater/**": "tombstone",
    ".agents/skills/desktop-add-gui-system-locale/**": "tombstone",
    ".agents/skills/desktop-add-gui-window-state/**": "tombstone",
    ".agents/skills/desktop-add-gui-dialog/**": "tombstone",
    ".agents/skills/desktop-curate-harness-memory/**": "tombstone",
    "scripts/validate_harness.py": "tombstone",
    "scripts/test_agile_workflow.py": "tombstone",
    "scripts/test_harness_scope_and_initialization_boundaries.py": "tombstone",
    "scripts/test_validate_harness.py": "tombstone",
    "scripts/harness_validation_test_support.py": "tombstone",
    "scripts/harness_validation/**": "tombstone",
    "docs/HARNESS_ENGINEERING.md": "tombstone",
    "docs/harness_engineering/**": "tombstone",
    "docs/RUST_CLI_TEMPLATE.md": "tombstone",
    "docs/CLI_CONTRACT.md": "tombstone",
    "docs/AGENT_POLICY.md": "protected",
    "docs/GUI_SUPPORT_SURFACES.md": "protected",
    "docs/product_spec/**": "protected",
    "docs/project_status/**": "protected",
    "docs/work_plan/**": "protected",
    "docs/adr/**": "protected",
    "docs/changelog/**": "protected",
    "docs/VERIFICATION.md": "protected",
    "docs/verification/**": "protected",
    "docs/TECH_DEBT.md": "protected",
    ".harness/version-state.json": "protected",
    "release-notes.json": "protected",
    "LICENSE.zh-CN.md": "protected",
    "LICENSE.en.md": "protected",
    "Cargo.toml": "protected",
    "Cargo.lock": "protected",
    ".gitignore": "protected",
    "AGENTS.md": "merge-sections",
    "README.md": "merge-sections",
    "docs/ENGINEERING_RULES.md": "merge-sections",
    "docs/RUST_GUI_TEMPLATE.md": "merge-sections",
    "docs/RELEASE.md": "merge-sections",
    "docs/design_standards/**": "managed",
    ".agents/skills/desktop-implement-change/scripts/check_file_line_limits.py": "managed",
    ".agents/skills/desktop-implement-change/scripts/test_check_file_line_limits.py": "managed",
    ".agents/skills/desktop-implement-change/scripts/check_core_first.py": "managed",
    ".agents/skills/desktop-implement-change/scripts/test_check_core_first.py": "managed",
    ".agents/skills/desktop-implement-change/scripts/check_rust_chinese_comments.py": "managed",
    ".agents/skills/desktop-implement-change/scripts/test_check_rust_chinese_comments.py": "managed",
    ".agents/skills/desktop-upgrade-harness/**": "managed-self",
    ".agents/skills/desktop-add-gui-system-tray/**": "conditional",
    ".agents/skills/desktop-add-gui-single-instance/**": "conditional",
    ".agents/skills/desktop-add-gui-deep-link/**": "conditional",
    ".agents/skills/desktop-add-gui-global-shortcut/**": "conditional",
    ".agents/skills/desktop-add-gui-system-notifications/**": "conditional",
    ".agents/skills/desktop-add-gui-autostart/**": "conditional",
    ".agents/skills/desktop-build-tauri-local-install/**": "conditional",
    ".agents/skills/**": "managed",
}
VERSION_PATTERN = re.compile(r"当前版本[：:]\s*`([^`]+)`")
