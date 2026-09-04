#!/usr/bin/env python3
"""为下游 Harness 工程升级生成三方计划并安全更新既有受管文件。"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

from harness_upgrade_core import build_plan, write_new_plan
from harness_upgrade_mutation import apply_plan
from harness_upgrade_record import record_lock
from harness_upgrade_safety import UpgradeError


def add_shared_arguments(parser: argparse.ArgumentParser) -> None:
    """为 plan 子命令添加候选、目标、清单和来源锁参数。"""

    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--source-version", required=True)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--candidate-root", type=Path, required=True)
    parser.add_argument("--target-root", type=Path, required=True)
    parser.add_argument("--ownership", type=Path, required=True)
    parser.add_argument("--lock", type=Path, required=True)


def build_parser() -> argparse.ArgumentParser:
    """构造稳定的非交互命令行接口。"""

    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    plan = commands.add_parser("plan", help="生成只读三方更新 plan")
    add_shared_arguments(plan)
    plan.add_argument("--output", type=Path)

    apply_command = commands.add_parser(
        "apply",
        help="把已批准更新应用到既有且未变化的 managed 文件",
    )
    apply_command.add_argument("--plan", type=Path, required=True)
    apply_command.add_argument("--approval", required=True)
    apply_command.add_argument("--path", required=True)

    record = commands.add_parser("record", help="记录已复核的 Harness 基线")
    record.add_argument("--plan", type=Path, required=True)
    record.add_argument("--source-version", required=True)
    record.add_argument("--source-commit", required=True)
    record.add_argument("--approval", required=True)
    record.add_argument("--bootstrap", action="store_true")
    record.add_argument("--resolved-manual", action="append", default=[])
    return parser


def main() -> int:
    """解析命令、输出结构化结果，并以非零状态暴露安全阻断。"""

    parser = build_parser()
    args = parser.parse_args()
    try:
        if args.command == "plan":
            result = build_plan(
                args.source_root,
                args.source_version,
                args.source_commit,
                args.candidate_root,
                args.target_root,
                args.ownership,
                args.lock,
            )
            if args.output:
                write_new_plan(
                    args.output,
                    result,
                    (
                        Path(result["candidate_root"]),
                        Path(result["target_root"]),
                    ),
                )
            print(json.dumps(result, ensure_ascii=False, sort_keys=True))
            return 2 if result["blocked"] else 0
        if args.command == "apply":
            result = apply_plan(args.plan, args.approval, args.path)
        else:
            result = record_lock(args)
        print(json.dumps(result, ensure_ascii=False, sort_keys=True))
        return 0
    except (OSError, UpgradeError, TypeError, ValueError) as exc:
        print(
            json.dumps({"ok": False, "error": str(exc)}, ensure_ascii=False),
            file=sys.stderr,
        )
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
