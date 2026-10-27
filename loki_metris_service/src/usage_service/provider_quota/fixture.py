#!/usr/bin/env python3
"""测试夹具：记录参数和标准输入，按 mode.txt 返回固定输出。不是产品代码。"""
import pathlib
import sys
import time

root = pathlib.Path(__file__).resolve().parent
argv = sys.argv[1:]
mode = (root / "mode.txt").read_text(encoding="utf-8").strip()
if mode == "ignore-stdin":
    time.sleep(30)
stdin = sys.stdin.read()
with (root / "argv.txt").open("a", encoding="utf-8") as handle:
    handle.write("\n".join(argv))
    handle.write("\n")
(root / "stdin.txt").write_text(stdin, encoding="utf-8")
with (root / "requests.txt").open("a", encoding="utf-8") as handle:
    handle.write(stdin + "\n")
if mode == "keychain":
    sys.stdout.write((root / "keychain-out.txt").read_text(encoding="utf-8"))
    raise SystemExit(0)
if mode == "keychain-denied":
    raise SystemExit(1)
if "-readonly" in argv:
    text = (root / "sqlite-out.txt").read_text(encoding="utf-8")
    # LIMIT 1 只回第一行，用来观察查询有没有把另一行丢掉。
    if any("LIMIT 1" in arg for arg in argv):
        text = text.split("\n", 1)[0]
    sys.stdout.write(text)
    raise SystemExit(0)
if "aiserver.v1.DashboardService/" in stdin:
    name = "plan-info.txt" if "/GetPlanInfo" in stdin else "period-usage.txt"
    response = root / name
    sys.stdout.write(response.read_text(encoding="utf-8") if response.exists() else "{}\n__LM_HTTP_404")
    raise SystemExit(0)
if mode == "sleep":
    time.sleep(30)
elif mode == "huge":
    sys.stdout.write("y" * 400000)
elif mode in ("status", "nonzero"):
    sys.stdout.write((root / "body.txt").read_text(encoding="utf-8"))
    if mode == "nonzero":
        raise SystemExit(1)
else:
    sys.stdout.write("{}\n__LM_HTTP_200")
