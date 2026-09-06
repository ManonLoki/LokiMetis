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
            "schemaVersion": 3,
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
            "rendererTranscriptScope": "interaction-session",
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
            "coldStartVisibleUsableMs": [1000, 2000, 2000, 2000, 3000],
            "interactions": [
                {
                    "sequence": index + 3,
                    "target": (
                        "navigation-dashboard",
                        "navigation-monitor",
                        "navigation-settings",
                    )[index % 3],
                    "result": (
                        "dashboard-page",
                        "monitor-page",
                        "settings-page",
                    )[index % 3],
                    "durationMs": 199 if index == 19 else 100,
                    "observableResult": True,
                }
                for index in range(20)
            ],
            "longTasksMs": [50, 199],
            "idleObservationSeconds": 30,
            "idleCpuPercentOfOneLogicalCore": [5, 5, 5, 5, 5],
            "hiddenTrayObservationSeconds": 30,
            "hiddenTrayCpuPercentOfOneLogicalCore": [2, 2, 2, 2, 2],
            "steadyRssMiB": 300,
            "peakRssMiB": 500,
            "rssBeforeCyclesMiB": 200,
            "rssAfterCyclesMiB": 232,
            "navigationInteractionCycles": 20,
        }

    def _evaluate(
        self,
        *,
        manifest: dict[str, object] | None = None,
        evidence: dict[str, object] | None = None,
        renderer_records: list[dict[str, object]] | None = None,
        tray_enabled: bool = True,
    ) -> dict[str, object]:
        """以深拷贝输入执行判定，避免测试场景相互污染。"""

        selected_evidence = deepcopy(evidence if evidence is not None else self.evidence)
        transcript_path = self._write_renderer_transcript(
            "renderer-evaluate.jsonl",
            renderer_records
            if renderer_records is not None
            else self._renderer_records(selected_evidence),
        )
        return performance.evaluate(
            self.probe,
            deepcopy(manifest if manifest is not None else self.manifest),
            selected_evidence,
            transcript_path,
            tray_enabled,
        )

    def _renderer_records(
        self, evidence: dict[str, object]
    ) -> list[dict[str, object]]:
        """按 raw evidence 构造应用会生成的合法 transcript 基线。"""

        source = evidence.get("rendererTimingSource")
        records: list[dict[str, object]] = [
            {
                "kind": "renderer-capabilities",
                "longTaskSupported": source == "performance-observer",
                "timingSource": source,
                "sequence": 1,
            },
            {
                "kind": "main-window-ready",
                "wallTimeMs": 1_800_000_000_000,
                "monotonicTimeMs": 128,
                "sequence": 2,
            },
        ]
        raw_interactions = evidence.get("interactions")
        if isinstance(raw_interactions, list):
            for item in raw_interactions:
                if not isinstance(item, dict):
                    continue
                records.append(
                    {
                        "kind": "interaction",
                        "target": item.get("target"),
                        "result": item.get("result"),
                        "durationMs": item.get("durationMs"),
                        "sequence": item.get("sequence"),
                    }
                )
        raw_blocking = evidence.get("longTasksMs")
        if isinstance(raw_blocking, list):
            for duration in raw_blocking:
                records.append(
                    {
                        "kind": "renderer-blocking-interval",
                        "timingSource": source,
                        "startTimeMs": len(records) * 16,
                        "durationMs": duration,
                        "sequence": len(records) + 1,
                    }
                )
        records.append(
            {
                "kind": "session-finalized",
                "sequence": len(records) + 1,
                "recordCount": len(records),
            }
        )
        return records

    def _write_renderer_transcript(
        self, name: str, records: list[dict[str, object]]
    ) -> Path:
        """逐行写入隔离 JSONL transcript，供 helper 读取真实字节。"""

        path = self.root / name
        path.write_text(
            "".join(
                json.dumps(record, ensure_ascii=False, separators=(",", ":")) + "\n"
                for record in records
            ),
            encoding="utf-8",
        )
        return path

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
        renderer_records: list[dict[str, object]] | None = None,
    ) -> tuple[int, dict[str, object]]:
        """写入 manifest/evidence 后调用命令行入口，返回退出码与保存的 JSON。"""

        manifest_path = self._write_json("probe.manifest.json", self.manifest)
        evidence_path = self._write_json(evidence_name, evidence)
        transcript_path = self._write_renderer_transcript(
            f"{evidence_name}.jsonl",
            renderer_records
            if renderer_records is not None
            else self._renderer_records(evidence),
        )
        output_path = self.root / output_name
        exit_code = performance.main(
            [
                "--probe",
                str(self.probe),
                "--manifest",
                str(manifest_path),
                "--evidence",
                str(evidence_path),
                "--renderer-transcript",
                str(transcript_path),
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
        self.assertEqual(result["schemaVersion"], 3)
        self.assertEqual(result["performanceSelection"], "enabled")
        self.assertTrue(result["windowStateRecoveryVerified"])
        self.assertFalse(result["waiverAllowed"])
        metrics = result["metrics"]
        self.assertEqual(metrics["coldStartMedianMs"], 2000)
        self.assertEqual(metrics["coldStartMaximumMs"], 3000)
        self.assertEqual(metrics["interactionP95Ms"], 100)
        self.assertEqual(metrics["interactionMaximumMs"], 199)
        self.assertEqual(metrics["rssGrowthLimitMiB"], 32)

    def test_animation_frame_gap_renderer_timing_source_is_valid(self) -> None:
        """原生 Long Task 不可用时允许可见页面的测试专用帧间隔观测。"""

        evidence = deepcopy(self.evidence)
        evidence["rendererTimingSource"] = "animation-frame-gap"

        result = self._evaluate(evidence=evidence)

        self.assertEqual(result["status"], "passed")
        self.assertFalse(result["waiverAllowed"])

    def test_valid_renderer_transcript_is_bound_by_basename_and_sha256(self) -> None:
        """通过证据只公开 transcript basename 与实际字节摘要。"""

        exit_code, saved = self._run_cli(
            self.evidence, "raw-transcript.json", "probe.transcript.performance.json"
        )

        transcript_path = self.root / "raw-transcript.json.jsonl"
        expected_sha = hashlib.sha256(transcript_path.read_bytes()).hexdigest()
        self.assertEqual(exit_code, 0)
        self.assertEqual(saved["status"], "passed")
        self.assertEqual(
            saved["rendererTranscript"],
            {
                "file": transcript_path.name,
                "sha256": expected_sha,
                "scope": "interaction-session",
            },
        )
        self.assertNotIn(str(self.root), json.dumps(saved, ensure_ascii=False))

    def test_renderer_transcript_scope_is_required_and_non_waivable(self) -> None:
        """缺失或错误的单会话范围声明都必须作为完整性失败关闭。"""

        for name, value in (("missing", None), ("wrong", "cold-start-series")):
            with self.subTest(name=name):
                evidence = deepcopy(self.evidence)
                if value is None:
                    evidence.pop("rendererTranscriptScope")
                else:
                    evidence["rendererTranscriptScope"] = value

                exit_code, saved = self._run_cli(
                    evidence,
                    f"raw-scope-{name}.json",
                    f"probe.scope-{name}.performance.json",
                )

                self.assertEqual(exit_code, 3)
                self.assertEqual(saved["status"], "failed")
                self.assertFalse(saved["waiverAllowed"])
                self.assertTrue(
                    any(
                        "rendererTranscriptScope" in failure
                        for failure in saved["nonWaivableFailures"]
                    )
                )

    def test_renderer_transcript_structure_tampering_is_non_waivable(self) -> None:
        """未知字段、缺少结束记录、断序或错误计数都必须失败关闭。"""

        baseline = self._renderer_records(self.evidence)
        unknown_field = deepcopy(baseline)
        unknown_field[1]["pageTitle"] = "must-not-be-accepted"
        missing_final = deepcopy(baseline[:-1])
        broken_sequence = deepcopy(baseline)
        broken_sequence[1]["sequence"] = 9
        wrong_record_count = deepcopy(baseline)
        wrong_record_count[-1]["recordCount"] = 0
        duplicate_ready = deepcopy(baseline)
        duplicate_ready.insert(2, deepcopy(duplicate_ready[1]))
        for sequence, record in enumerate(duplicate_ready, start=1):
            record["sequence"] = sequence
        duplicate_ready[-1]["recordCount"] = len(duplicate_ready) - 1
        scenarios = {
            "unknown-field": unknown_field,
            "missing-final": missing_final,
            "broken-sequence": broken_sequence,
            "wrong-record-count": wrong_record_count,
            "duplicate-ready": duplicate_ready,
        }

        for name, records in scenarios.items():
            with self.subTest(name=name):
                exit_code, saved = self._run_cli(
                    self.evidence,
                    f"raw-{name}.json",
                    f"probe.{name}.performance.json",
                    records,
                )

                self.assertEqual(exit_code, 3)
                self.assertEqual(saved["status"], "failed")
                self.assertFalse(saved["waiverAllowed"])
                self.assertTrue(saved["nonWaivableFailures"])

    def test_renderer_transcript_pair_and_raw_observations_must_match(self) -> None:
        """非法导航配对及交互或阻塞区间不匹配都属于完整性失败。"""

        baseline = self._renderer_records(self.evidence)
        illegal_pair = deepcopy(baseline)
        illegal_pair[2]["result"] = "settings-page"
        blocking_source_mismatch = deepcopy(baseline)
        blocking_source_mismatch[-2]["timingSource"] = "animation-frame-gap"
        timing_source_mismatch = deepcopy(baseline)
        timing_source_mismatch[0]["longTaskSupported"] = False
        timing_source_mismatch[0]["timingSource"] = "animation-frame-gap"
        for record in timing_source_mismatch:
            if record.get("kind") == "renderer-blocking-interval":
                record["timingSource"] = "animation-frame-gap"

        mismatched_interaction = deepcopy(self.evidence)
        mismatched_interaction["interactions"][0]["durationMs"] = 99
        mismatched_blocking = deepcopy(self.evidence)
        mismatched_blocking["longTasksMs"] = [50]
        scenarios = {
            "illegal-pair": (self.evidence, illegal_pair),
            "blocking-source": (self.evidence, blocking_source_mismatch),
            "timing-source": (self.evidence, timing_source_mismatch),
            "interaction-mismatch": (mismatched_interaction, baseline),
            "blocking-mismatch": (mismatched_blocking, baseline),
        }

        for name, (evidence, records) in scenarios.items():
            with self.subTest(name=name):
                exit_code, saved = self._run_cli(
                    evidence,
                    f"raw-{name}.json",
                    f"probe.{name}.performance.json",
                    records,
                )

                self.assertEqual(exit_code, 3)
                self.assertFalse(saved["waiverAllowed"])
                self.assertTrue(saved["nonWaivableFailures"])

    def test_symbolic_link_renderer_transcript_is_rejected(self) -> None:
        """renderer transcript 符号链接不能作为可信应用证据。"""

        target = self._write_renderer_transcript(
            "renderer-target.jsonl", self._renderer_records(self.evidence)
        )
        link = self.root / "renderer-link.jsonl"
        try:
            link.symlink_to(target)
        except OSError as error:
            self.skipTest(f"symbolic links unavailable: {error}")

        result = performance.evaluate(
            self.probe,
            deepcopy(self.manifest),
            deepcopy(self.evidence),
            link,
            True,
        )

        self.assertEqual(result["status"], "failed")
        self.assertFalse(result["waiverAllowed"])
        self.assertTrue(
            any("regular non-symlink" in item for item in result["failures"])
        )

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
        self.assertFalse(result["waiverAllowed"])
        self.assertTrue(result["nonWaivableFailures"])

    def test_observation_contract_failures_are_non_waivable(self) -> None:
        """观测不可用、来源无效或采样器缺失都必须使用不可豁免退出码。"""

        scenarios = {
            "unavailable": ("observationAvailable", False),
            "invalid-renderer": ("rendererTimingSource", "unsupported"),
            "missing-sampler": ("processSampler", ""),
        }
        for name, (key, value) in scenarios.items():
            with self.subTest(name=name):
                evidence = deepcopy(self.evidence)
                evidence[key] = value

                exit_code, saved = self._run_cli(
                    evidence, f"raw-{name}.json", f"probe.{name}.performance.json"
                )

                self.assertEqual(exit_code, 3)
                self.assertEqual(saved["status"], "failed")
                self.assertFalse(saved["waiverAllowed"])
                self.assertTrue(saved["nonWaivableFailures"])

    def test_latency_and_long_task_fail_at_exclusive_limits(self) -> None:
        """单次交互或 Long Task 达到 200 ms 时必须失败。"""

        evidence = deepcopy(self.evidence)
        evidence["interactions"][19]["durationMs"] = 200
        evidence["longTasksMs"] = [200]
        result = self._evaluate(evidence=evidence)

        self.assertEqual(result["status"], "failed")
        failures = "\n".join(result["failures"])
        self.assertIn("interaction is 200 ms or slower", failures)
        self.assertIn("Long Task is 200 ms or slower", failures)

    def test_cpu_rss_and_growth_budgets_are_independent(self) -> None:
        """CPU、稳态/峰值 RSS 与循环增长任一超限都必须单独报告。"""

        evidence = deepcopy(self.evidence)
        evidence["idleCpuPercentOfOneLogicalCore"] = [5.1] * 5
        evidence["steadyRssMiB"] = 300.1
        evidence["peakRssMiB"] = 500.1
        evidence["rssAfterCyclesMiB"] = 232.1
        result = self._evaluate(evidence=evidence)

        self.assertEqual(result["status"], "failed")
        failures = "\n".join(result["failures"])
        self.assertIn("CPU p95 exceeds 5%", failures)
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
        self.assertFalse(missing_result["waiverAllowed"])
        self.assertTrue(missing_result["nonWaivableFailures"])

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
        self.assertFalse(changed_result["waiverAllowed"])
        self.assertTrue(changed_result["nonWaivableFailures"])

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
        self.assertFalse(result["waiverAllowed"])
        self.assertTrue(result["nonWaivableFailures"])

    def test_cli_preserves_failed_observations_and_never_implies_waiver(self) -> None:
        """Helper 非零时仍原子保存失败指标，且不会自行制造用户豁免。"""

        evidence = deepcopy(self.evidence)
        evidence["peakRssMiB"] = 501

        exit_code, saved = self._run_cli(
            evidence, "raw-performance.json", "probe.performance.json"
        )

        self.assertEqual(exit_code, 1)
        self.assertEqual(saved["status"], "failed")
        self.assertEqual(saved["observations"]["peakRssMiB"], 501)
        self.assertTrue(saved["windowStateRecoveryVerified"])
        self.assertTrue(saved["waiverAllowed"])
        self.assertEqual(saved["nonWaivableFailures"], [])
        self.assertNotIn("waiver", saved)

    def test_metric_and_integrity_failure_mix_is_non_waivable(self) -> None:
        """纯阈值超限一旦混入观测完整性失败就必须使用不可豁免退出码。"""

        evidence = deepcopy(self.evidence)
        evidence["peakRssMiB"] = 501
        evidence["observationAvailable"] = False

        exit_code, saved = self._run_cli(
            evidence, "raw-mixed.json", "probe.mixed.performance.json"
        )

        self.assertEqual(exit_code, 3)
        self.assertEqual(saved["status"], "failed")
        self.assertFalse(saved["waiverAllowed"])
        non_waivable = "\n".join(saved["nonWaivableFailures"])
        self.assertIn("observationAvailable", non_waivable)
        self.assertNotIn("peak whole-process-tree RSS", non_waivable)


if __name__ == "__main__":
    unittest.main()
