#!/usr/bin/env python3
from __future__ import annotations

import os
import plistlib
import shutil
import struct
import subprocess
import tempfile
import unittest
import zlib
from pathlib import Path


SCRIPT = Path(__file__).with_name("verify-dmg-layout.sh")


def png_chunk(kind: bytes, payload: bytes) -> bytes:
    """构造带真实 CRC 的 PNG chunk。"""
    checksum = zlib.crc32(payload, zlib.crc32(kind)) & 0xFFFFFFFF
    return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", checksum)


def png_image(
    width: int,
    height: int,
    *,
    compressed_pixels: bytes | None = None,
) -> bytes:
    """构造可由标准 PNG 解析器读取的 RGBA 图片。"""
    rows = b"".join(b"\0" + b"\0" * (width * 4) for _ in range(height))
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", header)
        + png_chunk(
            b"IDAT",
            zlib.compress(rows) if compressed_pixels is None else compressed_pixels,
        )
        + png_chunk(b"IEND", b"")
    )


def finder_record(name: str, code: bytes, payload: bytes) -> bytes:
    """编码一个 blob 类型的 DS_Store B-tree 记录。"""
    encoded_name = name.encode("utf-16be")
    return (
        struct.pack(">I", len(encoded_name) // 2)
        + encoded_name
        + code
        + b"blob"
        + struct.pack(">I", len(payload))
        + payload
    )


def icon_location(x: int, y: int) -> bytes:
    """编码 Finder Iloc 的标准 16 字节值。"""
    return struct.pack(">IIII", x, y, 0xFFFFFFFF, 0xFFFF0000)


def alias_field(tag: int, value: bytes) -> bytes:
    """编码 Carbon Alias v2 的一个带偶数字节对齐的扩展字段。"""
    return struct.pack(">hh", tag, len(value)) + value + (b"\0" if len(value) & 1 else b"")


def background_alias(*, posix_path: str = "/.background/background.png") -> bytes:
    """构造与 Finder/create-dmg 一致的卷内背景 Alias 关键字段。"""
    target = "background.png"
    volume = b"Test Volume"
    content = bytearray(150)
    struct.pack_into(">4sHH", content, 0, b"\0" * 4, 0, 2)
    struct.pack_into(">H", content, 8, 0)
    content[10] = len(volume)
    content[11 : 11 + len(volume)] = volume
    encoded_target = target.encode("mac_roman")
    content[50] = len(encoded_target)
    content[51 : 51 + len(encoded_target)] = encoded_target
    unicode_target = target.encode("utf-16be")
    fields = b"".join(
        (
            alias_field(0, b".background"),
            alias_field(2, volume + b":.background:\0background.png"),
            alias_field(14, struct.pack(">H", len(target)) + unicode_target),
            alias_field(18, posix_path.encode("utf-8")),
            alias_field(19, b"/Volumes/Test Volume"),
            struct.pack(">hH", -1, 0),
        )
    )
    content.extend(fields)
    struct.pack_into(">H", content, 4, len(content))
    return bytes(content)


def ds_store(
    app_position: tuple[int, int] = (180, 220),
    applications_position: tuple[int, int] = (480, 220),
    window_size: tuple[int, int] = (660, 400),
    *,
    mode: str = "valid",
) -> bytes:
    """构造带真实 buddy allocator、DSDB superblock 和叶节点的最小布局。"""
    allocator_offset = 2_048
    allocator_size = 2_048
    superblock_offset = 4_096
    superblock_size = 32
    if mode == "invalid-signed-alias-size":
        tree_offset = 65_536
        tree_block_size = 65_536
        page_size = 65_536
    else:
        tree_offset = 8_192
        tree_block_size = 8_192 if mode == "oversized-tree-allocation" else 1_024
        page_size = 4_096
    content = bytearray(tree_offset + 4 + tree_block_size)
    struct.pack_into(
        ">I4sIII16s",
        content,
        0,
        1,
        b"Bud1",
        allocator_offset,
        allocator_size,
        allocator_offset,
        b"\0" * 16,
    )

    allocator = bytearray(allocator_size)
    struct.pack_into(">II", allocator, 0, 3, 0)
    offsets = [
        allocator_offset | 11,
        superblock_offset | 5,
        tree_offset | (tree_block_size.bit_length() - 1),
    ] + [0] * 253
    struct.pack_into(">256I", allocator, 8, *offsets)
    toc_cursor = 8 + 256 * 4
    struct.pack_into(">I", allocator, toc_cursor, 1)
    toc_cursor += 4
    allocator[toc_cursor] = 4
    allocator[toc_cursor + 1 : toc_cursor + 5] = b"DSDB"
    struct.pack_into(">I", allocator, toc_cursor + 5, 1)
    content[allocator_offset + 4 : allocator_offset + 4 + allocator_size] = allocator

    bounds = f"{{{{100, 100}}, {{{window_size[0]}, {window_size[1]}}}}}"
    alias = background_alias(
        posix_path="/.background/not-background.png"
        if mode == "wrong-background-alias"
        else "/.background/background.png"
    )
    if mode == "empty-background-alias":
        alias = b""
    elif mode == "invalid-signed-alias-size":
        oversized_alias = bytearray(alias)
        filler_length = 32_768 - len(oversized_alias) - 4
        oversized_alias[-4:-4] = alias_field(99, b"\0" * filler_length)
        struct.pack_into(">H", oversized_alias, 4, len(oversized_alias))
        alias = bytes(oversized_alias)
    record_values = [
        (
            "Child" if mode == "only-other-name-bwsp" else ".",
            b"bwsp",
            plistlib.dumps({"WindowBounds": bounds}, fmt=plistlib.FMT_BINARY),
        ),
        (
            ".",
            b"icvp",
            plistlib.dumps(
                {
                    "backgroundType": 1 if mode == "wrong-background-type" else 2,
                    "backgroundImageAlias": alias,
                },
                fmt=plistlib.FMT_BINARY,
            ),
        ),
        ("Applications", b"Iloc", icon_location(*applications_position)),
        ("Test App.app", b"Iloc", icon_location(*app_position)),
    ]
    if mode == "missing-icvp":
        record_values = [item for item in record_values if item[1] != b"icvp"]
    record_values.sort(key=lambda item: (item[0].lower(), item[1]))
    if mode == "unsorted-tree":
        record_values[-2:] = reversed(record_values[-2:])
    records = [finder_record(*item) for item in record_values]
    levels = 1 if mode == "wrong-tree-level" else 0
    struct.pack_into(
        ">IIIII",
        content,
        superblock_offset + 4,
        2,
        levels,
        len(records),
        1,
        page_size,
    )
    # Finder can use a compact block, while some writers allocate an 8192-byte
    # buddy bucket for the same 4096-byte logical page.
    tree = bytearray(tree_block_size)
    struct.pack_into(">II", tree, 0, 0, len(records))
    cursor = 8
    for record in records:
        tree[cursor : cursor + len(record)] = record
        cursor += len(record)
    if cursor > tree_block_size:
        raise AssertionError("test DS_Store records exceed the compact tree block")
    content[tree_offset + 4 : tree_offset + 4 + tree_block_size] = tree
    return bytes(content)


def executable(path: Path, content: str) -> None:
    """创建隔离 hdiutil 替身，不读取或挂载真实磁盘镜像。"""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")
    path.chmod(0o755)


def fake_hdiutil(root: Path) -> Path:
    """挂载预先构造的隔离卷夹具，并记录每次 detach。"""
    probe = root / "probe"
    detached = root / "detached"
    attached = root / "attached"
    executable(
        probe / "hdiutil",
        f"""#!/bin/sh
set -eu
if [ "$1" = attach ]; then
  : > '{attached}'
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
  cp -R "$AFH_DMG_FIXTURE_ROOT"/. "$mount"/
  printf '%s\n' '/dev/disk-test Apple_HFS Test'
elif [ "$1" = detach ]; then
  : > '{detached}'
else
  exit 2
fi
""",
    )
    return probe


def volume_fixture(root: Path, release_notes: Path, mode: str) -> Path:
    """生成最终卷结构；故障模式只改变被测试的单一契约。"""
    fixture = root / "volume"
    app = fixture / "Test App.app"
    contents = app / "Contents"
    resources = contents / "Resources"
    background = fixture / ".background"
    app.mkdir(parents=True)
    if mode == "symlinked-app-contents":
        external_contents = root / "external-contents"
        (external_contents / "Resources").mkdir(parents=True)
        contents.symlink_to(external_contents, target_is_directory=True)
    else:
        contents.mkdir()
        if mode == "symlinked-resource-directory":
            external_resources = root / "external-resources"
            external_resources.mkdir()
            resources.symlink_to(external_resources, target_is_directory=True)
        else:
            resources.mkdir()
    if mode == "symlinked-background-directory":
        external_background = root / "external-background"
        external_background.mkdir()
        background.symlink_to(external_background, target_is_directory=True)
    else:
        background.mkdir()

    if mode != "missing-background":
        dimensions = (640, 400) if mode == "wrong-background-size" else (660, 400)
        image = png_image(
            *dimensions,
            compressed_pixels=b"not-a-zlib-stream"
            if mode == "corrupt-png-pixels"
            else None,
        )
        if mode == "corrupt-png-crc":
            idat_type = image.index(b"IDAT")
            idat_length = struct.unpack_from(">I", image, idat_type - 4)[0]
            crc_offset = idat_type + 4 + idat_length
            image = (
                image[:crc_offset]
                + bytes((image[crc_offset] ^ 1,))
                + image[crc_offset + 1 :]
            )
        (background / "background.png").write_bytes(image)

    if mode != "missing-ds-store":
        if mode == "corrupt-ds-store":
            layout = b"not-a-ds-store"
        else:
            layout = ds_store(
                app_position=(181, 220) if mode == "wrong-app-position" else (180, 220),
                applications_position=(481, 220)
                if mode == "wrong-applications-position"
                else (480, 220),
                window_size=(659, 400) if mode == "wrong-window-size" else (660, 400),
                mode=mode,
            )
        (fixture / ".DS_Store").write_bytes(layout)

    target = "/tmp" if mode == "wrong-applications" else "/Applications"
    (fixture / "Applications").symlink_to(target)
    if mode == "multiple-apps":
        (fixture / "Second App.app").mkdir()
    if mode != "missing-release-notes":
        if mode == "mismatched-release-notes":
            (resources / "release-notes.json").write_text("mismatch", encoding="utf-8")
        else:
            shutil.copyfile(release_notes, resources / "release-notes.json")
    return fixture


class VerifyDmgLayoutTests(unittest.TestCase):
    """覆盖最终 DMG 布局成功路径与最危险的空白 Finder 窗口失败。"""

    def run_check(
        self,
        root: Path,
        dmg: Path,
        *,
        mode: str = "valid",
        platform: str = "Darwin",
        parser_available: bool = True,
    ) -> subprocess.CompletedProcess[str]:
        """在伪 macOS 与隔离 hdiutil 下运行只读检查。"""
        env = os.environ.copy()
        release_notes = root / "release-notes.json"
        if not release_notes.exists():
            release_notes.write_text(
                '{"schemaVersion":2,"releases":[]}\n', encoding="utf-8"
            )
        fixture = volume_fixture(root, release_notes, mode)
        python3 = shutil.which("python3")
        if python3 is None:
            self.fail("python3 is required to exercise the standard-library layout parser")
        prerequisite_paths = [str(fake_hdiutil(root))]
        if parser_available:
            prerequisite_paths.append(str(Path(python3).parent))
        env.update(
            {
                "AFH_PREREQ_PATH": os.pathsep.join(prerequisite_paths),
                "AFH_TEST_PLATFORM": platform,
                "AFH_ALLOW_TEST_OVERRIDES": "1",
                "AFH_DMG_FIXTURE_ROOT": str(fixture),
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
            self.assertIn("background_dimensions=660x400", result.stdout)
            self.assertIn("finder_window_size=660x400", result.stdout)
            self.assertIn("app_position=180,220", result.stdout)
            self.assertIn("applications_position=480,220", result.stdout)
            self.assertIn("background_binding=/.background/background.png", result.stdout)
            self.assertIn("release_notes=byte-identical", result.stdout)
            self.assertTrue((root / "detached").is_file())

    def test_larger_physical_buddy_block_is_accepted(self) -> None:
        """8192 字节物理块可承载 4096 字节逻辑页，不能误拒真实 dmgbuild 卷。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            result = self.run_check(root, dmg, mode="oversized-tree-allocation")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("gate.macos_dmg_layout.status=passed", result.stdout)
            self.assertTrue((root / "detached").is_file())

    def test_wrong_background_or_finder_window_size_is_rejected(self) -> None:
        """背景像素和 Finder 窗口尺寸都必须来自最终卷并精确为 660×400。"""
        for mode, reason in (
            ("wrong-background-size", "background-dimensions-invalid"),
            ("wrong-window-size", "finder-window-size-invalid"),
        ):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                dmg = root / "candidate.dmg"
                dmg.write_bytes(b"dmg")
                result = self.run_check(root, dmg, mode=mode)
                self.assertEqual(result.returncode, 41, result.stderr)
                self.assertIn(reason, result.stdout)
                self.assertTrue((root / "detached").is_file())

    def test_corrupt_png_stream_or_crc_fails_closed(self) -> None:
        """合法签名与尺寸不能掩盖损坏的像素流或 chunk CRC。"""
        for mode in ("corrupt-png-pixels", "corrupt-png-crc"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                dmg = root / "candidate.dmg"
                dmg.write_bytes(b"dmg")
                result = self.run_check(root, dmg, mode=mode)
                self.assertEqual(result.returncode, 41, result.stderr)
                self.assertIn("background-image-unparseable", result.stdout)
                self.assertTrue((root / "detached").is_file())

    def test_wrong_icon_coordinates_are_rejected(self) -> None:
        """应用和 Applications 任一 Iloc 坐标漂移都必须拒绝。"""
        for mode, reason in (
            ("wrong-app-position", "app-icon-position-invalid"),
            ("wrong-applications-position", "applications-icon-position-invalid"),
        ):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                dmg = root / "candidate.dmg"
                dmg.write_bytes(b"dmg")
                result = self.run_check(root, dmg, mode=mode)
                self.assertEqual(result.returncode, 41, result.stderr)
                self.assertIn(reason, result.stdout)
                self.assertTrue((root / "detached").is_file())

    def test_unparseable_ds_store_fails_closed(self) -> None:
        """非空但无法解析的 .DS_Store 不能冒充已写入的 Finder 布局。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            result = self.run_check(root, dmg, mode="corrupt-ds-store")
            self.assertEqual(result.returncode, 41, result.stderr)
            self.assertIn("finder-layout-unparseable", result.stdout)
            self.assertTrue((root / "detached").is_file())

    def test_root_window_record_is_required(self) -> None:
        """子项 bwsp 不能冒充根卷 Finder 窗口状态。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            result = self.run_check(root, dmg, mode="only-other-name-bwsp")
            self.assertEqual(result.returncode, 41, result.stderr)
            self.assertIn("finder-layout-unparseable", result.stdout)
            self.assertTrue((root / "detached").is_file())

    def test_background_view_and_alias_binding_are_required(self) -> None:
        """根 icvp、图片类型和精确背景 Alias 缺一不可。"""
        for mode in (
            "missing-icvp",
            "wrong-background-type",
            "empty-background-alias",
            "wrong-background-alias",
            "invalid-signed-alias-size",
        ):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                dmg = root / "candidate.dmg"
                dmg.write_bytes(b"dmg")
                result = self.run_check(root, dmg, mode=mode)
                self.assertEqual(result.returncode, 41, result.stderr)
                self.assertIn("finder-background-binding-invalid", result.stdout)
                self.assertTrue((root / "detached").is_file())

    def test_btree_depth_and_key_order_are_validated(self) -> None:
        """伪造层级或乱序活动记录都不得通过 DSDB 完整性检查。"""
        for mode in ("wrong-tree-level", "unsorted-tree"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                dmg = root / "candidate.dmg"
                dmg.write_bytes(b"dmg")
                result = self.run_check(root, dmg, mode=mode)
                self.assertEqual(result.returncode, 41, result.stderr)
                self.assertIn("finder-layout-unparseable", result.stdout)
                self.assertTrue((root / "detached").is_file())

    def test_missing_layout_parser_runtime_fails_before_mount(self) -> None:
        """没有标准库解析运行时时不得降级成仅检查文件存在。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            result = self.run_check(root, dmg, parser_available=False)
            self.assertEqual(result.returncode, 41, result.stderr)
            self.assertIn("layout-parser-runtime-missing", result.stdout)
            self.assertFalse((root / "detached").exists())

    def test_prerequisite_path_cannot_override_real_host_tools(self) -> None:
        """未进入显式测试宿主时，探针路径不能替换真实 hdiutil。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            release_notes = root / "release-notes.json"
            release_notes.write_text(
                '{"schemaVersion":2,"releases":[]}\n', encoding="utf-8"
            )
            env = os.environ.copy()
            env["AFH_PREREQ_PATH"] = str(fake_hdiutil(root))
            env.pop("AFH_TEST_PLATFORM", None)
            env.pop("AFH_ALLOW_TEST_OVERRIDES", None)
            result = subprocess.run(
                ["/bin/sh", str(SCRIPT), str(dmg), str(release_notes)],
                text=True,
                capture_output=True,
                env=env,
                timeout=10,
                check=False,
            )
            self.assertEqual(result.returncode, 41, result.stderr)
            self.assertIn("prerequisite-path-override-not-allowed", result.stdout)
            self.assertFalse((root / "attached").exists())

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

    def test_background_parent_symlink_cannot_escape_final_volume(self) -> None:
        """卷外合法 PNG 不能经 `.background` 目录链接冒充打包资源。"""
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            dmg = root / "candidate.dmg"
            dmg.write_bytes(b"dmg")
            result = self.run_check(root, dmg, mode="symlinked-background-directory")
            self.assertEqual(result.returncode, 41, result.stderr)
            self.assertIn("background-directory-invalid", result.stdout)
            self.assertTrue((root / "detached").is_file())

    def test_release_notes_parent_symlinks_cannot_escape_app_bundle(self) -> None:
        """Contents 或 Resources 指向卷外时，即使日志可读也必须拒绝。"""
        for mode, reason in (
            ("symlinked-app-contents", "app-contents-directory-invalid"),
            (
                "symlinked-resource-directory",
                "release-notes-resource-directory-invalid",
            ),
        ):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                dmg = root / "candidate.dmg"
                dmg.write_bytes(b"dmg")
                result = self.run_check(root, dmg, mode=mode)
                self.assertEqual(result.returncode, 41, result.stderr)
                self.assertIn(reason, result.stdout)
                self.assertTrue((root / "detached").is_file())

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
