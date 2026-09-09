"""覆盖 Rust、前端与通用文本分层行数门禁的边界语义。"""

from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from check_file_line_limits import (
    DEFAULT_HARD_LINE_LIMIT,
    DEFAULT_REVIEW_THRESHOLD,
    FRONTEND_CODE_SUFFIXES,
    FRONTEND_HARD_LINE_LIMIT,
    FRONTEND_REVIEW_THRESHOLD,
    RUST_HARD_LINE_LIMIT,
    RUST_REVIEW_THRESHOLD,
    inspect_repository,
    main,
)


class RepositoryFixture(unittest.TestCase):
    """为每个场景建立独立 Git 根并提供可见文件写入辅助。"""

    def setUp(self) -> None:
        """创建不会依赖用户 Git 身份的临时仓库。"""

        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        subprocess.run(
            ["git", "init", "--quiet", "--initial-branch=main", "."],
            cwd=self.root,
            check=True,
        )

    def tearDown(self) -> None:
        """移除场景仓库和所有测试文件。"""

        self.temporary.cleanup()

    def write(self, relative: str, payload: str | bytes) -> Path:
        """写入一个文件；字符串固定使用 UTF-8。"""

        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(payload, bytes):
            path.write_bytes(payload)
        else:
            path.write_text(payload, encoding="utf-8")
        return path

    def track(self, *relative: str) -> None:
        """把指定路径加入索引而不创建提交。"""

        subprocess.run(["git", "add", "--", *relative], cwd=self.root, check=True)


class FileLineLimitTests(RepositoryFixture):
    """验证三组阈值、Git 可见范围和封闭生成文件分类。"""

    def test_generic_text_accepts_500_lines_with_or_without_final_newline(self) -> None:
        """通用文本恰好 500 行应通过，尾部换行不增加物理行。"""

        self.write(
            "tracked.txt", "\n".join("x" for _ in range(DEFAULT_REVIEW_THRESHOLD))
        )
        self.write("untracked.txt", "y\n" * DEFAULT_REVIEW_THRESHOLD)
        self.track("tracked.txt")
        report = inspect_repository(self.root)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["checkedTextFiles"], 2)
        self.assertEqual(report["reviewCandidates"], [])

    def test_generic_text_reports_501_through_2000_without_hard_failure(self) -> None:
        """通用文本软阈值以上到硬上限应列为非阻断复核候选。"""

        self.write("tracked.md", "x\n" * (DEFAULT_REVIEW_THRESHOLD + 1))
        self.write("nested/untracked.py", "x\n" * DEFAULT_HARD_LINE_LIMIT)
        self.track("tracked.md")
        report = inspect_repository(self.root)
        self.assertTrue(report["ok"], report)
        self.assertEqual(
            [item["path"] for item in report["reviewCandidates"]],
            ["nested/untracked.py", "tracked.md"],
        )
        self.assertTrue(
            all(
                item["profile"] == "maintained_text"
                for item in report["reviewCandidates"]
            )
        )
        self.assertEqual(report["violations"], [])

    def test_generic_text_rejects_2001_tracked_and_untracked_lines(self) -> None:
        """通用文本超过 2000 行时，无论是否已跟踪都必须失败。"""

        payload = "\n".join("x" for _ in range(DEFAULT_HARD_LINE_LIMIT + 1))
        self.write("tracked.md", payload)
        self.write("nested/untracked.py", payload)
        self.track("tracked.md")
        report = inspect_repository(self.root)
        self.assertFalse(report["ok"])
        self.assertEqual(
            [item["path"] for item in report["violations"]],
            ["nested/untracked.py", "tracked.md"],
        )

    def test_rust_uses_400_line_review_and_800_line_hard_limits(self) -> None:
        """Rust 的 400/401/800/801 边界必须选择 Rust 配置。"""

        self.write("src/within.rs", "x\n" * RUST_REVIEW_THRESHOLD)
        self.write("src/review.rs", "x\n" * (RUST_REVIEW_THRESHOLD + 1))
        self.write("src/domain/mod.rs", "x\n" * RUST_HARD_LINE_LIMIT)
        self.write("src/violation.rs", "x\n" * (RUST_HARD_LINE_LIMIT + 1))
        report = inspect_repository(self.root)
        self.assertFalse(report["ok"], report)
        self.assertEqual(
            [
                (item["path"], item["profile"], item["threshold"], item["limit"])
                for item in report["reviewCandidates"]
            ],
            [
                ("src/domain/mod.rs", "rust", 400, 800),
                ("src/review.rs", "rust", 400, 800),
            ],
        )
        self.assertEqual(
            [
                (item["path"], item["profile"], item["limit"])
                for item in report["violations"]
            ],
            [("src/violation.rs", "rust", 800)],
        )

    def test_frontend_uses_500_line_review_and_1000_line_hard_limits(self) -> None:
        """前端源码的 500/501/1000/1001 边界必须选择前端配置。"""

        self.write("src/Within.tsx", "x\n" * FRONTEND_REVIEW_THRESHOLD)
        self.write("src/review.ts", "x\n" * (FRONTEND_REVIEW_THRESHOLD + 1))
        self.write("src/page.jsx", "x\n" * FRONTEND_HARD_LINE_LIMIT)
        self.write("src/violation.css", "x\n" * (FRONTEND_HARD_LINE_LIMIT + 1))
        report = inspect_repository(self.root)
        self.assertFalse(report["ok"], report)
        self.assertEqual(
            [
                (item["path"], item["profile"], item["threshold"], item["limit"])
                for item in report["reviewCandidates"]
            ],
            [
                ("src/page.jsx", "frontend", 500, 1000),
                ("src/review.ts", "frontend", 500, 1000),
            ],
        )
        self.assertEqual(
            [
                (item["path"], item["profile"], item["limit"])
                for item in report["violations"]
            ],
            [("src/violation.css", "frontend", 1000)],
        )

    def test_all_declared_frontend_suffixes_select_frontend_profile(self) -> None:
        """每个公开声明的前端后缀都必须稳定采用 500/1000 配置。"""

        for index, suffix in enumerate(sorted(FRONTEND_CODE_SUFFIXES)):
            self.write(
                f"frontend/example_{index}{suffix}",
                "x\n" * (FRONTEND_REVIEW_THRESHOLD + 1),
            )
        report = inspect_repository(self.root)
        self.assertTrue(report["ok"], report)
        self.assertEqual(len(report["reviewCandidates"]), len(FRONTEND_CODE_SUFFIXES))
        self.assertTrue(
            all(
                item["profile"] == "frontend"
                and item["threshold"] == FRONTEND_REVIEW_THRESHOLD
                and item["limit"] == FRONTEND_HARD_LINE_LIMIT
                for item in report["reviewCandidates"]
            )
        )

    def test_handles_hidden_and_spaced_paths(self) -> None:
        """所有宿主都必须无歧义处理隐藏且含空格的文件名。"""

        payload = "x\n" * (DEFAULT_HARD_LINE_LIMIT + 1)
        names = [".hidden file.md"]
        for name in names:
            self.write(name, payload)
        self.track(*names)
        report = inspect_repository(self.root)
        self.assertEqual(
            [item["path"] for item in report["violations"]],
            sorted(names),
        )

    @unittest.skipIf(os.name == "nt", "Windows filenames cannot contain newlines")
    def test_handles_newlines_in_paths(self) -> None:
        """支持该文件名的宿主必须以 NUL 清单无歧义处理换行。"""

        payload = "x\n" * (DEFAULT_HARD_LINE_LIMIT + 1)
        name = "line\nbreak.txt"
        self.write(name, payload)
        self.track(name)
        report = inspect_repository(self.root)
        self.assertEqual([item["path"] for item in report["violations"]], [name])

    def test_ignored_binary_and_symlink_targets_do_not_bypass_or_expand_scope(self) -> None:
        """忽略文件、二进制和链接目标不参与人工维护文本计数。"""

        self.write(".gitignore", "ignored.txt\ntarget.txt\n")
        self.write("ignored.txt", "x\n" * (DEFAULT_HARD_LINE_LIMIT + 1))
        self.write(
            "binary.bin", b"header\0" + b"x\n" * (DEFAULT_HARD_LINE_LIMIT + 1)
        )
        self.write("target.txt", "x\n" * (DEFAULT_HARD_LINE_LIMIT + 1))
        self.track(".gitignore", "binary.bin")
        link = self.root / "linked.txt"
        try:
            link.symlink_to(self.root / "target.txt")
        except (OSError, NotImplementedError):
            self.skipTest("当前平台不允许创建符号链接")
        self.track("linked.txt")
        report = inspect_repository(self.root)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["skippedBinaryFiles"], 1)
        self.assertEqual(report["skippedSymlinks"], 1)

    def test_deleted_tracked_file_is_skipped_as_absent_worktree_content(self) -> None:
        """索引中仍存在但工作树已删除的文件不应造成检查器运行错误。"""

        deleted = self.write(
            "deleted.txt", "x\n" * (DEFAULT_HARD_LINE_LIMIT + 1)
        )
        self.track("deleted.txt")
        deleted.unlink()
        report = inspect_repository(self.root)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["skippedMissingFiles"], 1)

    def test_excludes_only_known_generated_lockfile_names(self) -> None:
        """工具锁文件按精确名称排除，相似人工文件仍会失败。"""

        payload = "x\n" * (DEFAULT_HARD_LINE_LIMIT + 1)
        self.write("Cargo.lock", payload)
        self.write("Cargo.lock.notes", payload)
        self.track("Cargo.lock", "Cargo.lock.notes")
        report = inspect_repository(self.root)
        self.assertEqual(report["excludedGeneratedFiles"], ["Cargo.lock"])
        self.assertEqual(
            [item["path"] for item in report["violations"]],
            ["Cargo.lock.notes"],
        )

    def test_invalid_utf8_text_candidate_fails_closed(self) -> None:
        """无 NUL 且无法解码的文件不能静默伪装为二进制。"""

        self.write("unknown.dat", b"\xff\xfe\xfd")
        self.track("unknown.dat")
        report = inspect_repository(self.root)
        self.assertFalse(report["ok"])
        self.assertTrue(any("不是 UTF-8" in item for item in report["errors"]), report)


class FileLineLimitFailureTests(unittest.TestCase):
    """验证项目根、Git 失败和稳定命令行退出码。"""

    def test_text_cli_explains_rust_directory_mod_rs_structure(self) -> None:
        """Rust 硬超限诊断必须给出目录加 mod.rs 的拆分结构。"""

        report = {
            "ok": False,
            "errors": [],
            "reviewCandidates": [],
            "violations": [
                {
                    "path": "src/domain.rs",
                    "lines": 801,
                    "profile": "rust",
                    "threshold": 400,
                    "limit": 800,
                }
            ],
        }
        stdout = io.StringIO()
        stderr = io.StringIO()
        with mock.patch(
            "check_file_line_limits.inspect_repository", return_value=report
        ), redirect_stdout(stdout), redirect_stderr(stderr):
            self.assertEqual(main([]), 1)
        self.assertIn("<module>/mod.rs", stderr.getvalue())

    def test_non_git_directory_is_operational_failure(self) -> None:
        """缺少 Git 清单时必须返回检查器错误而不是空通过。"""

        with tempfile.TemporaryDirectory() as temporary:
            report = inspect_repository(Path(temporary))
        self.assertFalse(report["ok"])
        self.assertTrue(any("Git 文件枚举失败" in item for item in report["errors"]), report)

    def test_git_execution_error_fails_closed(self) -> None:
        """Git 无法启动时必须保留明确诊断。"""

        with tempfile.TemporaryDirectory() as temporary, mock.patch(
            "check_file_line_limits.subprocess.run",
            side_effect=OSError("missing git"),
        ):
            report = inspect_repository(Path(temporary))
        self.assertTrue(any("无法执行 Git" in item for item in report["errors"]), report)

    def test_json_cli_uses_zero_one_two_exit_contract(self) -> None:
        """JSON 入口分别用 0/1/2 表示通过、超限和运行错误。"""

        reports = (
            ({"ok": True, "errors": [], "reviewCandidates": [], "violations": []}, 0),
            ({"ok": False, "errors": [], "reviewCandidates": [], "violations": [{}]}, 1),
            ({"ok": False, "errors": ["failure"], "reviewCandidates": [], "violations": []}, 2),
        )
        for report, expected in reports:
            stdout = io.StringIO()
            stderr = io.StringIO()
            with self.subTest(expected=expected), mock.patch(
                "check_file_line_limits.inspect_repository",
                return_value=report,
            ), redirect_stdout(stdout), redirect_stderr(stderr):
                self.assertEqual(main(["--json"]), expected)
                self.assertEqual(json.loads(stdout.getvalue()), report)


if __name__ == "__main__":
    unittest.main()
