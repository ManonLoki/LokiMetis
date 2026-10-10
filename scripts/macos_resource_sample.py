"""只读 macOS 进程资源采样；标准库实现，不控制窗口或修改应用状态。

CPU 原值为 mach ticks，须通过本机 timebase 换成纳秒。每个采样点保存
身份、当前 resident/footprint 和生命周期峰值；集合内存相加不去重共享页。
"""

import argparse
import ctypes as C
import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time


class UsageV4(C.Structure):
    """对应 SDK rusage_info_v4，字段顺序保持 ABI，尾部含生命周期峰值。"""

    _fields_ = [("uuid", C.c_uint8 * 16)] + [
        (name, C.c_uint64)
        for name in (
            "user", "system", "package_idle_wakeups", "interrupt_wakeups",
            "pageins", "wired", "resident", "footprint", "start", "exit",
            "child_user", "child_system", "child_package_idle_wakeups",
            "child_interrupt_wakeups", "child_pageins", "child_elapsed",
            "disk_read", "disk_write", "qos_default", "qos_maintenance",
            "qos_background", "qos_utility", "qos_legacy", "qos_initiated",
            "qos_interactive", "billed_system", "serviced_system", "logical_writes",
            "lifetime_peak_footprint", "instructions", "cycles", "billed_energy",
            "serviced_energy", "interval_peak_footprint", "runnable_time",
        )
    ]


class Timebase(C.Structure):
    """mach ticks 到纳秒的有理数比例；不能把某台机器的比例写成通用常量。"""

    _fields_ = [("numer", C.c_uint32), ("denom", C.c_uint32)]


def cpu_delta(before, after, numer, denom):
    """仅接受同一活进程的递增端点，返回单核心百分比及真实 CPU 秒。"""
    if before["identity"] != after["identity"]:
        raise ValueError("进程身份变化")
    ticks = after["cpu_mach_ticks"] - before["cpu_mach_ticks"]
    elapsed_ns = after["stamp_ns"] - before["stamp_ns"]
    if ticks < 0 or elapsed_ns <= 0 or numer <= 0 or denom <= 0:
        raise ValueError("CPU 计数、时间或 timebase 无效")
    # 先保留整数乘法，避免大累计值转 float 后再相减损失精度。
    cpu_ns = ticks * numer / denom
    return {
        "cpu_mach_ticks": ticks,
        "cpu_seconds": cpu_ns / 1_000_000_000,
        "elapsed_seconds": elapsed_ns / 1_000_000_000,
        "cpu_percent": 100 * cpu_ns / elapsed_ns,
    }


def parse_process(item):
    """约束 PID 为 libproc 有符号32位正数，防止 ctypes 截断后读取另一进程。"""
    if not re.fullmatch(r"[a-zA-Z][a-zA-Z0-9_-]*=[1-9][0-9]*", item):
        raise ValueError("--pid 需要 LABEL=正整数PID")
    label, raw_pid = item.split("=")
    pid = int(raw_pid)
    if pid > 2**31 - 1:
        raise ValueError("PID 超出 libproc 有符号32位范围")
    return label, pid


class ProcessReader:
    """封装只读 libproc/mach ABI；同一次读取前后复核进程身份和退出状态。"""

    def __init__(self):
        """仅实际采样时加载 macOS 系统库，帮助与纯数学回归无需这些库。"""
        if sys.platform != "darwin":
            raise RuntimeError("本工具仅在 macOS 采样")
        if C.sizeof(UsageV4) != 296 or UsageV4.lifetime_peak_footprint.offset != 240:
            raise RuntimeError("rusage_info_v4 ABI 不符")
        self.proc = C.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        self.proc.proc_pid_rusage.argtypes = [C.c_int, C.c_int, C.c_void_p]
        self.proc.proc_pid_rusage.restype = C.c_int
        self.proc.proc_pidpath.argtypes = [C.c_int, C.c_void_p, C.c_uint32]
        self.proc.proc_pidpath.restype = C.c_int
        system = C.CDLL("/usr/lib/libSystem.B.dylib")
        system.mach_timebase_info.argtypes = [C.POINTER(Timebase)]
        system.mach_timebase_info.restype = C.c_int
        value = Timebase()
        if system.mach_timebase_info(C.byref(value)) != 0 or not value.numer or not value.denom:
            raise RuntimeError("mach_timebase_info 读取失败")
        self.timebase = {"numer": value.numer, "denom": value.denom}

    def usage(self, pid):
        """读取 V4 原始资源，失败保留错误，不把无权限或退出当零。"""
        value = UsageV4()
        if self.proc.proc_pid_rusage(pid, 4, C.byref(value)) != 0:
            raise OSError(C.get_errno(), f"proc_pid_rusage({pid})")
        return value

    def read(self, pid):
        """采样时钟包围资源读取，以中心时点计算各 PID 的实际间隔。"""
        begin = time.monotonic_ns()
        first = self.usage(pid)
        end = time.monotonic_ns()
        path = C.create_string_buffer(4096)
        if self.proc.proc_pidpath(pid, path, len(path)) <= 0:
            raise OSError(C.get_errno(), f"proc_pidpath({pid})")
        executable = os.fsdecode(path.value)
        second = self.usage(pid)
        if (not executable or not first.start or first.exit or second.exit
                or (first.start, bytes(first.uuid)) != (second.start, bytes(second.uuid))
                or second.user + second.system < first.user + first.system):
            raise RuntimeError(f"进程身份或生命周期变化 {pid}")
        return {
            "pid": pid, "identity": [first.start, bytes(first.uuid).hex(), executable],
            "cpu_mach_ticks": first.user + first.system, "stamp_ns": (begin + end) // 2,
            "read_span_ns": end - begin, "resident_bytes": first.resident,
            "footprint_bytes": first.footprint,
            "lifetime_peak_footprint_bytes": first.lifetime_peak_footprint,
            "package_idle_wakeups": first.package_idle_wakeups,
            "interrupt_wakeups": first.interrupt_wakeups,
        }


def command(arguments):
    """有界执行系统只读命令，非零退出或超时中止本轮。"""
    return subprocess.check_output(arguments, text=True, timeout=5)


def coalition(pid):
    """从 LaunchServices 获取应用实际集合，避免 PPID 遗漏 WebKit helper。"""
    raw = command(["/usr/bin/lsappinfo", "-all", "info", "-only", "coalitionPIDs", f"#{pid}"])
    match = re.search(r'"LSApplicationCoalitionPIDsKey"\s*=\s*\(([^)]*)\)', raw)
    if not match:
        raise RuntimeError(f"coalition 读取失败 {pid}")
    members = sorted(set(map(int, re.findall(r"\d+", match.group(1)))))
    if pid not in members:
        raise RuntimeError(f"coalition 不含主进程 {pid}")
    return members


def compilers():
    """仅记录采样点的编译进程，不能证明整个区间没有短命编译。"""
    raw = command(["/bin/ps", "-axo", "pid=,comm="])
    return [line.strip() for line in raw.splitlines() if line.split()
            and Path(line.strip().split(maxsplit=1)[-1]).name
            in {"cargo", "rustc", "clang", "clang++", "cc", "ld"}]


def foreground(reader):
    """只读前台身份，端点相同不能代替完整遮挡和交互时间线。"""
    asn = command(["/usr/bin/lsappinfo", "front"]).strip()
    raw = command(["/usr/bin/lsappinfo", "-all", "info", "-only", "pid", asn])
    match = re.search(r'"pid"\s*=\s*(\d+)', raw)
    if not match:
        raise RuntimeError("前台进程读取失败")
    return reader.read(int(match.group(1)))["identity"]


def endpoint(reader, roots, include_coalition):
    """每个端点复读集合、身份、内存及后台条件，拒绝两个应用集合重叠。"""
    groups = {name: coalition(pid) if include_coalition else [pid]
              for name, pid in roots.items()}
    ids = [pid for members in groups.values() for pid in members]
    if len(ids) != len(set(ids)):
        raise RuntimeError("应用进程集合重叠")
    values = {str(pid): reader.read(pid) for pid in sorted(ids)}
    return {"utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
            "groups": groups, "processes": values, "compilers": compilers(),
            "foreground_identity": foreground(reader)}


def interval(before, after, timebase, roots):
    """成员变化使整轮无效；分别报告主进程与完整集合，内存账本不做去重。"""
    if before["groups"] != after["groups"]:
        raise ValueError("采样端点 coalition 成员变化")
    deltas = {pid: cpu_delta(value, after["processes"][pid], **timebase)
              for pid, value in before["processes"].items()}
    groups = {}
    for name, members in before["groups"].items():
        groups[name] = {}
        for role, pids in {"root": [roots[name]], "full_collection": members}.items():
            groups[name][role] = {
                "members": pids, "cpu_percent": sum(deltas[str(pid)]["cpu_percent"] for pid in pids),
                "footprint_ledger_sum_bytes": sum(after["processes"][str(pid)]["footprint_bytes"] for pid in pids),
                "resident_sum_bytes": sum(after["processes"][str(pid)]["resident_bytes"] for pid in pids),
            }
    return {"processes": deltas, "groups": groups}


def main():
    """运行有限次数采样并独占创建本地 JSON；失败保留原始端点和明确原因。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", action="append", required=True, metavar="LABEL=PID")
    parser.add_argument("--coalition", action="store_true")
    parser.add_argument("--interval", type=float, default=20)
    parser.add_argument("--samples", type=int, default=1, help="差分区间数，端点数多一个")
    parser.add_argument("--warmup", type=float, default=5)
    parser.add_argument("--require-stable-foreground", action="store_true")
    parser.add_argument("--reject-compilers", action="store_true")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    roots = {}
    for item in args.pid:
        try:
            label, pid = parse_process(item)
        except ValueError as error:
            parser.error(str(error))
        if label in roots:
            parser.error("LABEL 不得重复")
        roots[label] = pid
    if not 1 <= args.interval <= 3600 or not 1 <= args.samples <= 1000 or not 0 <= args.warmup <= 600:
        parser.error("采样时长或次数超出有限范围")
    report = {"schema_version": 1, "valid": False, "roots": roots,
              "method": "RUSAGE_INFO_V4 mach-timebase / monotonic endpoint delta",
              "options": {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()},
              "endpoints": [], "intervals": [], "errors": [],
              "limits": ["端点不能捕捉区间短命 helper、遮挡变化或编译",
                         "集合内存相加不去重共享页，不包含 WindowServer 间接开销",
                         "峰值是进程生命周期累计值；不相加为同时发生的集合峰值"]}
    # 先独占输出路径，避免误覆盖既有证据；中断时仍留下不完整的原文件。
    with args.output.open("x", encoding="utf-8") as output:
        try:
            reader = ProcessReader()
            report["timebase"] = reader.timebase
            time.sleep(args.warmup)
            for index in range(args.samples + 1):
                if index:
                    time.sleep(args.interval)
                point = endpoint(reader, roots, args.coalition)
                report["endpoints"].append(point)
                if args.reject_compilers and point["compilers"]:
                    raise ValueError("采样点存在编译进程")
                if index:
                    previous = report["endpoints"][-2]
                    if args.require_stable_foreground and previous["foreground_identity"] != point["foreground_identity"]:
                        raise ValueError("采样端点前台变化")
                    report["intervals"].append(interval(previous, point, reader.timebase, roots))
                print(json.dumps({"endpoint": index, "groups": point["groups"]}), flush=True)
            report["valid"] = True
        except (Exception, KeyboardInterrupt) as error:
            report["errors"].append(f"{type(error).__name__}: {error}")
        json.dump(report, output, ensure_ascii=False, indent=2)
        output.write("\n")
    print(json.dumps({"valid": report["valid"], "errors": report["errors"]}, ensure_ascii=False))
    return 0 if report["valid"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
