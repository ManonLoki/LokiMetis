# GUI Release 性能证据契约

只在为同一 clean HEAD 生成原生 Release profile Tauri `--no-bundle` 运行探针后读取。本契约不接受 DMG、NSIS、归档或签名容器作为性能探针。

## 探针 manifest 输入

调用 helper 前，构建记录至少提供以下事实：

```json
{
  "interface": "gui",
  "performanceProbe": "loki_metis_gui",
  "performanceProbeKind": "tauri-no-bundle-executable",
  "performanceProbeBuildProfile": "release",
  "performanceProbeSha256": "<64 lowercase hex>",
  "sourceCommit": "<40 lowercase hex>",
  "sourceTreeState": "clean",
  "buildMode": "native",
  "platform": "macos",
  "architecture": "aarch64",
  "performanceSelection": "enabled",
  "e2eSelection": "disabled"
}
```

`performanceSelection` 必须为 `enabled`，证明构建流程已经为当前 GUI 发布明确选择或被产品/渠道硬要求执行性能测量。`performanceProbe` 必须是普通 basename，摘要只对应该运行时可执行文件。后续安装包的 `installer`、`archive` 和 `sha256` 是不同事实，不能替代任何 `performanceProbe*` 字段。选择 `disabled` 且无硬要求时不得调用 helper，也不得生成本契约中的探针或证据。

## 原始观测输入

下面是结构示例，不是已执行验证结果。数值数组保存逐次原始观测，helper 才负责计算 median、maximum 和 nearest-rank p95。

```json
{
  "schemaVersion": 2,
  "performanceProbeSha256": "<64 lowercase hex>",
  "performanceProbeKind": "tauri-no-bundle-executable",
  "sourceCommit": "<40 lowercase hex>",
  "sourceTreeState": "clean",
  "platform": "macos",
  "architecture": "aarch64",
  "buildMode": "native",
  "buildProfile": "release",
  "performanceSelection": "enabled",
  "e2eSelection": "disabled",
  "trayEnabled": true,
  "observationAvailable": true,
  "wholeProcessTree": true,
  "probeBytesUnmodified": true,
  "allProcessesRecovered": true,
  "rendererTimingSource": "performance-observer",
  "processSampler": "platform-native whole-tree sampler",
  "warmupRuns": 1,
  "windowStateIsolation": {
    "targetResolved": true,
    "snapshotStoredOutsideAppData": true,
    "originalSnapshotVerified": true,
    "fingerprintAlgorithm": "hmac-sha256-ephemeral-key",
    "original": {
      "kind": "present",
      "fingerprint": "<64 lowercase hex>"
    },
    "seed": {
      "kind": "absent"
    },
    "preLaunchResets": [
      { "phase": "warmup", "run": 1, "observed": { "kind": "absent" } },
      { "phase": "cold-start", "run": 1, "observed": { "kind": "absent" } },
      { "phase": "cold-start", "run": 2, "observed": { "kind": "absent" } },
      { "phase": "cold-start", "run": 3, "observed": { "kind": "absent" } },
      { "phase": "cold-start", "run": 4, "observed": { "kind": "absent" } },
      { "phase": "cold-start", "run": 5, "observed": { "kind": "absent" } }
    ],
    "restoration": {
      "observed": {
        "kind": "present",
        "fingerprint": "<same original fingerprint>"
      },
      "verified": true
    }
  },
  "coldStartVisibleUsableMs": [1200, 1250, 1300, 1280, 1240],
  "interactions": [
    {
      "name": "approved-settings-navigation",
      "durationMs": 76,
      "observableResult": true
    }
  ],
  "longTasksMs": [58],
  "idleObservationSeconds": 30,
  "idleCpuPercentOfOneLogicalCore": [1.1, 0.8, 1.3, 0.9, 1.0],
  "hiddenTrayObservationSeconds": 30,
  "hiddenTrayCpuPercentOfOneLogicalCore": [0.5, 0.4, 0.6, 0.4, 0.5],
  "steadyRssMiB": 180,
  "peakRssMiB": 240,
  "rssBeforeCyclesMiB": 180,
  "rssAfterCyclesMiB": 198,
  "navigationInteractionCycles": 20
}
```

实际 `interactions` 至少 20 项。每项名称只描述已批准动作，不得包含用户名、业务载荷或绝对路径；`observableResult` 只有真实界面结果已确认时才为 `true`。`idleCpuPercentOfOneLogicalCore` 和适用的 `hiddenTrayCpuPercentOfOneLogicalCore` 是每个采样时点对 Tauri、WebView 和受管子进程整棵树求和后的原始值，至少各 5 项且覆盖不少于 30 秒。`longTasksMs` 只记录不短于 50 ms 的浏览器 Long Task，没有时使用空数组。

`trayEnabled: false` 时必须省略两个 `hiddenTray*` 字段；启用时二者都必需。任何观测不可用、部分进程树、探针字节变化或未回收进程都必须使用 `false`，helper 会失败关闭。其中 `wholeProcessTree`、`probeBytesUnmodified`、`allProcessesRecovered` 任一不为 `true` 都是不可豁免的证据完整性失败；普通性能指标超限在三项完整性标记仍为 `true` 且窗口原状态恢复成立时，仍可进入后续性能 waiver 流程。

`windowStateIsolation` 不得包含状态文件路径、绝对目录、原始字节或其他文件内容。`targetResolved: true` 表示调用方已经从应用 identity 和当前平台应用数据目录解析出唯一、非符号链接且不越界的 window-state 目标；`snapshotStoredOutsideAppData: true` 表示原状态快照位于应用数据目录之外的独立临时目录；`originalSnapshotVerified: true` 表示快照与执行前原字节或原缺席一致。`fingerprintAlgorithm` 固定为 `hmac-sha256-ephemeral-key`：每轮生成只存在于临时测量进程的随机密钥，以 HMAC-SHA256 对存在状态的字节生成相等性指纹，恢复复核后销毁密钥；证据不得保存密钥或使用可对低熵窗口几何离线枚举的裸 SHA-256。

`original`、`seed`、每个 reset 的 `observed` 与 `restoration.observed` 只允许使用 `kind: present | absent`；`present` 必须同时给出 64 位小写 `fingerprint`，`absent` 必须省略 `fingerprint`。`preLaunchResets` 必须按顺序逐项包含 `warmupRuns` 个 `phase: warmup` 和恰好 5 个 `phase: cold-start`，各 phase 的 `run` 从 1 连续递增，且每一项的 `observed` 与唯一 `seed` 完全相同。`restoration` 必须有 `verified: true`，其 `observed` 必须与 `original` 的 kind/fingerprint 完全相同，才能证明原字节或原缺席已经恢复。缺项、额外项、乱序、重复、指纹不匹配或恢复未验证都会失败关闭。helper 只把白名单化后的 `windowStateIsolation` 写入输出；误填的路径、字节或其他字段既导致失败，也不会被固化进失败证据。

## Helper 输出与最终 manifest

Helper 输出一个 `schemaVersion: 2`、`kind: gui-release-performance` 的 JSON 对象，保留白名单化后的 `observations`、计算后的 `metrics`、固定 `thresholds`、探针绑定和 `failures`，并增加 `windowStateRecoveryVerified`、`waiverAllowed` 与 `nonWaivableFailures`。只有窗口原状态恢复验证通过时 `windowStateRecoveryVerified` 才为 `true`；`nonWaivableFailures` 收录 `wholeProcessTree`、`probeBytesUnmodified`、`allProcessesRecovered` 任一不为 `true`，以及窗口状态恢复未验证的错误。`waiverAllowed` 只在失败证据仍证明三项完整性标记均为 `true`、窗口恢复成立且 `nonWaivableFailures` 为空时为 `true`。退出码 0 只对应 `status: passed`；退出码 1 对应已原子保存且 `waiverAllowed: true` 的可豁免失败，包括普通性能指标超限；退出码 3 对应已原子保存且包含至少一项不可豁免完整性失败、`waiverAllowed: false` 的失败；退出码 2 只表示 helper 无法安全写出证据。

完整打包 manifest 的 `performanceEvidence` 使用结构化对象而不是安装容器摘要：

```json
{
  "performanceSelection": "enabled",
  "performanceStatus": "passed",
  "performanceThresholdProfile": "gui-release-v2",
  "performanceProbe": "loki_metis_gui",
  "performanceProbeSha256": "<probe sha256>",
  "performanceEvidence": {
    "path": "loki_metis_gui.performance.json",
    "sourceCommit": "<40 lowercase hex>",
    "platform": "macos",
    "architecture": "aarch64",
    "buildMode": "native",
    "buildProfile": "release"
  },
  "performanceRuntimeBinding": {
    "stagedUnsignedRuntimeSha256": "<same probe sha256>",
    "packagedRuntimePath": "<relative runtime path>",
    "packagedRuntimeSha256": "<final runtime sha256>",
    "binding": "byte-identical"
  }
}
```

`gui-release-v2` 的固定允许上限为：冷启动中位数 2400 ms、最大值 3600 ms；交互 p95 120 ms、单次严格小于 240 ms；Long Task 严格小于 240 ms；空闲整进程树 CPU p95 6%、隐藏/托盘 2.4%；稳态 RSS 360 MiB、峰值 600 MiB；循环后 RSS 增长不超过 `max(初始 RSS × 18%, 38.4 MiB)`。该 profile 相对 v1 精确放宽 20%，但预热、样本数、观察时长、循环数和 50 ms Long Task 记录下限不变。v1 证据保留原语义，不能仅改 profile 名称后作为 v2 证据。

打包前 staged unsigned runtime 必须与探针逐字节相同。`binding` 只允许 `byte-identical` 或 `verified-signing-transition`：前者要求最终包内 runtime 也逐字节相同；后者必须另外保留签名前相同摘要、签名后 runtime 摘要和签名验证。最终验收重新定位包内 runtime 核对。容器 SHA-256 永远不能填入 `performanceProbeSha256`。

`waived` 只能引用 `status: failed` 且 `waiverAllowed: true` 的原始输出，并增加非空原因、确认时间、用户确认摘要、已尝试修复和剩余风险；不得删除失败数组、覆盖失败证据或把 helper 输出改成 `passed`。`wholeProcessTree`、`probeBytesUnmodified`、`allProcessesRecovered` 任一不为 `true`，`windowStateRecoveryVerified: false`，或其他原因导致 `waiverAllowed: false` 时，均禁止创建性能 waiver；必须先修复完整性失败并重新运行 helper。

当次选择为 `disabled` 且无产品/渠道硬要求时，构建流程跳过整个 no-bundle 性能探针与 helper，最终 manifest 记录 `performanceSelection: disabled`、`performanceStatus: Not run`、非空原因与剩余风险，并且不得生成 `performanceProbe`、`performanceProbeSha256`、`performanceEvidence`、`performanceThresholdProfile`、`performanceWaiver` 或 `performanceRuntimeBinding`。`Not run` 不能覆盖同一候选已经产生的真实 `failed` 证据；已有失败只能修复后重建，或保留失败证据并走 `waived`。
