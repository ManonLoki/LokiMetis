"""资源采样回归只覆盖单位、端点身份和集合合同；不伪造宿主运行结果。"""

import ctypes
import contextlib
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from macos_resource_sample import ProcessReader, UsageV4, cpu_delta, interval, main, parse_process


class ResourceSampleTests(unittest.TestCase):
    """固定反例阻止重新把 mach ticks 当纳秒，或把错误端点读成零。"""

    def setUp(self):
        """构造整数 CPU 计数端点，模拟多核累计时间而非真实应用数据。"""
        self.before = {"identity": [1, "uuid", "/app"], "cpu_mach_ticks": 10**18,
                       "stamp_ns": 10**18, "footprint_bytes": 10, "resident_bytes": 20}
        self.after = dict(self.before, cpu_mach_ticks=10**18 + 48_000_000,
                          stamp_ns=10**18 + 1_000_000_000)

    def test_timebase_and_multicore_percent(self):
        """125/3 的 4800 万 ticks 等于两 CPU 秒，不能写成 0.048 秒或截为100%。"""
        result = cpu_delta(self.before, self.after, 125, 3)
        self.assertEqual(result["cpu_seconds"], 2)
        self.assertEqual(result["cpu_percent"], 200)
        self.assertNotEqual(result["cpu_seconds"], 0.048)

    def test_identity_counters_and_time_must_be_valid(self):
        """PID复用、计数倒退、时间不前进及无效比例都必须拒绝。"""
        for after in (dict(self.after, identity=[2, "uuid", "/app"]),
                      dict(self.after, cpu_mach_ticks=1),
                      dict(self.after, stamp_ns=self.before["stamp_ns"])):
            with self.assertRaises(ValueError):
                cpu_delta(self.before, after, 125, 3)
        with self.assertRaises(ValueError):
            cpu_delta(self.before, self.after, 125, 0)

    def test_membership_changes_invalidate_interval(self):
        """helper退出使端点集合变化时，不允许求剩余进程之和冒充完整集合。"""
        before = {"groups": {"app": [7, 8]}, "processes": {"7": self.before}}
        after = {"groups": {"app": [7]}, "processes": {"7": self.after}}
        with self.assertRaises(ValueError):
            interval(before, after, {"numer": 125, "denom": 3}, {"app": 7})

    def test_v4_layout_matches_sdk(self):
        """核对系统ABI尺寸与关键内存字段偏移，防止读取错误字段而仍输出数值。"""
        self.assertEqual(ctypes.sizeof(UsageV4), 296)
        self.assertEqual(UsageV4.resident.offset, 64)
        self.assertEqual(UsageV4.footprint.offset, 72)
        self.assertEqual(UsageV4.lifetime_peak_footprint.offset, 240)

    def test_pid_cannot_wrap_to_another_live_process(self):
        """超过有符号32位的PID不交给ctypes，边界内保留原始身份标签。"""
        self.assertEqual(parse_process("app=2147483647"), ("app", 2147483647))
        for item in ("app=2147483648", "app=4295030293", "app=0", "app=-1"):
            with self.assertRaises(ValueError):
                parse_process(item)

    def test_libproc_failure_is_not_a_zero_reading(self):
        """系统读取返回错误时直接抛出，不构造零CPU或零内存端点。"""
        reader = ProcessReader.__new__(ProcessReader)
        reader.proc = SimpleNamespace(proc_pid_rusage=Mock(return_value=-1))
        with patch("macos_resource_sample.C.get_errno", return_value=3):
            with self.assertRaises(OSError):
                reader.usage(7)

    def test_sampling_failure_marks_report_invalid(self):
        """实际命令入口收到驱动错误时，输出明确失败并返回非零退出码。"""
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "failure.json"
            argv = ["sampler", "--pid", "app=7", "--output", str(output)]
            with (patch("sys.argv", argv),
                  patch("macos_resource_sample.ProcessReader", side_effect=OSError("read failed")),
                  contextlib.redirect_stdout(io.StringIO())):
                self.assertEqual(main(), 1)
            report = json.loads(output.read_text())
            self.assertFalse(report["valid"])
            self.assertEqual(report["endpoints"], [])
            self.assertIn("OSError: read failed", report["errors"])


if __name__ == "__main__":
    unittest.main()
