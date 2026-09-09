#!/usr/bin/env python3
"""校验 GUI Release no-bundle 探针的原生性能观测并生成绑定证据。"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import statistics
import sys
import tempfile
from typing import Any

THRESHOLDS = {
    "coldStartRuns": 5,
    "coldStartMedianMsMaximum": 2400.0,
    "coldStartMaximumMs": 3600.0,
    "interactionSamplesMinimum": 20,
    "interactionP95MsMaximum": 120.0,
    "interactionSingleMsExclusiveMaximum": 240.0,
    "longTaskMsMinimum": 50.0,
    "longTaskMsExclusiveMaximum": 240.0,
    "cpuObservationSecondsMinimum": 30.0,
    "cpuSamplesMinimum": 5,
    "idleCpuP95PercentMaximum": 6.0,
    "hiddenTrayCpuP95PercentMaximum": 2.4,
    "steadyRssMiBMaximum": 360.0,
    "peakRssMiBMaximum": 600.0,
    "rssGrowthPercentMaximum": 18.0,
    "rssGrowthMiBMinimumAllowance": 38.4,
    "navigationInteractionCyclesMinimum": 20,
}

EVIDENCE_SCHEMA_VERSION = 2
SOURCE_COMMIT_PATTERN = re.compile(r"^[0-9a-f]{40}$")
SHA256_PATTERN = re.compile(r"^[0-9a-f]{64}$")
WINDOW_STATE_FINGERPRINT_ALGORITHM = "hmac-sha256-ephemeral-key"
NON_WAIVABLE_EVIDENCE_INTEGRITY_KEYS = frozenset(
    {
        "wholeProcessTree",
        "probeBytesUnmodified",
        "allProcessesRecovered",
    }
)
EVIDENCE_ALLOWED_KEYS = frozenset(
    {
        "schemaVersion",
        "performanceProbeSha256",
        "performanceProbeKind",
        "sourceCommit",
        "sourceTreeState",
        "platform",
        "architecture",
        "buildMode",
        "buildProfile",
        "performanceSelection",
        "e2eSelection",
        "trayEnabled",
        "observationAvailable",
        "wholeProcessTree",
        "probeBytesUnmodified",
        "allProcessesRecovered",
        "rendererTimingSource",
        "processSampler",
        "warmupRuns",
        "windowStateIsolation",
        "coldStartVisibleUsableMs",
        "interactions",
        "longTasksMs",
        "idleObservationSeconds",
        "idleCpuPercentOfOneLogicalCore",
        "hiddenTrayObservationSeconds",
        "hiddenTrayCpuPercentOfOneLogicalCore",
        "steadyRssMiB",
        "peakRssMiB",
        "rssBeforeCyclesMiB",
        "rssAfterCyclesMiB",
        "navigationInteractionCycles",
    }
)


class PerformanceEvidenceError(RuntimeError):
    """表示输入文件或证据输出边界不安全。"""


def _is_finite_number(value: object) -> bool:
    """只接受有限且不是布尔值的整数或浮点数。"""
    return (
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(float(value))
    )


def _read_json_object(path: Path, label: str) -> dict[str, Any]:
    """从普通非符号链接文件读取一个 JSON 对象。"""
    if not path.exists() or not path.is_file() or path.is_symlink():
        raise PerformanceEvidenceError(f"{label} must be a regular non-symlink file: {path}")
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        raise PerformanceEvidenceError(f"cannot read {label}: {exc}") from exc
    if not isinstance(value, dict):
        raise PerformanceEvidenceError(f"{label} root must be a JSON object")
    return value


def _sha256(path: Path) -> str:
    """流式计算普通候选文件的 SHA-256。"""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _number(
    document: dict[str, Any],
    key: str,
    errors: list[str],
    *,
    minimum: float = 0.0,
    integer: bool = False,
) -> float | int | None:
    """提取满足非负下界的数值字段并累积结构错误。"""
    value = document.get(key)
    if not _is_finite_number(value):
        errors.append(f"{key} must be a finite number")
        return None
    numeric = float(value)
    if numeric < minimum:
        errors.append(f"{key} must be >= {minimum:g}")
    if integer and numeric != int(numeric):
        errors.append(f"{key} must be an integer")
        return None
    return int(numeric) if integer else numeric


def _number_list(
    document: dict[str, Any],
    key: str,
    errors: list[str],
    *,
    minimum_items: int = 0,
    exact_items: int | None = None,
) -> list[float]:
    """提取有限非负数值列表并校验样本数量。"""
    raw = document.get(key)
    if not isinstance(raw, list):
        errors.append(f"{key} must be an array")
        return []
    values: list[float] = []
    for index, value in enumerate(raw):
        if not _is_finite_number(value) or float(value) < 0:
            errors.append(f"{key}[{index}] must be a finite non-negative number")
            continue
        values.append(float(value))
    if exact_items is not None and len(raw) != exact_items:
        errors.append(f"{key} must contain exactly {exact_items} samples")
    if len(raw) < minimum_items:
        errors.append(f"{key} must contain at least {minimum_items} samples")
    return values


def _nearest_rank_p95(values: list[float]) -> float | None:
    """按 nearest-rank 定义计算 p95。"""
    if not values:
        return None
    ordered = sorted(values)
    return ordered[math.ceil(0.95 * len(ordered)) - 1]


def _expect_equal(
    document: dict[str, Any],
    key: str,
    expected: object,
    errors: list[str],
    label: str,
) -> None:
    """要求证据字段与候选事实精确一致。"""
    if document.get(key) != expected:
        errors.append(f"{label}.{key} must equal {expected!r}")


def _reject_unexpected_keys(
    container: dict[str, Any],
    allowed_keys: set[str],
    label: str,
    errors: list[str],
    non_waivable: list[str] | None = None,
) -> list[str]:
    """拒绝白名单外字段；传入 non_waivable 时同时标记为不可豁免。"""
    unexpected = sorted(set(container) - allowed_keys)
    if unexpected:
        message = f"{label} contains unsupported fields: {', '.join(unexpected)}"
        errors.append(message)
        if non_waivable is not None:
            non_waivable.append(message)
    return unexpected


def _state_fingerprint(
    raw: object,
    label: str,
    errors: list[str],
) -> tuple[tuple[str, str | None] | None, dict[str, Any]]:
    """校验并白名单化不泄露路径或内容的 window-state 指纹。"""
    if not isinstance(raw, dict):
        errors.append(f"{label} must be an object")
        return None, {}

    unexpected = _reject_unexpected_keys(raw, {"kind", "fingerprint"}, label, errors)

    kind = raw.get("kind")
    sanitized: dict[str, Any] = {
        "kind": kind if kind in {"present", "absent"} else "invalid"
    }
    if kind not in {"present", "absent"}:
        errors.append(f"{label}.kind must be present or absent")
        return None, sanitized

    fingerprint = raw.get("fingerprint")
    if kind == "present":
        if not isinstance(fingerprint, str) or not SHA256_PATTERN.fullmatch(
            fingerprint
        ):
            errors.append(
                f"{label}.fingerprint must be 64 lowercase hexadecimal characters "
                "when kind is present"
            )
            return None, sanitized
        sanitized["fingerprint"] = fingerprint
    elif "fingerprint" in raw:
        errors.append(f"{label}.fingerprint must be absent when kind is absent")
        return None, sanitized

    if unexpected:
        return None, sanitized
    return (kind, fingerprint if kind == "present" else None), sanitized


def _validate_window_state_isolation(
    evidence: dict[str, Any],
    warmup_runs: int | float | None,
    errors: list[str],
) -> tuple[dict[str, Any], bool, list[str]]:
    """校验逐次同种子重置，并白名单化恢复证据。"""
    non_waivable: list[str] = []

    def add_error(message: str, *, recovery: bool = False) -> None:
        errors.append(message)
        if recovery:
            non_waivable.append(message)

    def absorb(messages: list[str]) -> None:
        """把子校验产生的错误标记为不可豁免并转发。"""
        for message in messages:
            add_error(message, recovery=True)

    raw = evidence.get("windowStateIsolation")
    if not isinstance(raw, dict):
        add_error("windowStateIsolation must be an object", recovery=True)
        return {}, False, non_waivable

    allowed_keys = {
        "targetResolved",
        "snapshotStoredOutsideAppData",
        "originalSnapshotVerified",
        "fingerprintAlgorithm",
        "original",
        "seed",
        "preLaunchResets",
        "restoration",
    }
    _reject_unexpected_keys(raw, allowed_keys, "windowStateIsolation", errors)

    sanitized: dict[str, Any] = {}
    target_resolved = raw.get("targetResolved") is True
    sanitized["targetResolved"] = target_resolved
    if not target_resolved:
        add_error("windowStateIsolation.targetResolved must be true", recovery=True)

    snapshot_outside = raw.get("snapshotStoredOutsideAppData") is True
    sanitized["snapshotStoredOutsideAppData"] = snapshot_outside
    if not snapshot_outside:
        add_error(
            "windowStateIsolation.snapshotStoredOutsideAppData must be true",
            recovery=True,
        )

    snapshot_verified = raw.get("originalSnapshotVerified") is True
    sanitized["originalSnapshotVerified"] = snapshot_verified
    if not snapshot_verified:
        add_error(
            "windowStateIsolation.originalSnapshotVerified must be true",
            recovery=True,
        )

    fingerprint_algorithm = raw.get("fingerprintAlgorithm")
    sanitized["fingerprintAlgorithm"] = (
        fingerprint_algorithm
        if fingerprint_algorithm == WINDOW_STATE_FINGERPRINT_ALGORITHM
        else "invalid"
    )
    if fingerprint_algorithm != WINDOW_STATE_FINGERPRINT_ALGORITHM:
        add_error(
            "windowStateIsolation.fingerprintAlgorithm must equal "
            f"{WINDOW_STATE_FINGERPRINT_ALGORITHM!r}",
            recovery=True,
        )

    original_errors: list[str] = []
    original, sanitized_original = _state_fingerprint(
        raw.get("original"), "windowStateIsolation.original", original_errors
    )
    sanitized["original"] = sanitized_original
    absorb(original_errors)

    seed_errors: list[str] = []
    seed, sanitized_seed = _state_fingerprint(
        raw.get("seed"), "windowStateIsolation.seed", seed_errors
    )
    sanitized["seed"] = sanitized_seed
    errors.extend(seed_errors)

    reset_items = raw.get("preLaunchResets")
    if not isinstance(reset_items, list):
        add_error("windowStateIsolation.preLaunchResets must be an array")
        reset_items = []

    expected_warmups = (
        int(warmup_runs)
        if isinstance(warmup_runs, int) and warmup_runs >= 1
        else 0
    )
    expected_runs = [
        ("warmup", run) for run in range(1, expected_warmups + 1)
    ]
    expected_runs.extend(
        ("cold-start", run)
        for run in range(1, int(THRESHOLDS["coldStartRuns"]) + 1)
    )
    if len(reset_items) != len(expected_runs):
        add_error(
            "windowStateIsolation.preLaunchResets must contain exactly "
            f"{expected_warmups} warmup and "
            f"{int(THRESHOLDS['coldStartRuns'])} cold-start resets"
        )

    sanitized_resets: list[dict[str, Any]] = []
    for index, item in enumerate(reset_items):
        label = f"windowStateIsolation.preLaunchResets[{index}]"
        if not isinstance(item, dict):
            add_error(f"{label} must be an object")
            sanitized_resets.append({})
            continue
        _reject_unexpected_keys(item, {"phase", "run", "observed"}, label, errors)
        phase = item.get("phase")
        run = item.get("run")
        observed_errors: list[str] = []
        observed, sanitized_observed = _state_fingerprint(
            item.get("observed"), f"{label}.observed", observed_errors
        )
        errors.extend(observed_errors)
        run_is_integer = isinstance(run, int) and not isinstance(run, bool)
        sanitized_item: dict[str, Any] = {
            "phase": phase if phase in {"warmup", "cold-start"} else "invalid",
            "run": run if run_is_integer else None,
            "observed": sanitized_observed,
        }
        sanitized_resets.append(sanitized_item)
        if not run_is_integer:
            add_error(f"{label}.run must be an integer")
        if index < len(expected_runs):
            expected_phase, expected_run = expected_runs[index]
            if phase != expected_phase:
                add_error(f"{label}.phase must equal {expected_phase!r}")
            if run_is_integer and run != expected_run:
                add_error(f"{label}.run must equal {expected_run}")
        if observed is not None and seed is not None and observed != seed:
            add_error(f"{label}.observed must match windowStateIsolation.seed")
    sanitized["preLaunchResets"] = sanitized_resets

    restoration_raw = raw.get("restoration")
    restoration_sanitized: dict[str, Any] = {}
    restored: tuple[str, str | None] | None = None
    restoration_verified = False
    if not isinstance(restoration_raw, dict):
        add_error("windowStateIsolation.restoration must be an object", recovery=True)
    else:
        _reject_unexpected_keys(
            restoration_raw,
            {"observed", "verified"},
            "windowStateIsolation.restoration",
            errors,
            non_waivable,
        )
        restored_errors: list[str] = []
        restored, sanitized_restored = _state_fingerprint(
            restoration_raw.get("observed"),
            "windowStateIsolation.restoration.observed",
            restored_errors,
        )
        restoration_sanitized["observed"] = sanitized_restored
        absorb(restored_errors)
        restoration_verified = restoration_raw.get("verified") is True
        restoration_sanitized["verified"] = restoration_verified
        if not restoration_verified:
            add_error(
                "windowStateIsolation.restoration.verified must be true",
                recovery=True,
            )
    sanitized["restoration"] = restoration_sanitized

    restored_matches = original is not None and restored == original
    if original is not None and restored is not None and not restored_matches:
        add_error(
            "windowStateIsolation.restoration.observed must match "
            "windowStateIsolation.original",
            recovery=True,
        )

    recovery_verified = not non_waivable and restored_matches
    return sanitized, recovery_verified, non_waivable


def _probe_name(manifest: dict[str, Any], errors: list[str]) -> str:
    """从 GUI manifest 提取明确命名的 no-bundle 运行探针。"""
    name = manifest.get("performanceProbe")
    if not isinstance(name, str) or not name:
        errors.append("manifest.performanceProbe must name the runtime executable")
        return ""
    if Path(name).name != name:
        errors.append("manifest.performanceProbe must be a basename")
    return name


def _interaction_durations(
    evidence: dict[str, Any], errors: list[str]
) -> list[float]:
    """提取具有可观察结果的批准交互时延。"""
    raw = evidence.get("interactions")
    if not isinstance(raw, list):
        errors.append("interactions must be an array")
        return []
    if len(raw) < THRESHOLDS["interactionSamplesMinimum"]:
        errors.append(
            "interactions must contain at least "
            f"{THRESHOLDS['interactionSamplesMinimum']} samples"
        )
    durations: list[float] = []
    for index, item in enumerate(raw):
        if not isinstance(item, dict):
            errors.append(f"interactions[{index}] must be an object")
            continue
        name = item.get("name")
        if not isinstance(name, str) or not name.strip():
            errors.append(f"interactions[{index}].name must be non-empty")
        if item.get("observableResult") is not True:
            errors.append(f"interactions[{index}].observableResult must be true")
        duration = item.get("durationMs")
        if not _is_finite_number(duration) or float(duration) < 0:
            errors.append(
                f"interactions[{index}].durationMs must be a finite non-negative number"
            )
            continue
        durations.append(float(duration))
    return durations


def evaluate(
    probe: Path,
    manifest: dict[str, Any],
    evidence: dict[str, Any],
    tray_enabled: bool,
    initial_errors: list[str] | None = None,
) -> dict[str, Any]:
    """对探针绑定、采样完整性和全部固定阈值作一次确定性判定。"""
    errors = list(initial_errors or [])
    actual_sha = ""
    if not probe.exists() or not probe.is_file() or probe.is_symlink():
        errors.append("performance probe must be a regular non-symlink file")
    else:
        try:
            actual_sha = _sha256(probe)
        except OSError as exc:
            errors.append(f"cannot hash performance probe: {exc}")

    probe_name = _probe_name(manifest, errors)
    if probe_name and probe.name != probe_name:
        errors.append("probe basename does not match manifest.performanceProbe")

    if manifest.get("interface") != "gui":
        errors.append("manifest.interface must be 'gui'")
    performance_selection = manifest.get("performanceSelection")
    if performance_selection != "enabled":
        errors.append(
            "manifest.performanceSelection must be 'enabled' for performance validation"
        )
    if manifest.get("performanceProbeKind") != "tauri-no-bundle-executable":
        errors.append(
            "manifest.performanceProbeKind must be 'tauri-no-bundle-executable'"
        )
    if manifest.get("performanceProbeBuildProfile") != "release":
        errors.append("manifest.performanceProbeBuildProfile must be 'release'")
    if manifest.get("sourceTreeState") != "clean":
        errors.append("manifest.sourceTreeState must be 'clean'")
    source_commit = manifest.get("sourceCommit")
    if not isinstance(source_commit, str) or not SOURCE_COMMIT_PATTERN.fullmatch(
        source_commit
    ):
        errors.append("manifest.sourceCommit must be 40 lowercase hexadecimal characters")
        source_commit = ""
    manifest_sha = manifest.get("performanceProbeSha256")
    if not isinstance(manifest_sha, str) or not SHA256_PATTERN.fullmatch(manifest_sha):
        errors.append(
            "manifest.performanceProbeSha256 must be 64 lowercase hexadecimal characters"
        )
    elif actual_sha and manifest_sha != actual_sha:
        errors.append("manifest.performanceProbeSha256 does not match probe bytes")
    if manifest.get("buildMode") != "native":
        errors.append("only a native buildMode can pass GUI performance validation")

    platform = manifest.get("platform")
    if not isinstance(platform, str) or not platform.strip():
        errors.append("manifest.platform must be a non-empty string")
    architecture = manifest.get("architecture")
    if not isinstance(architecture, str) or not architecture.strip():
        errors.append("manifest.architecture must be a non-empty string")

    e2e_selection = manifest.get("e2eSelection")
    if e2e_selection not in {"enabled", "disabled"}:
        errors.append("manifest.e2eSelection must be enabled or disabled")

    _reject_unexpected_keys(evidence, EVIDENCE_ALLOWED_KEYS, "evidence", errors)
    _expect_equal(
        evidence, "schemaVersion", EVIDENCE_SCHEMA_VERSION, errors, "evidence"
    )
    _expect_equal(evidence, "performanceSelection", "enabled", errors, "evidence")
    _expect_equal(evidence, "performanceProbeSha256", actual_sha, errors, "evidence")
    _expect_equal(evidence, "sourceCommit", source_commit, errors, "evidence")
    _expect_equal(evidence, "sourceTreeState", "clean", errors, "evidence")
    _expect_equal(
        evidence, "platform", platform, errors, "evidence"
    )
    _expect_equal(
        evidence, "architecture", architecture, errors, "evidence"
    )
    _expect_equal(evidence, "buildMode", "native", errors, "evidence")
    _expect_equal(evidence, "buildProfile", "release", errors, "evidence")
    _expect_equal(
        evidence,
        "performanceProbeKind",
        "tauri-no-bundle-executable",
        errors,
        "evidence",
    )
    _expect_equal(evidence, "e2eSelection", e2e_selection, errors, "evidence")
    _expect_equal(evidence, "trayEnabled", tray_enabled, errors, "evidence")

    non_waivable_failures: list[str] = []
    for key in (
        "observationAvailable",
        "wholeProcessTree",
        "probeBytesUnmodified",
        "allProcessesRecovered",
    ):
        if evidence.get(key) is not True:
            failure = f"evidence.{key} must be true"
            errors.append(failure)
            if key in NON_WAIVABLE_EVIDENCE_INTEGRITY_KEYS:
                non_waivable_failures.append(failure)
    if evidence.get("rendererTimingSource") != "performance-observer":
        errors.append("rendererTimingSource must be 'performance-observer'")
    process_sampler = evidence.get("processSampler")
    if not isinstance(process_sampler, str) or not process_sampler.strip():
        errors.append("processSampler must be non-empty")

    warmup_runs = _number(
        evidence, "warmupRuns", errors, minimum=1, integer=True
    )
    (
        sanitized_window_state,
        window_state_recovery_verified,
        window_state_non_waivable_failures,
    ) = _validate_window_state_isolation(evidence, warmup_runs, errors)
    non_waivable_failures.extend(window_state_non_waivable_failures)
    cold_starts = _number_list(
        evidence,
        "coldStartVisibleUsableMs",
        errors,
        exact_items=int(THRESHOLDS["coldStartRuns"]),
    )
    cold_median = statistics.median(cold_starts) if cold_starts else None
    cold_max = max(cold_starts) if cold_starts else None
    if cold_median is not None and cold_median > THRESHOLDS[
        "coldStartMedianMsMaximum"
    ]:
        errors.append("cold-start median exceeds 2400 ms")
    if cold_max is not None and cold_max > THRESHOLDS["coldStartMaximumMs"]:
        errors.append("cold-start maximum exceeds 3600 ms")

    interaction_durations = _interaction_durations(evidence, errors)
    interaction_p95 = _nearest_rank_p95(interaction_durations)
    interaction_max = max(interaction_durations) if interaction_durations else None
    if interaction_p95 is not None and interaction_p95 > THRESHOLDS[
        "interactionP95MsMaximum"
    ]:
        errors.append("interaction p95 exceeds 120 ms")
    if interaction_max is not None and interaction_max >= THRESHOLDS[
        "interactionSingleMsExclusiveMaximum"
    ]:
        errors.append("an interaction is 240 ms or slower")

    long_tasks = _number_list(evidence, "longTasksMs", errors)
    for index, duration in enumerate(long_tasks):
        if duration < THRESHOLDS["longTaskMsMinimum"]:
            errors.append(f"longTasksMs[{index}] is shorter than the 50 ms record floor")
    long_task_max = max(long_tasks) if long_tasks else 0.0
    if long_task_max >= THRESHOLDS["longTaskMsExclusiveMaximum"]:
        errors.append("a Long Task is 240 ms or slower")

    idle_seconds = _number(
        evidence,
        "idleObservationSeconds",
        errors,
        minimum=THRESHOLDS["cpuObservationSecondsMinimum"],
    )
    idle_cpu = _number_list(
        evidence,
        "idleCpuPercentOfOneLogicalCore",
        errors,
        minimum_items=int(THRESHOLDS["cpuSamplesMinimum"]),
    )
    idle_cpu_p95 = _nearest_rank_p95(idle_cpu)
    if idle_cpu_p95 is not None and idle_cpu_p95 > THRESHOLDS[
        "idleCpuP95PercentMaximum"
    ]:
        errors.append("idle whole-process-tree CPU p95 exceeds 6%")

    hidden_seconds: float | int | None = None
    hidden_cpu_p95: float | None = None
    if tray_enabled:
        hidden_seconds = _number(
            evidence,
            "hiddenTrayObservationSeconds",
            errors,
            minimum=THRESHOLDS["cpuObservationSecondsMinimum"],
        )
        hidden_cpu = _number_list(
            evidence,
            "hiddenTrayCpuPercentOfOneLogicalCore",
            errors,
            minimum_items=int(THRESHOLDS["cpuSamplesMinimum"]),
        )
        hidden_cpu_p95 = _nearest_rank_p95(hidden_cpu)
        if hidden_cpu_p95 is not None and hidden_cpu_p95 > THRESHOLDS[
            "hiddenTrayCpuP95PercentMaximum"
        ]:
            errors.append("hidden/tray whole-process-tree CPU p95 exceeds 2.4%")
    elif any(
        key in evidence
        for key in (
            "hiddenTrayObservationSeconds",
            "hiddenTrayCpuPercentOfOneLogicalCore",
        )
    ):
        errors.append("hidden/tray samples must be absent when tray is disabled")

    steady_rss = _number(evidence, "steadyRssMiB", errors)
    peak_rss = _number(evidence, "peakRssMiB", errors)
    rss_before = _number(evidence, "rssBeforeCyclesMiB", errors)
    rss_after = _number(evidence, "rssAfterCyclesMiB", errors)
    cycles = _number(
        evidence,
        "navigationInteractionCycles",
        errors,
        minimum=THRESHOLDS["navigationInteractionCyclesMinimum"],
        integer=True,
    )
    if steady_rss is not None and steady_rss > THRESHOLDS["steadyRssMiBMaximum"]:
        errors.append("steady whole-process-tree RSS exceeds 360 MiB")
    if peak_rss is not None and peak_rss > THRESHOLDS["peakRssMiBMaximum"]:
        errors.append("peak whole-process-tree RSS exceeds 600 MiB")

    rss_growth = None
    rss_growth_limit = None
    if rss_before is not None and rss_after is not None:
        rss_growth = rss_after - rss_before
        rss_growth_limit = max(
            rss_before * THRESHOLDS["rssGrowthPercentMaximum"] / 100.0,
            THRESHOLDS["rssGrowthMiBMinimumAllowance"],
        )
        if rss_growth > rss_growth_limit and not math.isclose(
            rss_growth,
            rss_growth_limit,
            rel_tol=1e-9,
            abs_tol=1e-9,
        ):
            errors.append("RSS growth after interaction cycles exceeds the allowed budget")

    metrics = {
        "warmupRuns": warmup_runs,
        "coldStartMedianMs": cold_median,
        "coldStartMaximumMs": cold_max,
        "interactionSamples": len(interaction_durations),
        "interactionP95Ms": interaction_p95,
        "interactionMaximumMs": interaction_max,
        "longTaskMaximumMs": long_task_max,
        "idleObservationSeconds": idle_seconds,
        "idleCpuP95PercentOfOneLogicalCore": idle_cpu_p95,
        "hiddenTrayObservationSeconds": hidden_seconds,
        "hiddenTrayCpuP95PercentOfOneLogicalCore": hidden_cpu_p95,
        "steadyRssMiB": steady_rss,
        "peakRssMiB": peak_rss,
        "navigationInteractionCycles": cycles,
        "rssGrowthMiB": rss_growth,
        "rssGrowthLimitMiB": rss_growth_limit,
    }
    status = "passed" if not errors else "failed"
    sanitized_observations = {
        key: evidence[key]
        for key in EVIDENCE_ALLOWED_KEYS
        if key in evidence and key != "windowStateIsolation"
    }
    sanitized_observations["windowStateIsolation"] = sanitized_window_state
    return {
        "schemaVersion": EVIDENCE_SCHEMA_VERSION,
        "kind": "gui-release-performance",
        "performanceSelection": performance_selection,
        "status": status,
        "windowStateRecoveryVerified": window_state_recovery_verified,
        "waiverAllowed": (
            status == "failed"
            and window_state_recovery_verified
            and not non_waivable_failures
        ),
        "nonWaivableFailures": non_waivable_failures,
        "probe": {
            "file": probe.name,
            "sha256": actual_sha,
            "sourceCommit": source_commit,
            "sourceTreeState": manifest.get("sourceTreeState"),
            "kind": manifest.get("performanceProbeKind"),
            "platform": platform,
            "architecture": architecture,
            "buildMode": manifest.get("buildMode"),
            "buildProfile": evidence.get("buildProfile"),
            "performanceSelection": performance_selection,
            "e2eSelection": e2e_selection,
        },
        "thresholdProfile": "gui-release-v2",
        "thresholds": THRESHOLDS,
        "metrics": metrics,
        "observations": sanitized_observations,
        "failures": errors,
    }


def _write_json_atomic(path: Path, payload: dict[str, Any], protected: list[Path]) -> None:
    """在既有普通目录内原子写证据，且不覆盖任何输入。"""
    parent = path.parent
    if not parent.exists() or not parent.is_dir() or parent.is_symlink():
        raise PerformanceEvidenceError("output parent must be a regular existing directory")
    if path.exists() and (not path.is_file() or path.is_symlink()):
        raise PerformanceEvidenceError("output must be a regular file when it exists")
    resolved_output = path.resolve(strict=False)
    if any(resolved_output == item.resolve(strict=False) for item in protected):
        raise PerformanceEvidenceError("output must not overwrite probe or input evidence")

    temporary_name = ""
    try:
        descriptor, temporary_name = tempfile.mkstemp(
            prefix=f".{path.name}.", suffix=".tmp", dir=parent
        )
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, ensure_ascii=False, indent=2, sort_keys=True)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary_name, path)
        temporary_name = ""
    finally:
        if temporary_name:
            try:
                os.unlink(temporary_name)
            except FileNotFoundError:
                pass


def _build_parser() -> argparse.ArgumentParser:
    """建立稳定、非交互的命令行参数。"""
    parser = argparse.ArgumentParser(
        description="Validate a native GUI Release no-bundle performance probe."
    )
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument(
        "--tray-enabled", choices=("enabled", "disabled"), required=True
    )
    parser.add_argument("--output", type=Path, required=True)
    return parser


def main(argv: list[str] | None = None) -> int:
    """读取输入、始终尽可能保存判定证据，并以退出码表达门禁结果。"""
    args = _build_parser().parse_args(argv)
    input_errors: list[str] = []
    try:
        manifest = _read_json_object(args.manifest, "manifest")
    except PerformanceEvidenceError as exc:
        manifest = {}
        input_errors.append(str(exc))
    try:
        evidence = _read_json_object(args.evidence, "evidence")
    except PerformanceEvidenceError as exc:
        evidence = {}
        input_errors.append(str(exc))

    result = evaluate(
        args.probe,
        manifest,
        evidence,
        args.tray_enabled == "enabled",
        input_errors,
    )
    try:
        _write_json_atomic(
            args.output,
            result,
            [args.probe, args.manifest, args.evidence],
        )
    except (OSError, PerformanceEvidenceError) as exc:
        print(f"performance evidence output failed: {exc}", file=sys.stderr)
        return 2

    print(
        json.dumps(
            {
                "status": result["status"],
                "evidence": args.output.name,
                "failures": result["failures"],
                "windowStateRecoveryVerified": result[
                    "windowStateRecoveryVerified"
                ],
                "waiverAllowed": result["waiverAllowed"],
                "nonWaivableFailures": result["nonWaivableFailures"],
            },
            ensure_ascii=False,
            sort_keys=True,
        )
    )
    if result["status"] == "passed":
        return 0
    return 1 if result["waiverAllowed"] else 3


if __name__ == "__main__":
    raise SystemExit(main())
