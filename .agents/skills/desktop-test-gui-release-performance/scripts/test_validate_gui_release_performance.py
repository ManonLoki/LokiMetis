#!/usr/bin/env python3
"""回归测试 GUI Release 性能证据的候选绑定与固定阈值。"""

from __future__ import annotations

from copy import deepcopy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

import validate_gui_release_performance as performance


class GuiReleasePerformanceTests(unittest.TestCase):
    """覆盖通过边界、失败关闭、原生平台和证据保存语义。"""

    def setUp(self) -> None:
        """为每个场景建立隔离候选、manifest 与有效观测。"""

        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.probe = self.root / "loki_metis_gui"
        self.probe.write_bytes(b"release-no-bundle-runtime-probe")
        digest = hashlib.sha256(self.probe.read_bytes()).hexdigest()
        self.manifest = {
            "interface": "gui",
            "performanceProbe": self.probe.name,
            "performanceProbeSha256": digest,
            "performanceProbeKind": "tauri-no-bundle-executable",
            "performanceProbeBuildProfile": "release",
            "sourceCommit": "a" * 40,
            "sourceTreeState": "clean",
            "buildMode": "native",
            "platform": "macos",
            "architecture": "aarch64",
            "performanceSelection": "enabled",
            "e2eSelection": "disabled",
        }
        self.evidence = {
            "schemaVersion": 2,
            "performanceProbeSha256": digest,
            "performanceProbeKind": "tauri-no-bundle-executable",
            "sourceCommit": "a" * 40,
            "sourceTreeState": "clean",
            "platform": "macos",
            "architecture": "aarch64",
            "buildMode": "native",
            "buildProfile": "release",
            "performanceSelection": "enabled",
            "e2eSelection": "disabled",
            "trayEnabled": True,
            "observationAvailable": True,
            "wholeProcessTree": True,
            "probeBytesUnmodified": True,
            "allProcessesRecovered": True,
            "rendererTimingSource": "performance-observer",
            "processSampler": "native-process-tree-sampler",
            "warmupRuns": 1,
            "windowStateIsolation": {
                "targetResolved": True,
                "snapshotStoredOutsideAppData": True,
                "originalSnapshotVerified": True,
                "fingerprintAlgorithm": "hmac-sha256-ephemeral-key",
                "original": {"kind": "present", "fingerprint": "b" * 64},
                "seed": {"kind": "absent"},
                "preLaunchResets": [
                    {
                        "phase": "warmup",
                        "run": 1,
                        "observed": {"kind": "absent"},
                    },
                    *[
                        {
                            "phase": "cold-start",
                            "run": run,
                            "observed": {"kind": "absent"},
                        }
                        for run in range(1, 6)
                    ],
                ],
                "restoration": {
                    "observed": {"kind": "present", "fingerprint": "b" * 64},
                    "verified": True,
                },
            },
            "coldStartVisibleUsableMs": [1200, 2400, 2400, 2400, 3600],
            "interactions": [
                {
                    "name": f"approved-interaction-{index}",
                    "durationMs": 239 if index == 19 else 120,
                    "observableResult": True,
                }
                for index in range(20)
            ],
            "longTasksMs": [50, 239],
            "idleObservationSeconds": 30,
            "idleCpuPercentOfOneLogicalCore": [6, 6, 6, 6, 6],
            "hiddenTrayObservationSeconds": 30,
            "hiddenTrayCpuPercentOfOneLogicalCore": [2.4, 2.4, 2.4, 2.4, 2.4],
            "steadyRssMiB": 360,
            "peakRssMiB": 600,
            "rssBeforeCyclesMiB": 200,
            "rssAfterCyclesMiB": 238.4,
            "navigationInteractionCycles": 20,
        }

    def _evaluate(
        self,
        *,
        manifest: dict[str, object] | None = None,
        evidence: dict[str, object] | None = None,
        tray_enabled: bool = True,
    ) -> dict[str, object]:
        """以深拷贝输入执行判定，避免测试场景相互污染。"""

        return performance.evaluate(
            self.probe,
            deepcopy(manifest if manifest is not None else self.manifest),
            deepcopy(evidence if evidence is not None else self.evidence),
            tray_enabled,
        )

    def _write_json(self, name: str, value: dict[str, object]) -> Path:
        """把命令行测试输入写入当前隔离目录。"""

        path = self.root / name
        path.write_text(
            json.dumps(value, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        return path

    def _run_cli(
        self,
        evidence: dict[str, object],
        evidence_name: str,
        output_name: str,
    ) -> tuple[int, dict[str, object]]:
        """写入 manifest/evidence 后调用命令行入口，返回退出码与保存的 JSON。"""

        manifest_path = self._write_json("probe.manifest.json", self.manifest)
        evidence_path = self._write_json(evidence_name, evidence)
        output_path = self.root / output_name
        exit_code = performance.main(
            [
                "--probe",
                str(self.probe),
                "--manifest",
                str(manifest_path),
                "--evidence",
                str(evidence_path),
                "--tray-enabled",
                "enabled",
                "--output",
                str(output_path),
            ]
        )
        return exit_code, json.loads(output_path.read_text(encoding="utf-8"))

    def test_threshold_boundaries_pass_with_e2e_disabled(self) -> None:
        """性能已启用时，E2E 关闭仍应真实测量并允许全部固定边界值通过。"""

        result = self._evaluate()

        self.assertEqual(result["status"], "passed")
        self.assertEqual(result["schemaVersion"], 2)
        self.assertEqual(result["performanceSelection"], "enabled")
        self.assertEqual(result["thresholdProfile"], "gui-release-v2")
        self.assertTrue(result["windowStateRecoveryVerified"])
        self.assertFalse(result["waiverAllowed"])
        metrics = result["metrics"]
        self.assertEqual(metrics["coldStartMedianMs"], 2400)
        self.assertEqual(metrics["coldStartMaximumMs"], 3600)
        self.assertEqual(metrics["interactionP95Ms"], 120)
        self.assertEqual(metrics["interactionMaximumMs"], 239)
        self.assertAlmostEqual(metrics["rssGrowthLimitMiB"], 38.4)

    def test_disabled_performance_selection_is_rejected(self) -> None:
        """性能选择关闭时不得调用 helper 或生成看似有效的性能证据。"""

        manifest = deepcopy(self.manifest)
        manifest["performanceSelection"] = "disabled"
        evidence = deepcopy(self.evidence)
        evidence["performanceSelection"] = "disabled"

        result = self._evaluate(manifest=manifest, evidence=evidence)

        self.assertEqual(result["status"], "failed")
        failures = "\n".join(result["failures"])
        self.assertIn("manifest.performanceSelection must be 'enabled'", failures)
        self.assertIn("evidence.performanceSelection must equal 'enabled'", failures)

    def test_debug_or_cross_compiled_probe_cannot_pass(self) -> None:
        """Debug 观测和 macOS xwin 交叉候选都不能形成原生 Release 结论。"""

        debug_evidence = deepcopy(self.evidence)
        debug_evidence["buildProfile"] = "debug"
        debug_result = self._evaluate(evidence=debug_evidence)
        self.assertEqual(debug_result["status"], "failed")
        self.assertTrue(
            any("buildProfile" in failure for failure in debug_result["failures"])
        )

        cross_manifest = deepcopy(self.manifest)
        cross_manifest["buildMode"] = "cross-compiled-xwin"
        cross_evidence = deepcopy(self.evidence)
        cross_evidence["buildMode"] = "cross-compiled-xwin"
        cross_result = self._evaluate(
            manifest=cross_manifest, evidence=cross_evidence
        )
        self.assertEqual(cross_result["status"], "failed")
        self.assertTrue(
            any("native buildMode" in failure for failure in cross_result["failures"])
        )

    def test_manifest_must_name_clean_head_no_bundle_probe(self) -> None:
        """安装容器摘要或 dirty 源状态不能冒充 no-bundle 性能探针。"""

        container_manifest = deepcopy(self.manifest)
        container_manifest.pop("performanceProbe")
        container_manifest.pop("performanceProbeSha256")
        container_manifest["installer"] = "example-v1.2.3-macos-aarch64.dmg"
        container_manifest["sha256"] = "b" * 64
        container_result = self._evaluate(manifest=container_manifest)
        self.assertEqual(container_result["status"], "failed")
        self.assertTrue(
            any(
                "performanceProbe" in failure
                for failure in container_result["failures"]
            )
        )

        dirty_manifest = deepcopy(self.manifest)
        dirty_manifest["sourceTreeState"] = "dirty"
        dirty_result = self._evaluate(manifest=dirty_manifest)
        self.assertEqual(dirty_result["status"], "failed")
        self.assertTrue(
            any("sourceTreeState" in failure for failure in dirty_result["failures"])
        )

        missing_target_manifest = deepcopy(self.manifest)
        missing_target_manifest.pop("platform")
        missing_target_manifest["architecture"] = ""
        missing_target_evidence = deepcopy(self.evidence)
        missing_target_evidence.pop("platform")
        missing_target_evidence["architecture"] = ""
        missing_target_result = self._evaluate(
            manifest=missing_target_manifest,
            evidence=missing_target_evidence,
        )
        self.assertEqual(missing_target_result["status"], "failed")
        self.assertTrue(
            any("manifest.platform" in failure for failure in missing_target_result["failures"])
        )
        self.assertTrue(
            any(
                "manifest.architecture" in failure
                for failure in missing_target_result["failures"]
            )
        )

    def test_rebuilding_probe_or_stale_source_binding_invalidates_evidence(self) -> None:
        """探针字节或源码提交变化后必须拒绝旧性能证据。"""

        self.probe.write_bytes(b"rebuilt-after-measurement")
        result = self._evaluate()
        self.assertEqual(result["status"], "failed")
        self.assertTrue(
            any("performanceProbeSha256" in failure for failure in result["failures"])
        )

        current_digest = hashlib.sha256(self.probe.read_bytes()).hexdigest()
        manifest = deepcopy(self.manifest)
        manifest["performanceProbeSha256"] = current_digest
        manifest["sourceCommit"] = "b" * 40
        evidence = deepcopy(self.evidence)
        evidence["performanceProbeSha256"] = current_digest
        stale_result = self._evaluate(manifest=manifest, evidence=evidence)
        self.assertEqual(stale_result["status"], "failed")
        self.assertTrue(
            any("sourceCommit" in failure for failure in stale_result["failures"])
        )

    def test_requires_five_starts_twenty_interactions_and_observation(self) -> None:
        """样本不足或无法观察必须失败关闭，不能降级为未发现问题。"""

        evidence = deepcopy(self.evidence)
        evidence["coldStartVisibleUsableMs"] = [1000] * 4
        evidence["interactions"] = evidence["interactions"][:19]
        evidence["idleObservationSeconds"] = 29.9
        evidence["idleCpuPercentOfOneLogicalCore"] = [1] * 4
        evidence["observationAvailable"] = False
        result = self._evaluate(evidence=evidence)

        self.assertEqual(result["status"], "failed")
        failures = "\n".join(result["failures"])
        self.assertIn("exactly 5", failures)
        self.assertIn("at least 20", failures)
        self.assertIn("at least 5", failures)
        self.assertIn("idleObservationSeconds must be >= 30", failures)
        self.assertIn("observationAvailable", failures)

    def test_latency_and_long_task_fail_at_exclusive_limits(self) -> None:
        """单次交互或 Long Task 达到 240 ms 时必须失败。"""

        evidence = deepcopy(self.evidence)
        evidence["interactions"][19]["durationMs"] = 240
        evidence["longTasksMs"] = [240]
        result = self._evaluate(evidence=evidence)

        self.assertEqual(result["status"], "failed")
        failures = "\n".join(result["failures"])
        self.assertIn("interaction is 240 ms or slower", failures)
        self.assertIn("Long Task is 240 ms or slower", failures)

    def test_cpu_rss_and_growth_budgets_are_independent(self) -> None:
        """CPU、稳态/峰值 RSS 与循环增长任一超限都必须单独报告。"""

        evidence = deepcopy(self.evidence)
        evidence["idleCpuPercentOfOneLogicalCore"] = [6.1] * 5
        evidence["steadyRssMiB"] = 360.1
        evidence["peakRssMiB"] = 600.1
        evidence["rssAfterCyclesMiB"] = 238.5
        result = self._evaluate(evidence=evidence)

        self.assertEqual(result["status"], "failed")
        failures = "\n".join(result["failures"])
        self.assertIn("CPU p95 exceeds 6%", failures)
        self.assertIn("steady whole-process-tree RSS", failures)
        self.assertIn("peak whole-process-tree RSS", failures)
        self.assertIn("RSS growth", failures)

    def test_tray_profile_controls_hidden_sampling(self) -> None:
        """启用托盘必须测隐藏状态，禁用托盘则不得强制该不适用场景。"""

        missing_hidden = deepcopy(self.evidence)
        missing_hidden.pop("hiddenTrayObservationSeconds")
        missing_hidden.pop("hiddenTrayCpuPercentOfOneLogicalCore")
        enabled_result = self._evaluate(evidence=missing_hidden)
        self.assertEqual(enabled_result["status"], "failed")
        self.assertTrue(
            any("hiddenTray" in failure for failure in enabled_result["failures"])
        )

        disabled_evidence = deepcopy(missing_hidden)
        disabled_evidence["trayEnabled"] = False
        disabled_result = self._evaluate(
            evidence=disabled_evidence, tray_enabled=False
        )
        self.assertEqual(disabled_result["status"], "passed")

    def test_parent_only_sampling_or_failed_cleanup_cannot_pass(self) -> None:
        """只采主进程或遗留受管进程都必须拒绝候选。"""

        evidence = deepcopy(self.evidence)
        evidence["wholeProcessTree"] = False
        evidence["allProcessesRecovered"] = False
        result = self._evaluate(evidence=evidence)

        self.assertEqual(result["status"], "failed")
        failures = "\n".join(result["failures"])
        self.assertIn("wholeProcessTree", failures)
        self.assertIn("allProcessesRecovered", failures)
        self.assertFalse(result["waiverAllowed"])
        non_waivable = "\n".join(result["nonWaivableFailures"])
        self.assertIn("wholeProcessTree", non_waivable)
        self.assertIn("allProcessesRecovered", non_waivable)

    def test_integrity_failures_are_non_waivable_and_exit_three(self) -> None:
        """整树、探针字节或进程回收不完整都必须以不可豁免状态退出。"""

        for key in (
            "wholeProcessTree",
            "probeBytesUnmodified",
            "allProcessesRecovered",
        ):
            with self.subTest(key=key):
                evidence = deepcopy(self.evidence)
                evidence[key] = False

                exit_code, saved = self._run_cli(
                    evidence, f"raw-{key}.json", f"probe.{key}.performance.json"
                )

                self.assertEqual(exit_code, 3)
                failure = f"evidence.{key} must be true"
                self.assertEqual(saved["status"], "failed")
                self.assertFalse(saved["waiverAllowed"])
                self.assertIn(failure, saved["failures"])
                self.assertIn(failure, saved["nonWaivableFailures"])

    def test_window_state_requires_same_seed_before_every_launch(self) -> None:
        """预热与每次冷启动都必须按顺序从同一已验证种子开始。"""

        missing_reset = deepcopy(self.evidence)
        missing_reset["windowStateIsolation"]["preLaunchResets"].pop()
        missing_result = self._evaluate(evidence=missing_reset)
        self.assertEqual(missing_result["status"], "failed")
        self.assertTrue(
            any(
                "exactly 1 warmup and 5 cold-start" in failure
                for failure in missing_result["failures"]
            )
        )

        changed_seed = deepcopy(self.evidence)
        changed_seed["windowStateIsolation"]["preLaunchResets"][2]["observed"] = {
            "kind": "present",
            "fingerprint": "c" * 64,
        }
        changed_result = self._evaluate(evidence=changed_seed)
        self.assertEqual(changed_result["status"], "failed")
        self.assertTrue(
            any(
                "must match windowStateIsolation.seed" in failure
                for failure in changed_result["failures"]
            )
        )

        two_warmups = deepcopy(self.evidence)
        two_warmups["warmupRuns"] = 2
        two_warmups["windowStateIsolation"]["preLaunchResets"].insert(
            1,
            {"phase": "warmup", "run": 2, "observed": {"kind": "absent"}},
        )
        two_warmup_result = self._evaluate(evidence=two_warmups)
        self.assertEqual(two_warmup_result["status"], "passed")

    def test_absent_original_and_present_seed_are_valid(self) -> None:
        """原缺席恢复与有摘要的固定 seed 都能形成不泄露内容的有效证据。"""

        evidence = deepcopy(self.evidence)
        isolation = evidence["windowStateIsolation"]
        isolation["original"] = {"kind": "absent"}
        isolation["restoration"] = {
            "observed": {"kind": "absent"},
            "verified": True,
        }
        isolation["seed"] = {"kind": "present", "fingerprint": "c" * 64}
        for reset in isolation["preLaunchResets"]:
            reset["observed"] = {"kind": "present", "fingerprint": "c" * 64}

        result = self._evaluate(evidence=evidence)

        self.assertEqual(result["status"], "passed")
        self.assertTrue(result["windowStateRecoveryVerified"])

    def test_unrestored_window_state_is_non_waivable(self) -> None:
        """原状态指纹不匹配时保存失败证据并使用不可豁免退出码。"""

        evidence = deepcopy(self.evidence)
        evidence["windowStateIsolation"]["restoration"]["observed"][
            "fingerprint"
        ] = "c" * 64
        result = self._evaluate(evidence=evidence)
        self.assertEqual(result["status"], "failed")
        self.assertFalse(result["windowStateRecoveryVerified"])
        self.assertFalse(result["waiverAllowed"])
        self.assertTrue(result["nonWaivableFailures"])

        exit_code, saved = self._run_cli(
            evidence, "raw-unrestored.json", "probe.unrestored.performance.json"
        )

        self.assertEqual(exit_code, 3)
        self.assertFalse(saved["windowStateRecoveryVerified"])
        self.assertFalse(saved["waiverAllowed"])
        self.assertTrue(saved["nonWaivableFailures"])

    def test_window_state_output_removes_rejected_path_and_bytes(self) -> None:
        """失败证据只保留白名单字段，不固化误填的本机路径或原始字节。"""

        evidence = deepcopy(self.evidence)
        evidence["windowStateIsolation"]["path"] = "/Users/alice/private/state.json"
        evidence["windowStateIsolation"]["original"]["bytes"] = "private-json"

        result = self._evaluate(evidence=evidence)

        self.assertEqual(result["status"], "failed")
        serialized = json.dumps(result, ensure_ascii=False)
        self.assertNotIn("/Users/alice/private/state.json", serialized)
        self.assertNotIn("private-json", serialized)
        self.assertNotIn('"path"', serialized)
        self.assertNotIn('"bytes"', serialized)

    def test_cli_preserves_failed_observations_and_never_implies_waiver(self) -> None:
        """Helper 非零时仍原子保存失败指标，且不会自行制造用户豁免。"""

        evidence = deepcopy(self.evidence)
        evidence["peakRssMiB"] = 601

        exit_code, saved = self._run_cli(
            evidence, "raw-performance.json", "probe.performance.json"
        )

        self.assertEqual(exit_code, 1)
        self.assertEqual(saved["status"], "failed")
        self.assertEqual(saved["observations"]["peakRssMiB"], 601)
        self.assertTrue(saved["windowStateRecoveryVerified"])
        self.assertTrue(saved["waiverAllowed"])
        self.assertEqual(saved["nonWaivableFailures"], [])
        self.assertNotIn("waiver", saved)


if __name__ == "__main__":
    unittest.main()
