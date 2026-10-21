#!/usr/bin/env python3
"""测试夹具：记录参数和标准输入，按 mode.txt 返回固定输出。不是产品代码。"""
import pathlib
import sys
import time

root = pathlib.Path(__file__).resolve().parent
argv = sys.argv[1:]
stdin = sys.stdin.read()
with (root / "argv.txt").open("a", encoding="utf-8") as handle:
    handle.write("\n".join(argv))
    handle.write("\n")
(root / "stdin.txt").write_text(stdin, encoding="utf-8")
mode = (root / "mode.txt").read_text(encoding="utf-8").strip()
if "-readonly" in argv:
    sys.stdout.write((root / "sqlite-out.txt").read_text(encoding="utf-8"))
    raise SystemExit(0)
if mode == "sleep":
    time.sleep(30)
elif mode == "huge":
    sys.stdout.write("y" * 400000)
elif mode == "status":
    sys.stdout.write((root / "body.txt").read_text(encoding="utf-8"))
else:
    sys.stdout.write("{}\n__LM_HTTP_200")
