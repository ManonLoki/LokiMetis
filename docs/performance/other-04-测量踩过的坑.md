# OTHER-04 测量时踩过的坑（隔离、多实例、单位）

这些都是我们实际踩过的，写下来是为了让后来的人少踩。

## 1. 设置 `HOME` 不能隔离应用数据

我们最初用 `HOME=/tmp/x` 启动应用做测量，以为它会读写临时目录。其实：

- macOS 上应用用 `NSHomeDirectory()` 求用户目录，**不看环境变量 `HOME`**；
- 结果测试实例读写了**真实的**数据目录和 Hook 配置，并和另一个正在运行的实例**互相纠正 Hook 配置**（每 5 秒一次），
  产生了大量虚假的 CPU 和 fcntl 采样。

### 正确做法

我们加了一个**默认关闭**的 Cargo 特性 `isolated-measurement`：

```
cargo build --release -p loki_metris_gui --features isolated-measurement
LOKI_METRIS_MEASURE_HOME=/private/tmp/lmm HOME=/private/tmp/lmm ./loki_metris_gui
```

该特性只重定向 `current_user_home()` 返回的目录；上面的历史实验还设置了 HOME，以隔离 macOS GUI 偏好和单实例目录。显式来源环境变量、已保存的自定义目录、宿主资源及系统集成不由该变量统一隔离。常规打包默认不带此特性。

## 2. 路径别太长

本地 socket 路径在 macOS 上最长约 104 字符。临时目录太长，单实例锁会失败（输出 `single_instance_unavailable`）。
用 `/private/tmp/lmm` 这种短路径。

## 3. 两个实例同时跑，会互相影响

除了 Hook 配置，还有单实例锁、索引数据库、通知状态文件。测量时保证只有一个实例用这份数据。

## 4. CPU 单位别搞错

`proc_pid_rusage` 里的 CPU 时间是 **Mach ticks**，不是纳秒。要用 `mach_timebase_info` 的 `numer/denom` 换算
（本机是 125/3）。当纳秒用会得到 1/40 的错误结果。用 `ps -o time=` 的累计 CPU 秒最直观。

## 5. 先对整数累计值求差，再转换

大累计值直接做浮点减法会丢精度。

## 6. 跨框架比较要包含全部进程

WebView 的辅助进程（WebKit helper）可能归 launchd 管，按父进程递归会漏。要用应用的 coalition 获取完整集合，
并检查 PID、启动时间、可执行路径；成员变化就整轮作废。

## 7. 前台和遮挡

窗口不在前台时可能根本没在绘制，见 [UI-04](ui-04-测卡顿别被后台窗口骗.md)。

## 8. 构建指纹

记录：完整 commit、是否 dirty、构建 profile、可执行文件的 SHA-256、实际 PID。
"commit 号"不能代表一个 dirty 构建的全部输入。

## 9. 一次采样的陷阱

- 5 秒采样没看到某函数，不等于它不存在；
- `sample` 的点数不能当 CPU 占比；
- 采样本身有开销，诊断用就好。

## 相关代码

- `loki_metris_gui/src/backend/local_index/user_paths.rs`（`isolated-measurement` 特性）
- 采样脚本：`scripts/macos_resource_sample.py`
- 详情：[性能优化参考 §3](../PERFORMANCE_OPTIMIZATION.md)、[案例 5.16](案例-1-写入图库与动画缓存.md)、[案例 5.23](案例-2-帧调度索引与内存.md)、[案例 5.34](案例-2-帧调度索引与内存.md)
