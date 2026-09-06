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

## 应用 renderer transcript

Helper 必须同时读取本轮应用自己生成的 JSONL transcript：

```text
python3 .agents/skills/desktop-test-gui-release-performance/scripts/validate_gui_release_performance.py --probe <release-no-bundle-executable> --manifest <build-manifest> --evidence <raw-json> --renderer-transcript <renderer-jsonl> --tray-enabled <enabled|disabled> --output <probe>.performance.json
```

`--renderer-transcript` 是必需参数，目标必须是普通非符号链接文件。每一行必须是一个没有重复字段的 JSON 对象，字段集合必须与对应 `kind` 精确相等，`sequence` 必须从 1 开始逐条连续递增：

Raw evidence 必须同时给出精确字段 `rendererTranscriptScope: "interaction-session"`。该值声明 transcript 只对应一个专用交互会话：从该会话启动与模块加载开始，贯穿至少 20 轮导航/内存循环、空闲观测，以及启用托盘时的隐藏/显示观测。1 次预热与恰好 5 次冷启动是独立进程启动，继续由外部秒表原始值和每次启动前的 window-state 重置证据证明；不得把单个 transcript 描述为覆盖每一次冷启动。

- 首条固定为 `renderer-capabilities(longTaskSupported,timingSource,sequence)`；`true` 只配 `performance-observer`，`false` 只配 `animation-frame-gap`，其 `timingSource` 必须与 raw evidence 一致。
- 全文件恰好一条 `main-window-ready(wallTimeMs,monotonicTimeMs,sequence)`。
- `interaction(target,result,durationMs,sequence)` 只允许 `navigation-dashboard → dashboard-page`、`navigation-monitor → monitor-page`、`navigation-settings → settings-page` 三组固定配对。
- `renderer-blocking-interval(timingSource,startTimeMs,durationMs,sequence)` 的来源必须与首条能力记录一致，且 `durationMs >= 50`。
- 最后一条必须是唯一的 `session-finalized(sequence,recordCount)`；其 `sequence` 等于总行数、`recordCount` 等于此前记录数，后面不得再有记录。

Helper 输出只保存 transcript 的 basename 和真实字节 SHA-256，不保存或回显绝对路径。文件缺失、不可读、符号链接、非法 UTF-8/JSON、未知或缺失字段、断序、非法配对、缺少结束握手或计数不一致全部属于不可豁免完整性失败。

## 原始观测输入

下面是结构示例，不是已执行验证结果。数值数组保存逐次原始观测，helper 才负责计算 median、maximum 和 nearest-rank p95。

```json
{
  "schemaVersion": 3,
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
  "rendererTranscriptScope": "interaction-session",
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
      "sequence": 3,
      "target": "navigation-settings",
      "result": "settings-page",
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

实际 `interactions` 至少 20 项。每项必须包含 transcript 中同一记录的 `sequence`、`target`、`result`、`durationMs`，以及固定为 `true` 的 `observableResult`；四个 transcript 字段必须按顺序逐项精确相等。`longTasksMs` 也必须与 transcript 中全部 `renderer-blocking-interval.durationMs` 按顺序精确相等。交互字段不得包含用户名、业务载荷或绝对路径。`idleCpuPercentOfOneLogicalCore` 和适用的 `hiddenTrayCpuPercentOfOneLogicalCore` 是每个采样时点对 Tauri、WebView 和受管子进程整棵树求和后的原始值，至少各 5 项且覆盖不少于 30 秒。`rendererTranscriptScope` 缺失或不是精确的 `interaction-session` 属于不可豁免完整性失败。

`rendererTimingSource` 只允许 `performance-observer` 或 `animation-frame-gap`。优先使用浏览器 `PerformanceObserver` 的原生 `longtask` entry；只有当前 WKWebView 不支持该 entry 时，才允许在可见页面启用测试专用的连续 `requestAnimationFrame` 帧间隔观测，并记录 `animation-frame-gap`。回退观测不得在隐藏或托盘状态运行，避免把系统节流误判为 Long Task。`longTasksMs` 在前一种来源下只记录不短于 50 ms 的 Long Task，在回退来源下只记录不短于 50 ms 的可见页面帧间隔，没有时使用空数组。来源缺失、值无效或两种观测都不可用属于不可豁免完整性失败。

`trayEnabled: false` 时必须省略两个 `hiddenTray*` 字段；启用时二者都必需。任何观测不可用、部分进程树、探针字节变化或未回收进程都必须使用 `false`，helper 会失败关闭。其中 `wholeProcessTree`、`probeBytesUnmodified`、`allProcessesRecovered` 任一不为 `true` 都是不可豁免的证据完整性失败；普通性能指标超限在三项完整性标记仍为 `true` 且窗口原状态恢复成立时，仍可进入后续性能 waiver 流程。

`windowStateIsolation` 不得包含状态文件路径、绝对目录、原始字节或其他文件内容。`targetResolved: true` 表示调用方已经从应用 identity 和当前平台应用数据目录解析出唯一、非符号链接且不越界的 window-state 目标；`snapshotStoredOutsideAppData: true` 表示原状态快照位于应用数据目录之外的独立临时目录；`originalSnapshotVerified: true` 表示快照与执行前原字节或原缺席一致。`fingerprintAlgorithm` 固定为 `hmac-sha256-ephemeral-key`：每轮生成只存在于临时测量进程的随机密钥，以 HMAC-SHA256 对存在状态的字节生成相等性指纹，恢复复核后销毁密钥；证据不得保存密钥或使用可对低熵窗口几何离线枚举的裸 SHA-256。

`original`、`seed`、每个 reset 的 `observed` 与 `restoration.observed` 只允许使用 `kind: present | absent`；`present` 必须同时给出 64 位小写 `fingerprint`，`absent` 必须省略 `fingerprint`。`preLaunchResets` 必须按顺序逐项包含 `warmupRuns` 个 `phase: warmup` 和恰好 5 个 `phase: cold-start`，各 phase 的 `run` 从 1 连续递增，且每一项的 `observed` 与唯一 `seed` 完全相同。`restoration` 必须有 `verified: true`，其 `observed` 必须与 `original` 的 kind/fingerprint 完全相同，才能证明原字节或原缺席已经恢复。缺项、额外项、乱序、重复、指纹不匹配或恢复未验证都会失败关闭。helper 只把白名单化后的 `windowStateIsolation` 写入输出；误填的路径、字节或其他字段既导致失败，也不会被固化进失败证据。

## Helper 输出与最终 manifest

Helper 输出一个 `schemaVersion: 3`、`kind: gui-release-performance` 的 JSON 对象，保留白名单化后的 `observations`、计算后的 `metrics`、固定 `thresholds`、探针绑定、`rendererTranscript.scope`/`rendererTranscript.file`/`rendererTranscript.sha256` 和 `failures`，并增加 `windowStateRecoveryVerified`、`waiverAllowed` 与 `nonWaivableFailures`。只有窗口原状态恢复验证通过时 `windowStateRecoveryVerified` 才为 `true`。只有冷启动、交互、Long Task/可见页面帧间隔、CPU、RSS 或 RSS 增长的明确阈值超限属于可申请豁免的纯性能失败；结构、探针或源码绑定、transcript 绑定、观测可用性、采样器、样本数量或数值完整性、未知字段、window-state 重置或恢复等其他错误全部进入 `nonWaivableFailures`。`waiverAllowed` 只在全部失败都是上述纯性能阈值超限、窗口恢复成立且 `nonWaivableFailures` 为空时为 `true`。退出码 0 只对应 `status: passed`；退出码 1 对应已原子保存且 `waiverAllowed: true` 的纯性能阈值失败；退出码 3 对应已原子保存且包含至少一项不可豁免完整性失败、`waiverAllowed: false` 的失败；退出码 2 只表示 helper 无法安全写出证据。

完整打包 manifest 的 `performanceEvidence` 使用结构化对象而不是安装容器摘要：

```json
{
  "performanceSelection": "enabled",
  "performanceStatus": "passed",
  "performanceThresholdProfile": "gui-release-v1",
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

打包前 staged unsigned runtime 必须与探针逐字节相同。`binding` 只允许 `byte-identical` 或 `verified-signing-transition`：前者要求最终包内 runtime 也逐字节相同；后者必须另外保留签名前相同摘要、签名后 runtime 摘要和签名验证。最终验收重新定位包内 runtime 核对。容器 SHA-256 永远不能填入 `performanceProbeSha256`。

`waived` 只能引用 `status: failed` 且 `waiverAllowed: true` 的原始输出，并增加非空原因、确认时间、用户确认摘要、已尝试修复和剩余风险；不得删除失败数组、覆盖失败证据或把 helper 输出改成 `passed`。只要存在任一结构、绑定、观测、采样、未知字段或 window-state 完整性错误，或者 `windowStateRecoveryVerified: false`、`waiverAllowed: false`，均禁止创建性能 waiver；必须先修复完整性失败并重新运行 helper。

当次选择为 `disabled` 且无产品/渠道硬要求时，构建流程跳过整个 no-bundle 性能探针与 helper，最终 manifest 记录 `performanceSelection: disabled`、`performanceStatus: Not run`、非空原因与剩余风险，并且不得生成 `performanceProbe`、`performanceProbeSha256`、`performanceEvidence`、`performanceThresholdProfile`、`performanceWaiver` 或 `performanceRuntimeBinding`。`Not run` 不能覆盖同一候选已经产生的真实 `failed` 证据；已有失败只能修复后重建，或保留失败证据并走 `waived`。
