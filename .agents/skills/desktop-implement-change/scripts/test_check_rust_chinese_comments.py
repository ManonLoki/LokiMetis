"""覆盖 Cargo package/workspace 的 Rust 中文声明文档注释门禁。"""

from __future__ import annotations

from contextlib import redirect_stderr, redirect_stdout
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from check_rust_chinese_comments import inspect_project, main


class RustCommentFixture(unittest.TestCase):
    """为每个场景建立隔离 Cargo 项目目录。"""

    def setUp(self) -> None:
        """创建临时项目根。"""

        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)

    def tearDown(self) -> None:
        """移除场景创建的全部临时文件。"""

        self.temporary.cleanup()

    def write(self, relative: str, payload: str | bytes) -> Path:
        """写入一个 UTF-8 文本或原始字节 fixture。"""

        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        if isinstance(payload, bytes):
            path.write_bytes(payload)
        else:
            path.write_text(payload, encoding="utf-8")
        return path

    def package(self, relative: str = ".", source: str = "") -> None:
        """创建带最小 manifest 与 lib.rs 的 Cargo package。"""

        prefix = "" if relative == "." else f"{relative}/"
        name = "root_package" if relative == "." else relative.replace("/", "_")
        self.write(
            f"{prefix}Cargo.toml",
            f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "2024"\n',
        )
        self.write(f"{prefix}src/lib.rs", source)


class RustChineseCommentTests(RustCommentFixture):
    """验证 workspace、声明类别、attribute、宏与失败关闭边界。"""

    def test_accepts_workspace_and_all_governed_declarations(self) -> None:
        """virtual workspace 应聚合 member 的生产、测试和构建声明。"""

        self.write("Cargo.toml", '[workspace]\nmembers = ["core", "gui"]\nresolver = "3"\n')
        self.package(
            "core",
            r'''
/// 保存任务状态。
pub struct Task;
/// 表示任务阶段。
pub enum Phase { Ready }
/// 表示共享内存值。
pub union Shared { value: u64 }
/// 定义执行器约束。
pub trait Runner {
    /// 执行任务。
    fn run(&self);
}
/// 表示任务标识。
pub type TaskId = u64;
/// 查询当前任务。
pub async fn load<'a>(_value: &'a str) {}
''',
        )
        self.package(
            "gui",
            r'''
/// 启动 GUI 宿主。
fn main() {}
''',
        )
        self.write("gui/build.rs", "/// 执行构建准备。\nfn main() {}\n")
        self.write(
            "gui/tests/gui.rs",
            "/// 验证 GUI 状态。\n#[test]\nfn reports_status() {}\n",
        )

        report = inspect_project(self.root)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["checkedPackages"], 2)
        self.assertEqual(report["checkedRustFiles"], 4)
        self.assertEqual(report["checkedDeclarations"], 10)

    def test_accepts_outer_attributes_and_rejects_non_item_evidence(self) -> None:
        """直接或条件 doc attribute 可用，其他非条目证据不能替代。"""

        self.package(
            source=r'''
//! 中文模块说明不能替代条目说明。
#[doc = "保存有效结构。"]
#[derive(Clone)]
struct Valid;
#[cfg_attr(all(), doc = "保存条件结构。")]
struct Conditional;
/// English only.
struct English;
#[allow(doc = "这不是文档属性")]
struct NestedButInvalid;
// 普通中文注释不属于 Rust 文档。
fn ordinary() {}
/// 中文实现说明只属于 impl。
impl Valid { fn method(&self) {} }
''',
        )

        report = inspect_project(self.root)
        self.assertEqual(
            [(item["kind"], item["name"]) for item in report["violations"]],
            [
                ("struct", "English"),
                ("struct", "NestedButInvalid"),
                ("fn", "ordinary"),
                ("fn", "method"),
            ],
        )

    def test_ignores_macro_bodies_strings_comments_and_function_pointer_tokens(self) -> None:
        """宏展开、字符串、普通注释和函数指针不得制造伪声明。"""

        self.package(
            source=r'''
// struct CommentOnly; fn comment_only() {}
const TEXT: &str = "trait StringOnly { fn string_only(); }";
/// 表示回调类型。
type Callback = fn(u64) -> u64;
macro_rules! make_function { ($name:ident) => { fn $name() {} }; }
const TOKENS: &str = stringify!(fn generated() {});
/// 返回原始输入。
fn identity(value: u64) -> u64 { value }
''',
        )

        report = inspect_project(self.root)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["checkedDeclarations"], 2)

    def test_reports_unbalanced_source_invalid_utf8_nul_and_symlinks(self) -> None:
        """残缺或不可安全读取的源码必须失败关闭。"""

        self.package(source="/// 中文函数。\nfn broken(\n")
        self.write("src/invalid.rs", b"\xff\xfe")
        self.write(
            "src/nul.rs",
            "/// 中文结构。\nstruct Nul;".encode("utf-8") + b"\0",
        )
        target = self.write("outside.rs", "/// 中文结构。\nstruct Outside;\n")
        link = self.root / "src" / "linked.rs"
        try:
            link.symlink_to(target)
        except (OSError, NotImplementedError):
            self.skipTest("当前平台不允许创建符号链接")

        report = inspect_project(self.root)
        self.assertFalse(report["ok"])
        self.assertTrue(any("未闭合" in item for item in report["errors"]), report)
        self.assertTrue(any("不是 UTF-8" in item for item in report["errors"]), report)
        self.assertTrue(any("NUL" in item for item in report["errors"]), report)
        self.assertTrue(any("符号链接" in item for item in report["errors"]), report)

    def test_rejects_workspace_escape_missing_member_and_empty_package(self) -> None:
        """workspace 路径越界、未匹配 member 和无源码 package 均不得空通过。"""

        self.write(
            "Cargo.toml",
            '[workspace]\nmembers = ["../outside", "missing-*", "empty"]\n',
        )
        self.write(
            "empty/Cargo.toml",
            '[package]\nname = "empty"\nversion = "0.1.0"\nedition = "2024"\n',
        )

        report = inspect_project(self.root)
        self.assertFalse(report["ok"])
        self.assertTrue(any("不得越界" in item for item in report["errors"]), report)
        self.assertTrue(any("未匹配" in item for item in report["errors"]), report)
        self.assertTrue(any("未找到 Rust 源码" in item for item in report["errors"]), report)

    def test_rejects_missing_manifest_and_zero_governed_declarations(self) -> None:
        """伪项目目录和没有受管声明的源码都不得形成成功报告。"""

        self.write("src/lib.rs", "const VALUE: u64 = 1;\n")
        missing_manifest = inspect_project(self.root)
        self.assertFalse(missing_manifest["ok"])
        self.assertTrue(
            any("缺少 Cargo.toml" in item for item in missing_manifest["errors"]),
            missing_manifest,
        )

        self.write(
            "Cargo.toml",
            '[package]\nname = "empty_declarations"\nversion = "0.1.0"\nedition = "2024"\n',
        )
        no_declarations = inspect_project(self.root)
        self.assertFalse(no_declarations["ok"])
        self.assertTrue(
            any("未发现受中文注释门禁管理" in item for item in no_declarations["errors"]),
            no_declarations,
        )


class RustChineseCommentCommandTests(unittest.TestCase):
    """验证稳定 JSON 输出和退出码。"""

    def test_json_command_uses_zero_one_two_exit_contract(self) -> None:
        """JSON 入口分别用 0/1/2 表示通过、注释违规和运行错误。"""

        reports = (
            ({"ok": True, "errors": [], "violations": []}, 0),
            ({"ok": False, "errors": [], "violations": [{}]}, 1),
            ({"ok": False, "errors": ["failure"], "violations": []}, 2),
        )
        for report, expected in reports:
            stdout = io.StringIO()
            stderr = io.StringIO()
            with self.subTest(expected=expected), mock.patch(
                "check_rust_chinese_comments.inspect_project", return_value=report
            ), redirect_stdout(stdout), redirect_stderr(stderr):
                self.assertEqual(main(["--json"]), expected)
                self.assertEqual(json.loads(stdout.getvalue()), report)


if __name__ == "__main__":
    unittest.main()
