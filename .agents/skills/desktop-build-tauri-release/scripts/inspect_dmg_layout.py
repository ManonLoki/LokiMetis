#!/usr/bin/env python3
"""Fail-closed inspection of the PNG and active Finder records in a DMG volume."""

from __future__ import annotations

import argparse
import plistlib
import re
import stat
import struct
import sys
import zlib
from dataclasses import dataclass
from pathlib import Path, PurePosixPath


MAX_PNG_BYTES = 64 * 1024 * 1024
MAX_PNG_DECODE_BYTES = 128 * 1024 * 1024
MAX_DS_STORE_BYTES = 32 * 1024 * 1024
MAX_ALLOCATOR_BLOCKS = 65_536
MAX_BTREE_RECORDS = 100_000
WINDOW_BOUNDS = re.compile(
    r"^\{\{\s*(-?\d+)\s*,\s*(-?\d+)\s*\},\s*"
    r"\{\s*(\d+)\s*,\s*(\d+)\s*\}\}$"
)


class InspectionError(Exception):
    """An expected validation failure with a stable gate reason."""

    def __init__(self, reason: str) -> None:
        super().__init__(reason)
        self.reason = reason


class ParseError(Exception):
    """Malformed or unsupported binary layout data."""


def read_regular_file(path: Path, maximum: int) -> bytes:
    """Read a bounded, non-symlink regular file without following aliases."""
    metadata = path.lstat()
    if path.is_symlink() or not stat.S_ISREG(metadata.st_mode):
        raise ParseError("not a regular file")
    if metadata.st_size <= 0 or metadata.st_size > maximum:
        raise ParseError("file size outside the accepted range")
    content = path.read_bytes()
    if len(content) != metadata.st_size:
        raise ParseError("file changed while being read")
    return content


def png_dimensions(content: bytes) -> tuple[int, int]:
    """Validate the PNG envelope and pixel stream, then return its dimensions."""
    if not content.startswith(b"\x89PNG\r\n\x1a\n"):
        raise ParseError("invalid PNG signature")
    cursor = 8
    dimensions: tuple[int, int] | None = None
    pixel_format: tuple[int, int, int] | None = None
    idat = bytearray()
    saw_idat = False
    ended_idat = False
    saw_iend = False
    saw_palette = False
    palette_entries = 0
    chunk_index = 0
    while cursor < len(content):
        if len(content) - cursor < 12:
            raise ParseError("truncated PNG chunk")
        length = struct.unpack_from(">I", content, cursor)[0]
        chunk_type = content[cursor + 4 : cursor + 8]
        end = cursor + 12 + length
        if end > len(content):
            raise ParseError("PNG chunk exceeds file")
        payload = content[cursor + 8 : cursor + 8 + length]
        expected_crc = struct.unpack_from(">I", content, cursor + 8 + length)[0]
        actual_crc = zlib.crc32(payload, zlib.crc32(chunk_type)) & 0xFFFFFFFF
        if actual_crc != expected_crc:
            raise ParseError("PNG chunk CRC mismatch")
        if chunk_index == 0 and chunk_type != b"IHDR":
            raise ParseError("IHDR is not the first PNG chunk")
        if chunk_type == b"IHDR":
            if chunk_index != 0 or dimensions is not None or length != 13:
                raise ParseError("invalid IHDR")
            width, height = struct.unpack_from(">II", payload)
            if width == 0 or height == 0:
                raise ParseError("empty PNG dimensions")
            bit_depth, color_type, compression, filtering, interlace = payload[8:]
            accepted_depths = {
                0: {1, 2, 4, 8, 16},
                2: {8, 16},
                3: {1, 2, 4, 8},
                4: {8, 16},
                6: {8, 16},
            }
            if (
                color_type not in accepted_depths
                or bit_depth not in accepted_depths[color_type]
                or compression != 0
                or filtering != 0
                or interlace not in (0, 1)
            ):
                raise ParseError("unsupported PNG pixel format")
            dimensions = (width, height)
            pixel_format = (bit_depth, color_type, interlace)
        elif chunk_type == b"PLTE":
            if saw_palette or saw_idat or length == 0 or length % 3 or length > 768:
                raise ParseError("invalid PNG palette")
            saw_palette = True
            palette_entries = length // 3
        elif chunk_type == b"IDAT":
            if ended_idat:
                raise ParseError("non-consecutive PNG image data")
            saw_idat = True
            idat.extend(payload)
        elif chunk_type == b"IEND":
            if length != 0 or end != len(content):
                raise ParseError("invalid IEND")
            saw_iend = True
            cursor = end
            break
        elif chunk_type[0] & 0x20 == 0:
            raise ParseError("unknown critical PNG chunk")
        elif saw_idat:
            ended_idat = True
        cursor = end
        chunk_index += 1
    if (
        dimensions is None
        or pixel_format is None
        or not saw_idat
        or not saw_iend
        or cursor != len(content)
    ):
        raise ParseError("incomplete PNG")
    bit_depth, color_type, interlace = pixel_format
    if color_type in (0, 4) and saw_palette:
        raise ParseError("PNG color type cannot use a palette")
    if color_type == 3 and (
        not saw_palette or palette_entries > 1 << bit_depth
    ):
        raise ParseError("indexed PNG has an invalid palette")
    channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[color_type]
    passes = [(0, 0, 1, 1)] if interlace == 0 else [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ]
    scanlines: list[tuple[int, int]] = []
    expected_bytes = 0
    for x_start, y_start, x_step, y_step in passes:
        pass_width = max(0, (dimensions[0] - x_start + x_step - 1) // x_step)
        pass_height = max(0, (dimensions[1] - y_start + y_step - 1) // y_step)
        if pass_width == 0 or pass_height == 0:
            continue
        row_bytes = (pass_width * channels * bit_depth + 7) // 8
        expected_bytes += pass_height * (1 + row_bytes)
        if expected_bytes > MAX_PNG_DECODE_BYTES:
            raise ParseError("PNG pixel stream is too large")
        scanlines.append((row_bytes, pass_height))
    try:
        decompressor = zlib.decompressobj()
        decoded = decompressor.decompress(bytes(idat), expected_bytes + 1)
        if decompressor.unconsumed_tail or len(decoded) > expected_bytes:
            raise ParseError("PNG pixel stream exceeds expected size")
    except zlib.error as error:
        raise ParseError("invalid PNG compression stream") from error
    if (
        not decompressor.eof
        or decompressor.unused_data
        or decompressor.unconsumed_tail
        or len(decoded) != expected_bytes
    ):
        raise ParseError("invalid PNG pixel stream")
    decoded_cursor = 0
    for row_bytes, pass_height in scanlines:
        for _ in range(pass_height):
            if decoded[decoded_cursor] > 4:
                raise ParseError("invalid PNG row filter")
            decoded_cursor += 1 + row_bytes
    if decoded_cursor != len(decoded):
        raise ParseError("PNG scanline size mismatch")
    return dimensions


class BlockReader:
    """Bounds-checked big-endian reader for one allocated DS_Store block."""

    def __init__(self, content: bytes) -> None:
        self.content = content
        self.cursor = 0

    def take(self, count: int) -> bytes:
        if count < 0 or self.cursor + count > len(self.content):
            raise ParseError("DS_Store block read out of bounds")
        result = self.content[self.cursor : self.cursor + count]
        self.cursor += count
        return result

    def u8(self) -> int:
        return self.take(1)[0]

    def u32(self) -> int:
        return struct.unpack(">I", self.take(4))[0]

    def u64(self) -> int:
        return struct.unpack(">Q", self.take(8))[0]


@dataclass(frozen=True)
class FinderRecord:
    name: str
    code: bytes
    type_code: bytes
    value: object


class DSStore:
    """Strict read-only parser for the buddy allocator and active DSDB B-tree."""

    def __init__(self, content: bytes) -> None:
        self.content = content
        self.offsets: list[int] = []
        self.root_node = 0
        self.levels = 0
        self.record_count = 0
        self.node_count = 0
        self.page_size = 0
        self.visited_nodes: set[int] = set()

    def allocated_block(self, block_id: int) -> bytes:
        if block_id < 0 or block_id >= len(self.offsets):
            raise ParseError("DS_Store block identifier is out of range")
        address = self.offsets[block_id]
        exponent = address & 0x1F
        offset = address & ~0x1F
        if address == 0 or exponent < 5 or exponent > 24:
            raise ParseError("invalid DS_Store block address")
        size = 1 << exponent
        if offset % size != 0:
            raise ParseError("misaligned DS_Store block")
        start = offset + 4
        end = start + size
        if start < 36 or end > len(self.content):
            raise ParseError("DS_Store block exceeds file")
        return self.content[start:end]

    def parse_allocator(self) -> int:
        if len(self.content) < 36:
            raise ParseError("truncated DS_Store header")
        magic, signature, root_offset, root_size, repeated_offset, _ = struct.unpack_from(
            ">I4sIII16s", self.content
        )
        if magic != 1 or signature != b"Bud1" or root_offset != repeated_offset:
            raise ParseError("invalid DS_Store buddy header")
        root_start = root_offset + 4
        root_end = root_start + root_size
        if root_size < 16 or root_end > len(self.content):
            raise ParseError("invalid DS_Store allocator root")
        root = BlockReader(self.content[root_start:root_end])
        block_count = root.u32()
        root.u32()  # Reserved allocator field.
        if block_count == 0 or block_count > MAX_ALLOCATOR_BLOCKS:
            raise ParseError("invalid DS_Store block count")
        padded_count = (block_count + 255) & ~255
        self.offsets = [root.u32() for _ in range(padded_count)][:block_count]
        toc_count = root.u32()
        if toc_count == 0 or toc_count > 255:
            raise ParseError("invalid DS_Store table-of-contents count")
        toc: dict[bytes, int] = {}
        for _ in range(toc_count):
            name_length = root.u8()
            if name_length == 0 or name_length > 64:
                raise ParseError("invalid DS_Store table-of-contents name")
            name = root.take(name_length)
            block_id = root.u32()
            if name in toc:
                raise ParseError("duplicate DS_Store table-of-contents name")
            toc[name] = block_id
        if b"DSDB" not in toc:
            raise ParseError("DS_Store DSDB entry is missing")
        return toc[b"DSDB"]

    @staticmethod
    def parse_record(reader: BlockReader) -> FinderRecord:
        name_length = reader.u32()
        if name_length == 0 or name_length > 1_024:
            raise ParseError("invalid DS_Store record name length")
        try:
            name = reader.take(name_length * 2).decode("utf-16be")
        except UnicodeDecodeError as error:
            raise ParseError("invalid DS_Store record name") from error
        code = reader.take(4)
        type_code = reader.take(4)
        if type_code == b"bool":
            value: object = bool(reader.u8())
        elif type_code in (b"long", b"shor"):
            value = reader.u32()
        elif type_code == b"blob":
            value = reader.take(reader.u32())
        elif type_code == b"ustr":
            length = reader.u32()
            try:
                value = reader.take(length * 2).decode("utf-16be")
            except UnicodeDecodeError as error:
                raise ParseError("invalid DS_Store Unicode value") from error
        elif type_code == b"type":
            value = reader.take(4)
        elif type_code in (b"comp", b"dutc"):
            value = reader.u64()
        else:
            raise ParseError("unsupported DS_Store record type")
        return FinderRecord(name, code, type_code, value)

    def traverse(self, block_id: int, expected_depth: int) -> list[FinderRecord]:
        if block_id in self.visited_nodes:
            raise ParseError("cyclic or repeated DS_Store B-tree node")
        self.visited_nodes.add(block_id)
        if len(self.visited_nodes) > self.node_count:
            raise ParseError("DS_Store B-tree exceeds declared node count")
        block = self.allocated_block(block_id)
        # page_size is the logical node limit. Buddy allocations may be smaller
        # for sparse Finder nodes or one bucket larger (8192 for a 4096 request),
        # so bound parsing to the logical page without equating physical size.
        block = block[: self.page_size]
        reader = BlockReader(block)
        next_node = reader.u32()
        count = reader.u32()
        if count > self.record_count or count > MAX_BTREE_RECORDS:
            raise ParseError("invalid DS_Store node record count")
        records: list[FinderRecord] = []
        if next_node:
            if expected_depth == 0:
                raise ParseError("internal DS_Store node appears at leaf depth")
            for _ in range(count):
                records.extend(self.traverse(reader.u32(), expected_depth - 1))
                records.append(self.parse_record(reader))
            records.extend(self.traverse(next_node, expected_depth - 1))
        else:
            if expected_depth != 0:
                raise ParseError("leaf DS_Store node appears above declared depth")
            for _ in range(count):
                records.append(self.parse_record(reader))
        return records

    def records(self) -> list[FinderRecord]:
        superblock_id = self.parse_allocator()
        superblock = BlockReader(self.allocated_block(superblock_id))
        self.root_node = superblock.u32()
        self.levels = superblock.u32()
        self.record_count = superblock.u32()
        self.node_count = superblock.u32()
        self.page_size = superblock.u32()
        if (
            self.record_count == 0
            or self.record_count > MAX_BTREE_RECORDS
            or self.levels > 64
            or self.node_count == 0
            or self.node_count > MAX_ALLOCATOR_BLOCKS
            or self.page_size < 512
            or self.page_size > 1 << 20
            or self.page_size & (self.page_size - 1)
        ):
            raise ParseError("invalid DS_Store B-tree metadata")
        records = self.traverse(self.root_node, self.levels)
        if len(records) != self.record_count or len(self.visited_nodes) != self.node_count:
            raise ParseError("DS_Store B-tree metadata does not match active records")
        keys = [(record.name.lower(), record.code) for record in records]
        if any(previous >= current for previous, current in zip(keys, keys[1:])):
            raise ParseError("DS_Store records are not strictly sorted")
        return records


def icon_position(records: list[FinderRecord], name: str) -> tuple[int, int]:
    matches = [record for record in records if record.name == name and record.code == b"Iloc"]
    if len(matches) != 1:
        raise ParseError("missing or ambiguous Iloc record")
    record = matches[0]
    if (
        record.type_code != b"blob"
        or not isinstance(record.value, bytes)
        or len(record.value) != 16
    ):
        raise ParseError("invalid Iloc record")
    return struct.unpack_from(">II", record.value)


def unique_blob_record(
    records: list[FinderRecord], name: str, code: bytes
) -> bytes:
    """Return one exact blob record; aliases or child-window records cannot substitute."""
    matches = [record for record in records if record.name == name and record.code == code]
    if len(matches) != 1:
        raise ParseError("missing or ambiguous Finder record")
    record = matches[0]
    if record.type_code != b"blob" or not isinstance(record.value, bytes):
        raise ParseError("Finder property record is not a blob")
    return record.value


def plist_properties(records: list[FinderRecord], name: str, code: bytes) -> dict:
    try:
        properties = plistlib.loads(unique_blob_record(records, name, code))
    except (plistlib.InvalidFileException, ValueError, TypeError) as error:
        raise ParseError("invalid Finder property list") from error
    if not isinstance(properties, dict):
        raise ParseError("Finder property list is not a dictionary")
    return properties


def window_size(records: list[FinderRecord]) -> tuple[int, int]:
    properties = plist_properties(records, ".", b"bwsp")
    bounds = properties.get("WindowBounds")
    if not isinstance(bounds, str):
        raise ParseError("invalid Finder WindowBounds")
    match = WINDOW_BOUNDS.fullmatch(bounds)
    if match is None:
        raise ParseError("unparseable Finder WindowBounds")
    return int(match.group(3)), int(match.group(4))


def alias_extra_fields(content: bytes) -> dict[int, bytes]:
    """Parse the bounded TLV section of a modern Carbon Alias v2 record."""
    if len(content) < 154:
        raise ParseError("truncated background Alias")
    _appinfo, record_size, version = struct.unpack_from(">4shh", content)
    if record_size < 150 or record_size != len(content) or version != 2:
        raise ParseError("invalid background Alias header")
    kind = struct.unpack_from(">h", content, 8)[0]
    if kind != 0:
        raise ParseError("background Alias does not target a file")
    legacy_name_length = content[50]
    if legacy_name_length == 0 or legacy_name_length > 63:
        raise ParseError("invalid background Alias target name")
    try:
        legacy_name = content[51 : 51 + legacy_name_length].decode("mac_roman")
    except UnicodeDecodeError as error:
        raise ParseError("invalid legacy Alias target name") from error
    if legacy_name != "background.png":
        raise ParseError("background Alias legacy target is wrong")

    fields: dict[int, bytes] = {}
    cursor = 150
    while cursor + 4 <= len(content):
        tag, length = struct.unpack_from(">hh", content, cursor)
        cursor += 4
        if tag == -1:
            if length != 0 or cursor != len(content):
                raise ParseError("invalid background Alias terminator")
            return fields
        if length < 0:
            raise ParseError("negative background Alias field length")
        end = cursor + length
        padded_end = end + (length & 1)
        if end > len(content) or padded_end > len(content):
            raise ParseError("background Alias field exceeds record")
        if tag in fields:
            raise ParseError("duplicate background Alias field")
        fields[tag] = content[cursor:end]
        cursor = padded_end
    raise ParseError("background Alias has no terminator")


def background_alias_path(content: bytes) -> str:
    """Prove that the Alias resolves by path to the packaged background PNG."""
    fields = alias_extra_fields(content)
    required_tags = (0, 2, 14, 18, 19)
    if any(tag not in fields for tag in required_tags):
        raise ParseError("background Alias is missing required path fields")
    if fields[0] != b".background":
        raise ParseError("background Alias folder is wrong")
    carbon_suffix = b":.background:\0background.png"
    if not fields[2].endswith(carbon_suffix) or len(fields[2]) == len(carbon_suffix):
        raise ParseError("background Alias Carbon path is wrong")

    unicode_name = fields[14]
    if len(unicode_name) < 2:
        raise ParseError("background Alias Unicode target is truncated")
    character_count = struct.unpack_from(">H", unicode_name)[0]
    if len(unicode_name) != 2 + character_count * 2:
        raise ParseError("background Alias Unicode target length is wrong")
    try:
        target_name = unicode_name[2:].decode("utf-16be")
        posix_path = fields[18].decode("utf-8")
        mount_path = fields[19].decode("utf-8")
    except UnicodeDecodeError as error:
        raise ParseError("background Alias path encoding is invalid") from error
    if target_name != "background.png" or posix_path != "/.background/background.png":
        raise ParseError("background Alias POSIX target is wrong")
    parsed_mount = PurePosixPath(mount_path)
    if (
        not parsed_mount.is_absolute()
        or "\0" in mount_path
        or ".." in parsed_mount.parts
        or str(parsed_mount) != mount_path
    ):
        raise ParseError("background Alias mount path is invalid")
    return posix_path


def background_binding(records: list[FinderRecord]) -> str:
    properties = plist_properties(records, ".", b"icvp")
    if type(properties.get("backgroundType")) is not int or properties["backgroundType"] != 2:
        raise ParseError("Finder background type is not image")
    alias = properties.get("backgroundImageAlias")
    if not isinstance(alias, bytes) or not alias:
        raise ParseError("Finder background Alias is missing")
    return background_alias_path(alias)


def inspect(arguments: argparse.Namespace) -> list[str]:
    try:
        dimensions = png_dimensions(read_regular_file(arguments.background, MAX_PNG_BYTES))
    except (OSError, ParseError) as error:
        raise InspectionError("background-image-unparseable") from error
    expected_size = (arguments.width, arguments.height)
    if dimensions != expected_size:
        raise InspectionError("background-dimensions-invalid")
    try:
        records = DSStore(read_regular_file(arguments.ds_store, MAX_DS_STORE_BYTES)).records()
        finder_size = window_size(records)
        app_position = icon_position(records, arguments.app_name)
        applications_position = icon_position(records, "Applications")
    except (OSError, ParseError, OverflowError, struct.error) as error:
        raise InspectionError("finder-layout-unparseable") from error
    if finder_size != expected_size:
        raise InspectionError("finder-window-size-invalid")
    try:
        bound_background = background_binding(records)
    except (ParseError, OverflowError, struct.error) as error:
        raise InspectionError("finder-background-binding-invalid") from error
    if app_position != (arguments.app_x, arguments.app_y):
        raise InspectionError("app-icon-position-invalid")
    if applications_position != (arguments.applications_x, arguments.applications_y):
        raise InspectionError("applications-icon-position-invalid")
    return [
        f"background_dimensions={dimensions[0]}x{dimensions[1]}",
        f"finder_window_size={finder_size[0]}x{finder_size[1]}",
        f"app_position={app_position[0]},{app_position[1]}",
        f"applications_position={applications_position[0]},{applications_position[1]}",
        f"background_binding={bound_background}",
    ]


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser()
    result.add_argument("--background", type=Path, required=True)
    result.add_argument("--ds-store", type=Path, required=True)
    result.add_argument("--app-name", required=True)
    result.add_argument("--width", type=int, required=True)
    result.add_argument("--height", type=int, required=True)
    result.add_argument("--app-x", type=int, required=True)
    result.add_argument("--app-y", type=int, required=True)
    result.add_argument("--applications-x", type=int, required=True)
    result.add_argument("--applications-y", type=int, required=True)
    return result


def main() -> int:
    try:
        output = inspect(parser().parse_args())
    except InspectionError as error:
        print(f"reason={error.reason}")
        return 1
    except Exception:
        print("reason=finder-layout-unparseable")
        return 1
    print("\n".join(output))
    return 0


if __name__ == "__main__":
    sys.exit(main())
