#!/usr/bin/env python3
"""覆盖 Tauri 更新日志资源映射与候选字节门禁。"""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import verify_release_notes_resource as verifier


class VerifyReleaseNotesResourceTests(unittest.TestCase):
    """验证映射、逐字节比较和符号链接失败关闭。"""

    def setUp(self) -> None:
        """创建包含 GUI 发布配置与根更新日志的隔离项目。"""

        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.gui_root = self.root / "sample_gui"
        self.config_path = self.gui_root / verifier.RELEASE_CONFIG
        self.config_path.parent.mkdir(parents=True)
        (self.gui_root / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
        self.source = self.root / "release-notes.json"
        self.source.write_text(
            '{"schemaVersion":2,"releases":[]}\n', encoding="utf-8"
        )
        self.config_path.write_text(
            json.dumps(
                {
                    "bundle": {
                        "resources": {
                            verifier.ROOT_CARGO_SOURCE_MAPPING: verifier.RESOURCE_TARGET
                        }
                    }
                }
            ),
            encoding="utf-8",
        )

    def tearDown(self) -> None:
        """回收隔离项目。"""

        self.temporary.cleanup()

    def test_accepts_fixed_config_and_byte_identical_bundled_resource(self) -> None:
        """固定映射与逐字节一致的候选资源应返回同一摘要。"""

        bundled = self.root / "bundle" / "release-notes.json"
        bundled.parent.mkdir()
        bundled.write_bytes(self.source.read_bytes())
        config_digest = verifier.verify_config(self.root, self.gui_root)
        bundled_digest = verifier.verify_bytes(self.source, bundled)
        self.assertEqual(config_digest, bundled_digest)

    def test_rejects_missing_or_redirected_resource_mapping(self) -> None:
        """缺少映射或目标路径漂移都必须在构建前阻断。"""

        for resources in (
            {},
            {verifier.ROOT_CARGO_SOURCE_MAPPING: "nested/release-notes.json"},
            {
                verifier.CONVENTIONAL_CARGO_SOURCE_MAPPING: verifier.RESOURCE_TARGET
            },
        ):
            with self.subTest(resources=resources):
                self.config_path.write_text(
                    json.dumps({"bundle": {"resources": resources}}),
                    encoding="utf-8",
                )
                with self.assertRaisesRegex(
                    verifier.ResourceVerificationError, "fixed release-notes mapping"
                ):
                    verifier.verify_config(self.root, self.gui_root)

    def test_accepts_conventional_src_tauri_cargo_root_mapping(self) -> None:
        """传统 src-tauri Cargo 根必须按其真实解析基准接受两级上跳。"""

        (self.gui_root / "Cargo.toml").unlink()
        (self.gui_root / "src-tauri" / "Cargo.toml").write_text(
            "[package]\n", encoding="utf-8"
        )
        self.config_path.write_text(
            json.dumps(
                {
                    "bundle": {
                        "resources": {
                            verifier.CONVENTIONAL_CARGO_SOURCE_MAPPING: verifier.RESOURCE_TARGET
                        }
                    }
                }
            ),
            encoding="utf-8",
        )

        self.assertEqual(
            verifier.verify_config(self.root, self.gui_root),
            verifier._sha256(self.source.read_bytes()),
        )

    def test_rejects_missing_or_ambiguous_cargo_manifest_root(self) -> None:
        """缺失或同时存在两个 Cargo 根时不得猜测 Tauri 的资源解析基准。"""

        root_manifest = self.gui_root / "Cargo.toml"
        root_manifest.unlink()
        with self.assertRaisesRegex(
            verifier.ResourceVerificationError, "exactly one supported Cargo manifest"
        ):
            verifier.verify_config(self.root, self.gui_root)

        root_manifest.write_text("[package]\n", encoding="utf-8")
        (self.gui_root / "src-tauri" / "Cargo.toml").write_text(
            "[package]\n", encoding="utf-8"
        )
        with self.assertRaisesRegex(
            verifier.ResourceVerificationError, "exactly one supported Cargo manifest"
        ):
            verifier.verify_config(self.root, self.gui_root)

    def test_rejects_bundled_bytes_that_differ_from_source(self) -> None:
        """候选资源即使是合法 JSON，只要字节不同也不能通过。"""

        bundled = self.root / "bundled.json"
        bundled.write_text('{"schemaVersion":2,"releases":[]} ', encoding="utf-8")
        with self.assertRaisesRegex(
            verifier.ResourceVerificationError, "do not match"
        ):
            verifier.verify_bytes(self.source, bundled)

    def test_rejects_symlinked_source_or_bundled_resource(self) -> None:
        """源文件与候选文件都不得通过符号链接替换。"""

        bundled_target = self.root / "bundled-target.json"
        bundled_target.write_bytes(self.source.read_bytes())
        bundled_link = self.root / "bundled-link.json"
        bundled_link.symlink_to(bundled_target)
        with self.assertRaisesRegex(
            verifier.ResourceVerificationError, "regular non-symlink"
        ):
            verifier.verify_bytes(self.source, bundled_link)

        source_target = self.root / "source-target.json"
        source_target.write_bytes(self.source.read_bytes())
        self.source.unlink()
        self.source.symlink_to(source_target)
        with self.assertRaisesRegex(
            verifier.ResourceVerificationError, "regular non-symlink"
        ):
            verifier.verify_config(self.root, self.gui_root)


if __name__ == "__main__":
    unittest.main()
