#!/usr/bin/env python3
"""聚合 Harness 升级计划与写入回归。"""

from __future__ import annotations

import unittest

import harness_upgrade_mutation_tests
import harness_upgrade_plan_tests


def load_tests(
    loader: unittest.TestLoader,
    tests: unittest.TestSuite,
    pattern: str | None,
) -> unittest.TestSuite:
    """从非 discovery 命名模块加载每组测试一次。"""

    del tests, pattern
    suite = unittest.TestSuite()
    suite.addTests(loader.loadTestsFromModule(harness_upgrade_plan_tests))
    suite.addTests(loader.loadTestsFromModule(harness_upgrade_mutation_tests))
    return suite


if __name__ == "__main__":
    unittest.main()
