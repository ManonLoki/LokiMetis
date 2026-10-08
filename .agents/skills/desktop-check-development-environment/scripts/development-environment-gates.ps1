#Requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$CheckOnly,
    [string[]]$Interfaces = @(),
    [ValidateSet("tauri", "gpui")]
    [string]$GuiFramework = "tauri"
)

$ErrorActionPreference = "Stop"
# MSRV 的唯一事实来源见 docs/RUST_CLI_TEMPLATE.md；修改此值时必须同步更新 development-environment-gates.sh。
$MinimumRustMajor = 1
$MinimumRustMinor = 98
$MinimumRustPatch = 1
$NodeRequirement = ">=24.21.0"
$PnpmRequirement = ">=12.4.1"
$PnpmInstallRequirement = "pnpm@>=12.4.1"
$GitRequirement = ">=2.36.0"
$PnpmRegistry = "https://registry.npmjs.org/"
$TestMode = $env:AFH_TEST_MODE -eq "1"
$ProbePath = if ($env:AFH_PREREQ_PATH) { $env:AFH_PREREQ_PATH } else { $env:PATH }
$RustChange = "existing"
$GitChange = "existing"
$NodeChange = "existing"
$PnpmChange = "existing"
$MsvcChange = "existing"
$CargoBin = $null
$GitBin = $null
$NodeBin = $null
$PnpmBin = $null
$SelectedNodeVersion = $null
$NormalizedInterfaces = @($Interfaces | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim().ToUpperInvariant() })
$UnsupportedInterfaces = @($NormalizedInterfaces | Where-Object { $_ -and $_ -notin @("CLI", "TUI", "MCP", "GUI") })
if ($UnsupportedInterfaces.Count -gt 0) {
    [Console]::Error.WriteLine("不支持的接口：$($UnsupportedInterfaces -join ',')")
    exit 2
}
$PnpmRequired = ($NormalizedInterfaces -contains "GUI")
$TemporaryDirectories = [System.Collections.Generic.List[string]]::new()

# 使用稳定退出码结束门禁，调用方可以据此区分具体失败阶段。
function Stop-Gate {
    param([int]$Code, [string]$Message)
    [Console]::Error.WriteLine("错误：$Message")
    exit $Code
}

# 任何单一路径根都不得夹带 PATH 分隔符；否则后续持久化会把它拆成未授权的额外目录。
function Assert-SinglePathRootValue {
    param([string]$Value, [string]$Label)
    if ([string]::IsNullOrWhiteSpace($Value)) { return }
    $expanded = [Environment]::ExpandEnvironmentVariables($Value)
    if ($Value.Contains([string][IO.Path]::PathSeparator) -or $expanded.Contains([string][IO.Path]::PathSeparator)) {
        Stop-Gate 24 "$Label 不能包含 PATH 分隔符 $([IO.Path]::PathSeparator)：$Value"
    }
    if ($expanded -match '^[A-Za-z]:(?![\\/])' -or $expanded -match '^[\\/](?![\\/])' -or
        -not [IO.Path]::IsPathRooted($expanded)) {
        Stop-Gate 24 "$Label 必须是完整绝对路径，不能使用 drive-relative 或 root-relative 形式：$Value"
    }
}

# 在给定 PATH 中使用 PowerShell 自身的真实命令解析规则；不自行猜测扩展名优先级。
function Resolve-PathCommand {
    param([string]$Name, [string]$PathValue)
    if ([string]::IsNullOrWhiteSpace($PathValue)) { return $null }
    $originalPath = $env:PATH
    try {
        $env:PATH = $PathValue
        $resolved = Get-Command -Name $Name -CommandType Application,ExternalScript -All -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if (-not $resolved -or [string]::IsNullOrWhiteSpace([string]$resolved.Path)) { return $null }
        return [IO.Path]::GetFullPath([string]$resolved.Path)
    } finally {
        $env:PATH = $originalPath
    }
}

# 只在门禁探测路径中解析工具，便于隔离机器已有环境并验证缺失分支。
function Resolve-GateCommand {
    param([string]$Name)
    return Resolve-PathCommand $Name $script:ProbePath
}

# 下载官方 HTTPS 制品；file URL 仅供显式开启的隔离测试镜像使用。
function Get-OfficialFile {
    param([string]$Uri, [string]$Destination)
    if ($Uri.StartsWith("https://", [StringComparison]::OrdinalIgnoreCase)) {
        Invoke-WebRequest -UseBasicParsing -Uri $Uri -OutFile $Destination
        return
    }
    if ($Uri.StartsWith("file://", [StringComparison]::OrdinalIgnoreCase) -and $TestMode -and $env:AFH_ALLOW_FILE_URLS -eq "1") {
        Copy-Item -LiteralPath ([Uri]$Uri).LocalPath -Destination $Destination
        return
    }
    Stop-Gate 24 "不支持的下载 URL 协议：$Uri"
}

# 建立并登记本进程专用临时目录，finally 只清理这些已知目标。
function New-GateTemporaryDirectory {
    $directory = Join-Path ([IO.Path]::GetTempPath()) ("agent-first-gate-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $directory | Out-Null
    $script:TemporaryDirectories.Add($directory)
    return $directory
}

# 直接调用用户 PATH 中的 Rust 工具；不注入或改写 CARGO_HOME/RUSTUP_HOME。
function Invoke-RustCommand {
    param(
        [string]$Path,
        [string[]]$ArgumentList = @(),
        [switch]$SuppressStderr
    )
    if ($SuppressStderr) {
        $output = (& $Path @ArgumentList 2>$null)
    } else {
        $output = (& $Path @ArgumentList)
    }
    $script:RustCommandExitCode = $LASTEXITCODE
    return $output
}

# 验证 rustup/rustc/cargo 属于同一可解释稳定工具链，并以 rustc -vV 锁定 release 与 host。
function Test-RustVersion {
    param([string]$RustupPath, [string]$RustcPath, [string]$CargoPath)
    $rustupText = Invoke-RustCommand $RustupPath @("--version") -SuppressStderr
    if ($script:RustCommandExitCode -ne 0) { Stop-Gate 21 "rustup 探测失败" }
    if ($rustupText -notmatch '^rustup (\d+)\.(\d+)\.(\d+)(?:\s|$)') {
        Stop-Gate 21 "现有 rustup 不是可识别的稳定发布版：$rustupText"
    }
    $rustText = Invoke-RustCommand $RustcPath @("--version") -SuppressStderr
    if ($script:RustCommandExitCode -ne 0) { Stop-Gate 21 "rustc 探测失败" }
    $cargoText = Invoke-RustCommand $CargoPath @("--version") -SuppressStderr
    if ($script:RustCommandExitCode -ne 0) { Stop-Gate 21 "cargo 探测失败" }
    if ($rustText -notmatch '^rustc (\d+)\.(\d+)\.(\d+)(?:\s|$)') {
        Stop-Gate 21 "现有 Rust 工具链不是可识别的稳定发布版：$rustText"
    }
    $rustMajor = [int]$Matches[1]
    $rustMinor = [int]$Matches[2]
    $rustPatch = [int]$Matches[3]
    $rustRelease = "$($Matches[1]).$($Matches[2]).$($Matches[3])"
    if ($cargoText -notmatch '^cargo (\d+)\.(\d+)\.(\d+)(?:\s|$)') {
        Stop-Gate 21 "现有 Cargo 不是可识别的稳定发布版：$cargoText"
    }
    $cargoMajor = [int]$Matches[1]
    $cargoMinor = [int]$Matches[2]
    $cargoPatch = [int]$Matches[3]
    if ($cargoMajor -ne $rustMajor -or $cargoMinor -ne $rustMinor) {
        Stop-Gate 21 "rustc 与 cargo 不属于同一 stable 工具链"
    }
    $verboseText = ((Invoke-RustCommand $RustcPath @("-vV") -SuppressStderr) -join "`n")
    if ($script:RustCommandExitCode -ne 0) { Stop-Gate 21 "rustc -vV 探测失败" }
    $releaseLine = @($verboseText -split "`n" | Where-Object { $_ -match '^release:\s*(\S+)\s*$' })
    $hostLine = @($verboseText -split "`n" | Where-Object { $_ -match '^host:\s*(\S+)\s*$' })
    if ($releaseLine.Count -ne 1 -or $releaseLine[0] -notmatch "^release:\s*$([Regex]::Escape($rustRelease))\s*$") {
        Stop-Gate 21 "rustc -vV 的 release 与 rustc --version 不一致"
    }
    if ($hostLine.Count -ne 1 -or $hostLine[0] -notmatch '^host:\s*(\S+)\s*$') {
        Stop-Gate 21 "rustc -vV 未返回唯一可识别 host"
    }
    $hostLine[0] -match '^host:\s*(\S+)\s*$' | Out-Null
    $script:RustupVersion = $rustupText
    $script:RustVersion = $rustText
    $script:CargoVersion = $cargoText
    $script:RustHost = $Matches[1]
    if ($rustMajor -lt $MinimumRustMajor -or
        ($rustMajor -eq $MinimumRustMajor -and ($rustMinor -lt $MinimumRustMinor -or
            ($rustMinor -eq $MinimumRustMinor -and $rustPatch -lt $MinimumRustPatch))) -or
        $cargoMajor -lt $MinimumRustMajor -or
        ($cargoMajor -eq $MinimumRustMajor -and ($cargoMinor -lt $MinimumRustMinor -or
            ($cargoMinor -eq $MinimumRustMinor -and $cargoPatch -lt $MinimumRustPatch)))) {
        return "upgrade-required"
    }
    return "passed"
}

# 所有可改变探测路径、下载来源或安全校验的覆盖只服务隔离测试，生产环境必须失败关闭。
$TestOverrideNames = @(
    "AFH_PREREQ_PATH",
    "AFH_ALLOW_FILE_URLS",
    "AFH_RUSTUP_DIST_BASE",
    "AFH_NODE_DIST_BASE",
    "AFH_VS_BUILDTOOLS_URL",
    "AFH_SKIP_AUTHENTICODE",
    "AFH_SKIP_PERSIST_PATH",
    "AFH_NODE_HOME",
    "AFH_PNPM_HOME",
    "AFH_MANAGED_CARGO_HOME",
    "AFH_MANAGED_RUSTUP_HOME",
    "AFH_PNPM_REGISTRY",
    "AFH_TEST_USER_PATH_FILE",
    "AFH_TEST_MACHINE_PATH",
    "AFH_TEST_USER_CARGO_HOME",
    "AFH_TEST_USER_RUSTUP_HOME",
    "AFH_TEST_USER_PROFILE_ROOT",
    "AFH_TEST_LOCAL_APPDATA_ROOT",
    "AFH_TEST_ROAMING_APPDATA_ROOT"
)
if ($env:AFH_TEST_MODE -and -not $TestMode) {
    Stop-Gate 2 "AFH_TEST_MODE 只接受显式值 1"
}
foreach ($overrideName in $TestOverrideNames) {
    $overrideValue = [Environment]::GetEnvironmentVariable($overrideName, "Process")
    if (-not [string]::IsNullOrEmpty($overrideValue) -and -not $TestMode) {
        Stop-Gate 2 "测试覆盖 $overrideName 仅在 AFH_TEST_MODE=1 时允许"
    }
}
if ($TestMode -and $env:AFH_PNPM_REGISTRY) {
    $PnpmRegistry = $env:AFH_PNPM_REGISTRY
}
if (-not $PnpmRegistry.StartsWith("https://", [StringComparison]::OrdinalIgnoreCase)) {
    Stop-Gate 2 "pnpm registry 必须使用 HTTPS"
}

$UserProfileRoot = [Environment]::GetFolderPath("UserProfile")
$LocalAppDataRoot = [Environment]::GetFolderPath("LocalApplicationData")
$RoamingAppDataRoot = [Environment]::GetFolderPath("ApplicationData")
if ($TestMode -and $env:AFH_TEST_USER_PROFILE_ROOT) { $UserProfileRoot = $env:AFH_TEST_USER_PROFILE_ROOT }
if ($TestMode -and $env:AFH_TEST_LOCAL_APPDATA_ROOT) { $LocalAppDataRoot = $env:AFH_TEST_LOCAL_APPDATA_ROOT }
if ($TestMode -and $env:AFH_TEST_ROAMING_APPDATA_ROOT) { $RoamingAppDataRoot = $env:AFH_TEST_ROAMING_APPDATA_ROOT }
if (-not $UserProfileRoot -and $TestMode) { $UserProfileRoot = $env:USERPROFILE }
if (-not $LocalAppDataRoot -and $TestMode) { $LocalAppDataRoot = $env:LOCALAPPDATA }
if (-not $RoamingAppDataRoot -and $TestMode) { $RoamingAppDataRoot = $env:APPDATA }
if (-not $UserProfileRoot -or -not $LocalAppDataRoot -or -not $RoamingAppDataRoot) { Stop-Gate 24 "无法解析当前用户的标准安装根" }
foreach ($standardUserRoot in @($UserProfileRoot, $LocalAppDataRoot, $RoamingAppDataRoot)) {
    Assert-SinglePathRootValue $standardUserRoot "标准当前用户安装根"
}

# 读取真正会被新登录会话继承的标准用户环境；隔离测试使用显式重定向值。
function Get-PersistedUserEnvironmentValue {
    param([ValidateSet("CARGO_HOME", "RUSTUP_HOME")][string]$Name)
    if ($TestMode) {
        return [Environment]::GetEnvironmentVariable("AFH_TEST_USER_$Name", "Process")
    }
    return [Environment]::GetEnvironmentVariable($Name, "User")
}

$script:DefaultCargoHome = Join-Path $UserProfileRoot ".cargo"
$script:DefaultRustupHome = Join-Path $UserProfileRoot ".rustup"
$script:ProcessCargoHome = [Environment]::GetEnvironmentVariable("CARGO_HOME", "Process")
$script:ProcessRustupHome = [Environment]::GetEnvironmentVariable("RUSTUP_HOME", "Process")
$script:PersistedCargoHome = Get-PersistedUserEnvironmentValue "CARGO_HOME"
$script:PersistedRustupHome = Get-PersistedUserEnvironmentValue "RUSTUP_HOME"
$script:ManagedCargoHome = if ($script:ProcessCargoHome) {
    $script:ProcessCargoHome
} elseif ($script:PersistedCargoHome) {
    $script:PersistedCargoHome
} else {
    $script:DefaultCargoHome
}
$script:ManagedRustupHome = if ($script:ProcessRustupHome) {
    $script:ProcessRustupHome
} elseif ($script:PersistedRustupHome) {
    $script:PersistedRustupHome
} else {
    $script:DefaultRustupHome
}
$script:ManagedNodeHome = Join-Path $LocalAppDataRoot "Programs\nodejs"
$script:ManagedPnpmHome = Join-Path $RoamingAppDataRoot "npm"
if ($TestMode) {
    if ($env:AFH_MANAGED_CARGO_HOME) { $script:ManagedCargoHome = $env:AFH_MANAGED_CARGO_HOME }
    if ($env:AFH_MANAGED_RUSTUP_HOME) { $script:ManagedRustupHome = $env:AFH_MANAGED_RUSTUP_HOME }
    if ($env:AFH_NODE_HOME) { $script:ManagedNodeHome = $env:AFH_NODE_HOME }
    if ($env:AFH_PNPM_HOME) { $script:ManagedPnpmHome = $env:AFH_PNPM_HOME }
}
foreach ($userInstallRoot in @($script:ManagedCargoHome, $script:ManagedRustupHome, $script:ManagedNodeHome, $script:ManagedPnpmHome)) {
    Assert-SinglePathRootValue $userInstallRoot "当前用户安装根"
    if (-not [IO.Path]::IsPathRooted($userInstallRoot)) { Stop-Gate 24 "当前用户安装根必须是绝对路径：$userInstallRoot" }
}

# 逐级拒绝 reparse point 与非目录组件；生产安装根必须留在对应的当前用户目录内。
function Assert-ManagedDirectoryPath {
    param([string]$Path, [string]$TrustedRoot, [string]$Label)
    $fullPath = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    $trustedPath = [IO.Path]::GetFullPath($TrustedRoot).TrimEnd('\', '/')
    $trustedPrefix = $trustedPath + [IO.Path]::DirectorySeparatorChar
    if ($fullPath.StartsWith($trustedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        $walkRoot = $trustedPath
        $relative = $fullPath.Substring($trustedPrefix.Length)
    } elseif ($TestMode) {
        $walkRoot = [IO.Path]::GetPathRoot($fullPath)
        $relative = $fullPath.Substring($walkRoot.Length)
    } else {
        Stop-Gate 24 "$Label 必须位于对应的当前用户目录内：$fullPath"
    }
    $cursor = $walkRoot
    foreach ($component in ($relative -split '[\\/]')) {
        if (-not $component) { continue }
        if ($component -in @('.', '..')) { Stop-Gate 24 "$Label 包含不安全路径组件：$fullPath" }
        $cursor = Join-Path $cursor $component
        if (-not (Test-Path -LiteralPath $cursor)) { continue }
        $item = Get-Item -LiteralPath $cursor -Force
        if (-not $item.PSIsContainer) { Stop-Gate 24 "$Label 的路径组件不是普通目录：$cursor" }
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            Stop-Gate 24 "$Label 的路径组件不能是 reparse point：$cursor"
        }
    }
}

# 使用 Windows 文件句柄取得已存在路径的最终规范目标；目录与普通文件都允许只读解析。
function Get-FinalExistingPath {
    param([string]$Path, [int]$ErrorCode, [string]$Label)
    if (-not ("AgentFirstHarness.FinalPath" -as [type])) {
        try {
            Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace AgentFirstHarness {
    public static class FinalPath {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern SafeFileHandle CreateFile(
            string fileName, uint desiredAccess, uint shareMode, IntPtr securityAttributes,
            uint creationDisposition, uint flagsAndAttributes, IntPtr templateFile);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern uint GetFinalPathNameByHandle(
            SafeFileHandle file, StringBuilder path, uint pathLength, uint flags);

        public static string Resolve(string path) {
            const uint shareAll = 0x00000001 | 0x00000002 | 0x00000004;
            const uint openExisting = 3;
            const uint backupSemantics = 0x02000000;
            using (SafeFileHandle handle = CreateFile(
                path, 0, shareAll, IntPtr.Zero, openExisting, backupSemantics, IntPtr.Zero)) {
                if (handle.IsInvalid) {
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                }
                StringBuilder buffer = new StringBuilder(32768);
                uint length = GetFinalPathNameByHandle(handle, buffer, (uint)buffer.Capacity, 0);
                if (length == 0 || length >= buffer.Capacity) {
                    throw new Win32Exception(Marshal.GetLastWin32Error());
                }
                string result = buffer.ToString();
                if (result.StartsWith(@"\\?\UNC\", StringComparison.OrdinalIgnoreCase)) {
                    result = @"\\" + result.Substring(8);
                } else if (result.StartsWith(@"\\?\", StringComparison.OrdinalIgnoreCase)) {
                    result = result.Substring(4);
                }
                return Path.GetFullPath(result).TrimEnd(
                    Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
            }
        }
    }
}
'@ | Out-Null
        } catch {
            Stop-Gate $ErrorCode "$Label 无法初始化最终路径解析器；已在执行或覆盖 wrapper 前停止"
        }
    }
    try {
        return [AgentFirstHarness.FinalPath]::Resolve($Path)
    } catch {
        Stop-Gate $ErrorCode "$Label 无法解析最终文件目标；已在执行或覆盖 wrapper 前停止：$Path"
    }
}

function Test-PathWithinDirectory {
    param([string]$Path, [string]$Directory)
    $fullPath = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    $fullDirectory = [IO.Path]::GetFullPath($Directory).TrimEnd('\', '/')
    $prefix = $fullDirectory + [IO.Path]::DirectorySeparatorChar
    return $fullPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)
}

# wrapper 必须是指定目录的直接普通文件，并且 Windows 最终解析目标仍在该目录内。
function Assert-ManagedCommandWrapper {
    param(
        [string]$Path,
        [string]$ManagedDirectory,
        [int]$ErrorCode,
        [string]$Label
    )
    $managed = [IO.Path]::GetFullPath($ManagedDirectory).TrimEnd('\', '/')
    $candidate = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    $managedItem = Get-Item -LiteralPath $managed -Force -ErrorAction SilentlyContinue
    if (-not $managedItem -or -not $managedItem.PSIsContainer -or
        ($managedItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        Stop-Gate $ErrorCode "$Label 的受管目录必须是非 reparse point 的普通目录：$managed"
    }
    $parent = [IO.Path]::GetFullPath((Split-Path -Parent $candidate)).TrimEnd('\', '/')
    if (-not [string]::Equals($parent, $managed, [StringComparison]::OrdinalIgnoreCase)) {
        Stop-Gate $ErrorCode "$Label 不在受管目录内；已在执行或覆盖 wrapper 前停止：$candidate"
    }
    $item = Get-Item -LiteralPath $candidate -Force -ErrorAction SilentlyContinue
    if (-not $item -or $item.PSIsContainer -or
        ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        Stop-Gate $ErrorCode "$Label 必须是非 reparse point 的普通文件；已在执行或覆盖 wrapper 前停止：$candidate"
    }
    $finalManaged = Get-FinalExistingPath $managed $ErrorCode "$Label 的受管目录"
    $finalCandidate = Get-FinalExistingPath $candidate $ErrorCode $Label
    if (-not (Test-PathWithinDirectory $finalCandidate $finalManaged)) {
        Stop-Gate $ErrorCode "$Label 的最终文件目标越出受管目录；已在执行或覆盖 wrapper 前停止：$finalCandidate"
    }
    return $candidate
}

function Resolve-ManagedCommandWrapper {
    param(
        [string]$Name,
        [string]$ManagedDirectory,
        [int]$ErrorCode,
        [string]$Label
    )
    $resolved = Resolve-PathCommand $Name $ManagedDirectory
    if (-not $resolved) { return $null }
    return Assert-ManagedCommandWrapper $resolved $ManagedDirectory $ErrorCode $Label
}

# 自定义 Rust home 只有在 User 作用域中持久存在且与当前进程一致时才可用于安装。
# 这样不会把一次性 Process 值误当作新登录会话可用的全局配置，也不会擅自持久化用户临时选择。
function Assert-DurableRustHomes {
    foreach ($spec in @(
        @("CARGO_HOME", $script:ManagedCargoHome, $script:DefaultCargoHome, $script:ProcessCargoHome, $script:PersistedCargoHome, $env:AFH_MANAGED_CARGO_HOME),
        @("RUSTUP_HOME", $script:ManagedRustupHome, $script:DefaultRustupHome, $script:ProcessRustupHome, $script:PersistedRustupHome, $env:AFH_MANAGED_RUSTUP_HOME)
    )) {
        $name = [string]$spec[0]
        $effective = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables([string]$spec[1])).TrimEnd('\', '/')
        $default = [IO.Path]::GetFullPath([Environment]::ExpandEnvironmentVariables([string]$spec[2])).TrimEnd('\', '/')
        Assert-SinglePathRootValue $effective "$name 安装位置"
        Assert-SinglePathRootValue $default "$name 默认安装位置"
        $processValue = [string]$spec[3]
        $persistedValue = [string]$spec[4]
        $testManagedOverride = [string]$spec[5]
        $process = if ([string]::IsNullOrWhiteSpace($processValue)) { "" } else {
            $expandedProcess = [Environment]::ExpandEnvironmentVariables($processValue)
            Assert-SinglePathRootValue $expandedProcess "$name 当前进程值"
            [IO.Path]::GetFullPath($expandedProcess).TrimEnd('\', '/')
        }
        $persisted = if ([string]::IsNullOrWhiteSpace($persistedValue)) { "" } else {
            $expandedPersisted = [Environment]::ExpandEnvironmentVariables($persistedValue)
            Assert-SinglePathRootValue $expandedPersisted "$name User 作用域持久值"
            [IO.Path]::GetFullPath($expandedPersisted).TrimEnd('\', '/')
        }

        if ($TestMode -and -not [string]::IsNullOrWhiteSpace($testManagedOverride)) {
            $persisted = $effective
        } elseif ($process) {
            if ($persisted -and -not [string]::Equals($process, $persisted, [StringComparison]::OrdinalIgnoreCase)) {
                Stop-Gate 24 "$name 的当前进程值与 User 作用域持久值不一致；拒绝把工具安装到仅当前会话可用的位置"
            }
            if (-not $persisted -and -not [string]::Equals($process, $default, [StringComparison]::OrdinalIgnoreCase)) {
                Stop-Gate 24 "$name 只存在于当前进程；请先把该标准变量配置到 User 作用域，或移除它以使用默认用户目录"
            }
        }
        if ($persisted -and -not [string]::Equals($effective, $persisted, [StringComparison]::OrdinalIgnoreCase)) {
            Stop-Gate 24 "$name 的安装位置与 User 作用域持久值不一致"
        }
        if (-not $process -and -not $persisted -and
            -not [string]::Equals($effective, $default, [StringComparison]::OrdinalIgnoreCase)) {
            Stop-Gate 24 "$name 的非默认安装位置没有 User 作用域持久配置"
        }
        [Environment]::SetEnvironmentVariable($name, $effective, "Process")
    }
}

# 预检后逐级建立普通目录，避免 -Force 追随预置 junction/symlink。
function Initialize-ManagedDirectoryPath {
    param([string]$Path, [string]$TrustedRoot, [string]$Label)
    Assert-ManagedDirectoryPath $Path $TrustedRoot $Label
    $fullPath = [IO.Path]::GetFullPath($Path).TrimEnd('\', '/')
    $trustedPath = [IO.Path]::GetFullPath($TrustedRoot).TrimEnd('\', '/')
    $trustedPrefix = $trustedPath + [IO.Path]::DirectorySeparatorChar
    if ($fullPath.StartsWith($trustedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        $walkRoot = $trustedPath
        $relative = $fullPath.Substring($trustedPrefix.Length)
    } else {
        $rootPrefix = [IO.Path]::GetPathRoot($fullPath)
        $walkRoot = $rootPrefix
        $relative = $fullPath.Substring($rootPrefix.Length)
    }
    $cursor = $walkRoot
    foreach ($component in ($relative -split '[\\/]')) {
        if (-not $component) { continue }
        $cursor = Join-Path $cursor $component
        if (-not (Test-Path -LiteralPath $cursor)) { New-Item -ItemType Directory -Path $cursor | Out-Null }
        $item = Get-Item -LiteralPath $cursor -Force
        if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            Stop-Gate 24 "$Label 在创建时发生变化：$cursor"
        }
    }
}

# 读取用户级 PATH；隔离测试把注册表写入重定向到临时文本文件。
function Get-PersistedUserPath {
    if ($TestMode -and $env:AFH_TEST_USER_PATH_FILE) {
        if (Test-Path -LiteralPath $env:AFH_TEST_USER_PATH_FILE -PathType Leaf) {
            $redirected = Get-Content -LiteralPath $env:AFH_TEST_USER_PATH_FILE -Raw
            if ($null -eq $redirected) { return "" }
            return $redirected.TrimEnd("`r", "`n")
        }
        return ""
    }
    $value = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($null -eq $value) { return "" }
    return $value
}

# 读取新 Windows 会话会优先消费的 Machine PATH；隔离测试使用显式重定向值。
function Get-PersistedMachinePath {
    if ($TestMode) { return [string]$env:AFH_TEST_MACHINE_PATH }
    $value = [Environment]::GetEnvironmentVariable("Path", "Machine")
    if ($null -eq $value) { return "" }
    return $value
}

# 写入用户级 PATH；测试重定向必须显式处于 AFH_TEST_MODE，避免触碰真实用户环境。
function Set-PersistedUserPath {
    param([string]$Value)
    if ($TestMode -and $env:AFH_TEST_USER_PATH_FILE) {
        $parent = Split-Path -Parent $env:AFH_TEST_USER_PATH_FILE
        if ($parent) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
        [IO.File]::WriteAllText($env:AFH_TEST_USER_PATH_FILE, $Value, [Text.UTF8Encoding]::new($false))
        return
    }
    [Environment]::SetEnvironmentVariable("Path", $Value, "User")
    if (-not ("AgentFirstHarness.NativeEnvironment" -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace AgentFirstHarness {
    public static class NativeEnvironment {
        [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern IntPtr SendMessageTimeout(
            IntPtr hWnd, uint msg, UIntPtr wParam, string lParam,
            uint flags, uint timeout, out UIntPtr result);
    }
}
'@
    }
    $broadcastResult = [UIntPtr]::Zero
    $broadcast = [AgentFirstHarness.NativeEnvironment]::SendMessageTimeout(
        [IntPtr]0xffff,
        0x001A,
        [UIntPtr]::Zero,
        "Environment",
        0x0002,
        5000,
        [ref]$broadcastResult
    )
    if ($broadcast -eq [IntPtr]::Zero) {
        Stop-Gate 24 "用户级 PATH 已写入，但无法通知新的 Windows 会话"
    }
}

# 清理 PATH 项并先展开环境变量；只保留可归一化的绝对路径，拒绝所有相对/cwd 形态。
function ConvertTo-PathEntryValue {
    param([string]$Entry)
    if ([string]::IsNullOrWhiteSpace($Entry)) { return "" }
    $trimmed = $Entry.Trim().Trim('"')
    if ([string]::IsNullOrWhiteSpace($trimmed)) { return "" }
    $expanded = [Environment]::ExpandEnvironmentVariables($trimmed)
    if ($expanded -match '^[A-Za-z]:(?![\\/])' -or $expanded -match '^[\\/](?![\\/])') { return "" }
    try {
        if (-not [IO.Path]::IsPathRooted($expanded)) { return "" }
        $full = [IO.Path]::GetFullPath($expanded)
        $root = [IO.Path]::GetPathRoot($full)
        if ($root -and $full.TrimEnd('\', '/') -eq $root.TrimEnd('\', '/')) {
            if (($expanded.EndsWith('\') -or $expanded.EndsWith('/')) -and
                -not ($root.EndsWith('\') -or $root.EndsWith('/'))) {
                return "$root$([IO.Path]::DirectorySeparatorChar)"
            }
            return $root
        }
        return $full.TrimEnd('\', '/')
    } catch {
        return ""
    }
}

# 归一化绝对 PATH 项用于大小写不敏感去重；无法展开或解析的项直接丢弃。
function Get-PathEntryKey {
    param([string]$Entry)
    $value = ConvertTo-PathEntryValue $Entry
    if (-not $value) { return "" }
    try {
        if ([IO.Path]::IsPathRooted($value)) {
            return ([IO.Path]::GetFullPath($value)).ToUpperInvariant()
        }
    } catch { return "" }
    return ""
}

# 使用与当前门禁相同的 PowerShell 规则解析持久 PATH，把 Machine shadow 前移到安装前。
function Resolve-PersistedPathCommand {
    param([string]$Name, [string]$PathValue)
    $normalizedPath = ConvertTo-NormalizedPathValue -Values @($PathValue)
    return Resolve-PathCommand $Name $normalizedPath
}

# Windows 新进程按 Machine、User 的固定顺序合并持久 PATH；预检与安装后新 PowerShell 使用同一顺序。
function Get-PersistedCombinedPath {
    return ConvertTo-NormalizedPathValue -Values @((Get-PersistedMachinePath), (Get-PersistedUserPath))
}

# 对本轮已通过且不会安装/升级的工具，必须在任何副作用前从持久 PATH 解析同一路径并复跑同一版本。
function Assert-PersistedCommandIdentity {
    param(
        [string]$Name,
        [string]$CurrentPath,
        [string]$ExpectedVersion,
        [string]$PersistentPath
    )
    $resolved = Resolve-PersistedPathCommand $Name $PersistentPath
    if (-not $resolved) {
        Stop-Gate 24 "持久 User/Machine PATH 无法解析当前已通过的 $Name；拒绝在仅当前进程可见的环境上继续安装"
    }
    $expectedPath = [IO.Path]::GetFullPath($CurrentPath)
    if (-not [string]::Equals($resolved, $expectedPath, [StringComparison]::OrdinalIgnoreCase)) {
        Stop-Gate 24 "持久 User/Machine PATH 解析的 $Name 与当前已通过工具路径不一致；已在下载和写入前停止：$resolved"
    }

    $originalPath = $env:PATH
    try {
        $env:PATH = $PersistentPath
        $actualVersion = (((& $resolved --version 2>$null) -join "`n")).Trim()
        $probeExitCode = $LASTEXITCODE
    } finally {
        $env:PATH = $originalPath
    }
    if ($probeExitCode -ne 0) {
        Stop-Gate 24 "持久 User/Machine PATH 中的 $Name 无法复跑版本探测"
    }
    if ($actualVersion -ne $ExpectedVersion) {
        Stop-Gate 24 "持久 User/Machine PATH 中的 $Name 版本与当前已通过版本不一致；已在下载和写入前停止"
    }
}

# 对本轮缺失或需要升级的工具，只允许持久 PATH 不可见，或解析到与当前探测相同的路径。
# 这样既能由后续安装补齐缺失的持久入口，也不会忽略持久 PATH 中已有的另一份（尤其是更高版本）工具。
function Assert-PendingPersistedCommandIdentity {
    param(
        [string]$Name,
        [string]$CurrentPath,
        [string]$PersistentPath
    )
    $resolved = Resolve-PersistedPathCommand $Name $PersistentPath
    if ([string]::IsNullOrWhiteSpace($CurrentPath)) {
        if ($resolved) {
            Stop-Gate 24 "当前探测未找到 $Name，但持久 User/Machine PATH 已解析到现有工具；拒绝忽略现有工具后另行安装：$resolved"
        }
        return
    }
    if (-not $resolved) { return }
    $expectedPath = [IO.Path]::GetFullPath($CurrentPath)
    if (-not [string]::Equals($resolved, $expectedPath, [StringComparison]::OrdinalIgnoreCase)) {
        Stop-Gate 24 "持久 User/Machine PATH 解析的待恢复 $Name 与当前探测工具路径不一致；拒绝安装或前置另一版本：$resolved"
    }
}

# 以 Machine PATH +（待前置目录 + 当前 User PATH）的真实新会话顺序，锁定所有不会在本轮改变的工具。
function Assert-ProjectedPersistedToolIdentities {
    param([string[]]$PrependedUserEntries)
    if ($TestMode -and $env:AFH_SKIP_PERSIST_PATH -eq "1") { return }
    $projectedUserValues = @($PrependedUserEntries) + @((Get-PersistedUserPath))
    $projectedUserPath = ConvertTo-NormalizedPathValue -Values $projectedUserValues
    $projectedPath = ConvertTo-NormalizedPathValue -Values @((Get-PersistedMachinePath), $projectedUserPath)

    $assertProjectedCommand = {
        param([string]$Name, [string]$CurrentPath, [string]$ExpectedVersion)
        $resolved = Resolve-PersistedPathCommand $Name $projectedPath
        $expected = [IO.Path]::GetFullPath($CurrentPath)
        if (-not $resolved -or -not [string]::Equals($resolved, $expected, [StringComparison]::OrdinalIgnoreCase)) {
            Stop-Gate 24 "按即将写入的 User PATH 顺序，$Name 将不再解析到当前已通过工具；已在写入前停止：$resolved"
        }
        Assert-PersistedCommandIdentity -Name $Name -CurrentPath $CurrentPath -ExpectedVersion $ExpectedVersion -PersistentPath $projectedPath
    }

    if ($gitState -eq "passed") {
        & $assertProjectedCommand "git" $git $GitVersion
    }
    if ($rustState -eq "passed") {
        & $assertProjectedCommand "rustup" $rustup $RustupVersion
        & $assertProjectedCommand "rustc" $rustc $RustVersion
        & $assertProjectedCommand "cargo" $cargo $CargoVersion
        Assert-PersistedRustHostIdentity -RustcPath $rustc -ExpectedVersion $RustVersion -ExpectedHost $RustHost -PersistentPath $projectedPath
    }
    if ($nodeState -eq "passed") {
        & $assertProjectedCommand "node" $node $NodeVersion
        & $assertProjectedCommand "npm" $npm $NpmVersion
    }
    if ($PnpmRequired -and $pnpmState -eq "passed") {
        & $assertProjectedCommand "pnpm" $pnpm $PnpmVersion
    }
}

# 即将加入 User PATH 的专用工具目录不得夹带其他环境门禁命令；这也覆盖相关工具自身仍待升级的组合安装。
function Assert-ProjectedPathDirectoryOwnership {
    param(
        [string]$Directory,
        [string[]]$AllowedNames,
        [string]$Label
    )
    if ($TestMode -and $env:AFH_SKIP_PERSIST_PATH -eq "1") { return }
    if (-not (Test-Path -LiteralPath $Directory -PathType Container)) { return }
    foreach ($name in @("git", "rustup", "rustc", "cargo", "node", "npm", "pnpm")) {
        if ($AllowedNames -contains $name) { continue }
        $resolved = Resolve-PathCommand $name $Directory
        if ($resolved) {
            Stop-Gate 24 "${Label}包含意外的 $name；该目录将在即将写入的 User PATH 中接管其他工具，已在该目录加入 PATH 前停止：$resolved"
        }
    }
}

# rustc 还必须从同一持久 PATH 复跑 -vV，并锁定 release 与 host。
function Assert-PersistedRustHostIdentity {
    param(
        [string]$RustcPath,
        [string]$ExpectedVersion,
        [string]$ExpectedHost,
        [string]$PersistentPath
    )
    if ($ExpectedVersion -notmatch '^rustc (\d+\.\d+\.\d+)(?:\s|$)') {
        Stop-Gate 24 "当前已通过的 rustc 版本无法提取 release"
    }
    $expectedRelease = $Matches[1]
    $originalPath = $env:PATH
    try {
        $env:PATH = $PersistentPath
        $verboseText = ((& $RustcPath -vV 2>$null) -join "`n")
        $probeExitCode = $LASTEXITCODE
    } finally {
        $env:PATH = $originalPath
    }
    if ($probeExitCode -ne 0) {
        Stop-Gate 24 "持久 User/Machine PATH 中的 rustc 无法复跑 -vV"
    }
    $releaseLines = @($verboseText -split "`n" | Where-Object { $_ -match '^release:\s*(\S+)\s*$' })
    $hostLines = @($verboseText -split "`n" | Where-Object { $_ -match '^host:\s*(\S+)\s*$' })
    if ($releaseLines.Count -ne 1 -or $releaseLines[0] -notmatch "^release:\s*$([Regex]::Escape($expectedRelease))\s*$" -or
        $hostLines.Count -ne 1 -or $hostLines[0] -notmatch "^host:\s*$([Regex]::Escape($ExpectedHost))\s*$") {
        Stop-Gate 24 "持久 User/Machine PATH 中的 rustc release 或 host 与当前已通过工具链不一致；已在下载和写入前停止"
    }
}

function Assert-MachinePathCommandAlignment {
    param([string]$Name, [string]$CurrentPath, [bool]$WillInstallToUserRoot)
    $shadow = Resolve-PersistedPathCommand $Name (Get-PersistedMachinePath)
    if (-not $shadow) { return }
    if ($WillInstallToUserRoot -or [string]::IsNullOrWhiteSpace($CurrentPath)) {
        Stop-Gate 24 "持久 Machine PATH 中的 $Name 会遮蔽待安装的当前用户工具；已在下载和写入前停止：$shadow"
    }
    $expected = [IO.Path]::GetFullPath($CurrentPath)
    if (-not [string]::Equals($shadow, $expected, [StringComparison]::OrdinalIgnoreCase)) {
        Stop-Gate 24 "持久 Machine PATH 中的 $Name 与当前已通过工具路径不一致；已在下载和写入前停止：$shadow"
    }
}

function Assert-NoMachinePathCommandShadow {
    param([string[]]$Names, [string]$Label)
    $machinePath = Get-PersistedMachinePath
    foreach ($name in $Names) {
        $shadow = Resolve-PersistedPathCommand $name $machinePath
        if ($shadow) {
            Stop-Gate 24 "持久 Machine PATH 中的 $name 会遮蔽待安装的$Label；已在下载和写入前停止：$shadow"
        }
    }
}

# 归一化一个或多个 PATH 值：保留原顺序，去除空项与大小写不敏感的重复项。
function ConvertTo-NormalizedPathValue {
    param([string[]]$Values)
    $result = [System.Collections.Generic.List[string]]::new()
    $seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($value in $Values) {
        foreach ($entry in ($value -split [IO.Path]::PathSeparator)) {
            $key = Get-PathEntryKey $entry
            if ($key -and $seen.Add($key)) {
                $result.Add((ConvertTo-PathEntryValue $entry))
            }
        }
    }
    return ($result -join [IO.Path]::PathSeparator)
}

# 把新工具目录加入本次复探 PATH，同样消除空段与重复项，避免子进程把空段解释为 cwd。
function Add-ProbePathEntry {
    param([string]$Entry)
    $script:ProbePath = ConvertTo-NormalizedPathValue -Values @($Entry, $script:ProbePath)
    $env:PATH = $script:ProbePath
}

$script:ProbePath = ConvertTo-NormalizedPathValue -Values @($script:ProbePath)
$env:PATH = ConvertTo-NormalizedPathValue -Values @($env:PATH)

# 把安装目录原子写入用户级 PATH，移除空项与重复项，并保留全部既有合法用户条目。
function Add-PersistedUserPathEntries {
    param([string[]]$Entries)
    if ($TestMode -and $env:AFH_SKIP_PERSIST_PATH -eq "1") { return }
    Assert-ProjectedPersistedToolIdentities -PrependedUserEntries $Entries

    $result = [System.Collections.Generic.List[string]]::new()
    $seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)

    foreach ($entry in @($Entries) + @((Get-PersistedUserPath) -split [IO.Path]::PathSeparator)) {
        $key = Get-PathEntryKey $entry
        if (-not $key) { continue }
        if ($seen.Add($key)) { $result.Add((ConvertTo-PathEntryValue $entry)) }
    }
    $newValue = $result -join [IO.Path]::PathSeparator
    Set-PersistedUserPath $newValue

    $persistedKeys = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in ((Get-PersistedUserPath) -split [IO.Path]::PathSeparator)) {
        $key = Get-PathEntryKey $entry
        if ($key) { [void]$persistedKeys.Add($key) }
    }
    foreach ($entry in $Entries) {
        if (-not $persistedKeys.Contains((Get-PathEntryKey $entry))) {
            Stop-Gate 24 "用户级 PATH 写入后复探缺少目录：$entry"
        }
    }
}

# 从持久 User/Machine PATH 启动一个全新 PowerShell，并实际调用工具证明新会话可发现它们。
function Test-FreshPowerShellToolDiscovery {
    param([bool]$RequirePnpm)

    $shellName = if ($PSVersionTable.PSEdition -eq "Core") { "pwsh.exe" } else { "powershell.exe" }
    $shellExecutable = Join-Path $PSHOME $shellName
    if (-not (Test-Path -LiteralPath $shellExecutable -PathType Leaf)) {
        Stop-Gate 24 "无法启动新的 PowerShell 复探用户级 PATH"
    }
    $encodeFreshValue = {
        param([string]$Value)
        [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($Value))
    }
    $checkRows = [System.Collections.Generic.List[string]]::new()
    foreach ($spec in @(
        @("git", $git, $GitVersion),
        @("rustup", $rustup, $RustupVersion),
        @("rustc", $rustc, $RustVersion),
        @("cargo", $cargo, $CargoVersion)
    )) {
        $encodedPath = & $encodeFreshValue ([IO.Path]::GetFullPath([string]$spec[1]))
        $encodedVersion = & $encodeFreshValue ([string]$spec[2])
        $checkRows.Add(('    @("{0}", "--version", "{1}", "{2}")' -f $spec[0], $encodedPath, $encodedVersion))
    }
    foreach ($spec in @(
        @("node", $node, $NodeVersion),
        @("npm", $npm, $NpmVersion)
    )) {
        $encodedPath = & $encodeFreshValue ([IO.Path]::GetFullPath([string]$spec[1]))
        $encodedVersion = & $encodeFreshValue ([string]$spec[2])
        $checkRows.Add(('    @("{0}", "--version", "{1}", "{2}")' -f $spec[0], $encodedPath, $encodedVersion))
    }
    if ($RequirePnpm) {
        $encodedPath = & $encodeFreshValue ([IO.Path]::GetFullPath([string]$pnpm))
        $encodedVersion = & $encodeFreshValue ([string]$PnpmVersion)
        $checkRows.Add(('    @("pnpm", "--version", "{0}", "{1}")' -f $encodedPath, $encodedVersion))
    }
    $checksLiteral = $checkRows -join ",`n"
    $encodedRustHost = & $encodeFreshValue $RustHost
    $encodedUserCargoHome = & $encodeFreshValue ([string](Get-PersistedUserEnvironmentValue "CARGO_HOME"))
    $encodedUserRustupHome = & $encodeFreshValue ([string](Get-PersistedUserEnvironmentValue "RUSTUP_HOME"))
    $childScript = @"
`$ErrorActionPreference = "Stop"
function Decode-AfhValue([string]`$Value) { [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String(`$Value)) }
`$expectedRustHost = Decode-AfhValue "$encodedRustHost"
`$userCargoHome = Decode-AfhValue "$encodedUserCargoHome"
`$userRustupHome = Decode-AfhValue "$encodedUserRustupHome"
if ([string]::IsNullOrWhiteSpace(`$userCargoHome)) { Remove-Item Env:CARGO_HOME -ErrorAction SilentlyContinue } else { [Environment]::SetEnvironmentVariable("CARGO_HOME", `$userCargoHome, "Process") }
if ([string]::IsNullOrWhiteSpace(`$userRustupHome)) { Remove-Item Env:RUSTUP_HOME -ErrorAction SilentlyContinue } else { [Environment]::SetEnvironmentVariable("RUSTUP_HOME", `$userRustupHome, "Process") }
if (`$env:AFH_TEST_MODE -eq "1" -and `$env:AFH_TEST_USER_PATH_FILE) {
    `$userPath = [IO.File]::ReadAllText(`$env:AFH_TEST_USER_PATH_FILE).TrimEnd("`r", "`n")
    `$machinePath = `$env:AFH_TEST_MACHINE_PATH
} else {
    `$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    `$machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
}
`$pathEntries = [System.Collections.Generic.List[string]]::new()
`$pathSeen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach (`$pathValue in @(`$machinePath, `$userPath)) {
    if ([string]::IsNullOrWhiteSpace(`$pathValue)) { continue }
    foreach (`$pathEntry in (`$pathValue -split [regex]::Escape([string][IO.Path]::PathSeparator))) {
        `$normalizedEntry = [Environment]::ExpandEnvironmentVariables(`$pathEntry.Trim().Trim('"'))
        `$driveOrRootRelative = `$normalizedEntry -match '^[A-Za-z]:(?![\\/])' -or `$normalizedEntry -match '^[\\/](?![\\/])'
        if ([string]::IsNullOrWhiteSpace(`$normalizedEntry) -or `$driveOrRootRelative -or -not [IO.Path]::IsPathRooted(`$normalizedEntry)) { continue }
        try {
            `$fullEntry = [IO.Path]::GetFullPath(`$normalizedEntry)
            `$entryRoot = [IO.Path]::GetPathRoot(`$fullEntry)
            `$normalizedEntry = if (`$entryRoot -and `$fullEntry.TrimEnd('\', '/') -eq `$entryRoot.TrimEnd('\', '/')) { `$entryRoot } else { `$fullEntry.TrimEnd('\', '/') }
        } catch { continue }
        if (`$pathSeen.Add(`$normalizedEntry)) { `$pathEntries.Add(`$normalizedEntry) }
    }
}
if (`$pathEntries.Count -eq 0) { exit 40 }
`$env:PATH = `$pathEntries -join [IO.Path]::PathSeparator
function Resolve-AfhFreshCommand([string]`$Name) {
    `$resolved = Get-Command -Name `$Name -CommandType Application,ExternalScript -All -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not `$resolved -or [string]::IsNullOrWhiteSpace([string]`$resolved.Path)) { return "" }
    return [IO.Path]::GetFullPath([string]`$resolved.Path)
}
`$checks = @(
$checksLiteral
)
foreach (`$check in `$checks) {
    `$name = [string]`$check[0]
    `$argument = [string]`$check[1]
    `$expectedPath = Decode-AfhValue ([string]`$check[2])
    `$expectedVersion = Decode-AfhValue ([string]`$check[3])
    `$resolvedPath = Resolve-AfhFreshCommand `$name
    if (-not `$resolvedPath) { exit 41 }
    if (-not [string]::Equals(`$resolvedPath, `$expectedPath, [StringComparison]::OrdinalIgnoreCase)) { exit 42 }
    `$actualVersion = (((& `$resolvedPath `$argument 2>`$null) -join "`n")).Trim()
    if (`$LASTEXITCODE -ne 0) { exit 42 }
    if (`$actualVersion -ne `$expectedVersion) { exit 43 }
    if (`$name -eq "rustc") {
        `$verbose = ((& `$resolvedPath -vV 2>`$null) -join "`n")
        if (`$LASTEXITCODE -ne 0 -or `$expectedVersion -notmatch '^rustc (\d+\.\d+\.\d+)(?:\s|`$)') { exit 44 }
        `$expectedRelease = `$Matches[1]
        `$releaseLines = @(`$verbose -split "`n" | Where-Object { `$_ -match '^release:\s*(\S+)\s*`$' })
        `$hostLines = @(`$verbose -split "`n" | Where-Object { `$_ -match '^host:\s*(\S+)\s*`$' })
        if (`$releaseLines.Count -ne 1 -or `$releaseLines[0] -notmatch "^release:\s*`$([Regex]::Escape(`$expectedRelease))\s*`$") { exit 44 }
        if (`$hostLines.Count -ne 1 -or `$hostLines[0] -notmatch "^host:\s*`$([Regex]::Escape(`$expectedRustHost))\s*`$") { exit 44 }
    }
}
"@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childScript))
    & $shellExecutable -NoLogo -NoProfile -NonInteractive -EncodedCommand $encoded
    if ($LASTEXITCODE -ne 0) {
        Stop-Gate 24 "新的 PowerShell 无法从持久 User/Machine PATH 复探全部环境工具"
    }
}

# 使用 .NET 计算 SHA-256，避免最小 PowerShell 环境尚未自动加载 Get-FileHash 模块。
function Get-Sha256File {
    param([string]$Path)
    $stream = [IO.File]::OpenRead($Path)
    try {
        $algorithm = [Security.Cryptography.SHA256]::Create()
        try {
            return ([BitConverter]::ToString($algorithm.ComputeHash($stream))).Replace("-", "").ToLowerInvariant()
        } finally {
            $algorithm.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

# 返回 Node 安装目录内的本机可执行文件；cmd 仅供显式 file URL 隔离夹具使用。
function Get-NodeExecutableInDirectory {
    param([string]$Directory)
    foreach ($candidateName in @("node.exe", "node.cmd")) {
        if ($candidateName -eq "node.cmd" -and $env:AFH_ALLOW_FILE_URLS -ne "1") { continue }
        $candidate = Join-Path $Directory $candidateName
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            $item = Get-Item -LiteralPath $candidate -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0) { return $item.FullName }
        }
    }
    return $null
}

function Test-NodeExecutableInDirectory {
    param([string]$Directory)
    return $null -ne (Get-NodeExecutableInDirectory $Directory)
}

# npm 会覆盖这些 Windows 用户级 wrapper；任何既有 reparse/越界目标都必须在 npm 写入前阻断。
function Assert-ManagedPnpmWrapperInventory {
    if (-not (Test-Path -LiteralPath $script:ManagedPnpmHome -PathType Container)) { return }
    Assert-ProjectedPathDirectoryOwnership $script:ManagedPnpmHome @("pnpm") "pnpm 当前用户全局前缀"
    $wrapperNames = @(
        "pnpm", "pnpm.com", "pnpm.exe", "pnpm.cmd", "pnpm.bat", "pnpm.ps1",
        "pnpx", "pnpx.com", "pnpx.exe", "pnpx.cmd", "pnpx.bat", "pnpx.ps1"
    )
    foreach ($item in @(Get-ChildItem -LiteralPath $script:ManagedPnpmHome -Force)) {
        if ($item.Name -notin $wrapperNames) { continue }
        Assert-ManagedCommandWrapper $item.FullName $script:ManagedPnpmHome 28 "pnpm 用户级 wrapper $($item.Name)" | Out-Null
    }
}

# 在读取远端发布索引前安全枚举版本形态目录；明显冲突、损坏 marker 或残缺安装必须先失败关闭。
function Assert-ManagedNodeRootInventory {
    Assert-ManagedDirectoryPath $script:ManagedNodeHome $LocalAppDataRoot "Node.js 当前用户安装根"
    if (-not (Test-Path -LiteralPath $script:ManagedNodeHome -PathType Container)) { return }

    foreach ($entry in @(Get-ChildItem -LiteralPath $script:ManagedNodeHome -Force)) {
        if ($entry.Name -notmatch '^v\d+\.\d+\.\d+$') { continue }
        if (-not $entry.PSIsContainer -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            Stop-Gate 26 "Node.js 版本目标不是普通目录：$($entry.FullName)"
        }
        Assert-ManagedDirectoryPath $entry.FullName $LocalAppDataRoot "Node.js 版本目录"
        Assert-ProjectedPathDirectoryOwnership $entry.FullName @("node", "npm") "Node.js 版本目录"
        $ownershipMarker = Join-Path $entry.FullName ".agent-first-harness-managed"
        if (-not (Test-Path -LiteralPath $ownershipMarker -PathType Leaf)) {
            Stop-Gate 26 "Node.js 目标已存在但缺少有效受管所有权标记：$($entry.FullName)"
        }
        $markerItem = Get-Item -LiteralPath $ownershipMarker -Force
        if (($markerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            Stop-Gate 26 "Node.js 目标已存在但缺少有效受管所有权标记：$($entry.FullName)"
        }
        $markerText = ((Get-Content -LiteralPath $ownershipMarker -Raw) -replace "`r`n", "`n").TrimEnd("`r", "`n")
        $markerLines = @($markerText -split "`n")
        if ($markerLines.Count -ne 3 -or
            $markerLines[0] -ne "managed by agent-first-harness development environment gate" -or
            $markerLines[1] -ne "node.version=$($entry.Name)" -or
            $markerLines[2] -notmatch '^node\.archive\.sha256=[0-9a-fA-F]{64}$') {
            Stop-Gate 26 "Node.js 目标已存在但缺少有效受管所有权标记：$($entry.FullName)"
        }
        $nodeExecutable = Get-NodeExecutableInDirectory $entry.FullName
        $npmExecutable = Resolve-ManagedCommandWrapper "npm" $entry.FullName 26 "Node.js 版本目录内的 npm wrapper"
        if (-not $nodeExecutable -or -not $npmExecutable) {
            Stop-Gate 26 "Node.js 目标已存在但不可用：$($entry.FullName)"
        }
    }
}

# 验证 Node.js 达到连续最低下界；任何更高正式版本都直接通过。
function Test-NodeVersion {
    param([string]$NodePath)
    $nodeText = ([string](& $NodePath --version 2>$null)).Trim()
    if ($LASTEXITCODE -ne 0) { Stop-Gate 23 "Node.js 探测失败" }
    if ($nodeText -notmatch '^v(\d+)\.(\d+)\.(\d+)$') {
        Stop-Gate 23 "现有 Node.js 不是可识别的稳定发布版：$nodeText"
    }
    $major = [int]$Matches[1]
    $minor = [int]$Matches[2]
    $patch = [int]$Matches[3]
    $compatible = $major -gt 24 -or
        ($major -eq 24 -and ($minor -gt 21 -or ($minor -eq 21 -and $patch -ge 0)))
    $script:NodeVersion = $nodeText
    if (-not $compatible) { return "upgrade-required" }
    return "passed"
}

# Node 路线同时要求可解析的稳定 npm，保证全局安装 pnpm 与新会话复探使用同一工具。
function Test-NpmVersion {
    param([string]$NpmPath)
    $npmText = ([string](& $NpmPath --version 2>$null)).Trim()
    if ($LASTEXITCODE -ne 0) { Stop-Gate 23 "npm 探测失败" }
    if ($npmText -notmatch '^(\d+)\.(\d+)\.(\d+)$') { Stop-Gate 23 "现有 npm 不是可识别的稳定发布版：$npmText" }
    $script:NpmVersion = $npmText
}

# 验证 pnpm 满足最低版本；明确低于下界时返回待升级状态。
function Test-PnpmVersion {
    param([string]$PnpmPath)
    $pnpmText = ([string](& $PnpmPath --version 2>$null)).Trim()
    if ($LASTEXITCODE -ne 0) { Stop-Gate 28 "pnpm 探测失败" }
    if ($pnpmText -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
        Stop-Gate 28 "现有 pnpm 不是可识别的稳定发布版：$pnpmText"
    }
    $major = [int]$Matches[1]
    $minor = [int]$Matches[2]
    $patch = [int]$Matches[3]
    $script:PnpmVersion = $pnpmText
    if ($major -lt 12 -or ($major -eq 12 -and ($minor -lt 4 -or ($minor -eq 4 -and $patch -lt 1)))) {
        return "upgrade-required"
    }
    return "passed"
}

# Git 是所有初始化路径的基础工具；明确低于下界时返回待升级状态。
function Test-GitVersion {
    param([string]$GitPath)
    $gitText = ([string](& $GitPath --version 2>$null)).Trim()
    if ($LASTEXITCODE -ne 0) { Stop-Gate 29 "Git 探测失败" }
    if ($gitText -notmatch '^git version (\d+)\.(\d+)\.(\d+)(?:\.windows\.\d+)?(?:\s.*)?$') {
        Stop-Gate 29 "现有 Git 不是可识别的稳定发布版：$gitText"
    }
    $script:GitVersion = $gitText
    $gitMajor = [int]$Matches[1]
    $gitMinor = [int]$Matches[2]
    if ($gitMajor -lt 2 -or ($gitMajor -eq 2 -and $gitMinor -lt 36)) { return "upgrade-required" }
    return "passed"
}

# Windows 只通过既有 winget 安装或升级 Git.Git，并在当前进程刷新常见 Git 路径。
function Install-MissingGit {
    param([ValidateSet("installed", "upgraded")][string]$Change)
    $winget = Resolve-GateCommand "winget"
    if (-not $winget) { Stop-Gate 29 "Windows 安装 Git 需要既有 winget" }
    $action = if ($Change -eq "upgraded") { "升级" } else { "安装" }
    [Console]::Error.WriteLine("正在通过既有 winget $action Git.Git。")
    $arguments = @("install", "--id", "Git.Git", "--exact", "--silent", "--disable-interactivity", "--accept-package-agreements", "--accept-source-agreements")
    if ($Change -eq "upgraded") { $arguments += "--force" }
    & $winget @arguments
    if ($LASTEXITCODE -ne 0) { Stop-Gate 29 "winget $action Git.Git 失败" }
    $candidateBins = if ($env:AFH_PREREQ_PATH) {
        @()
    } else {
        @(
            (Join-Path $env:ProgramFiles "Git\cmd"),
            (Join-Path $env:LOCALAPPDATA "Programs\Git\cmd")
        )
    }
    foreach ($candidateBin in $candidateBins) {
        if (Test-Path -LiteralPath (Join-Path $candidateBin "git.exe") -PathType Leaf) {
            $script:GitBin = $candidateBin
            Add-ProbePathEntry $GitBin
            break
        }
    }
    $script:GitChange = $Change
}

# 使用既有 rustup 或已校验的官方 rustup-init 安装/升级 stable 工具链。
function Install-MissingRust {
    param([ValidateSet("installed", "upgraded")][string]$Change)
    $rustup = Resolve-GateCommand "rustup"
    if ($Change -eq "upgraded" -and $rustup) {
        [Console]::Error.WriteLine("正在通过既有 rustup 把 Rust 升级到当前 stable 工具链。")
        Initialize-ManagedDirectoryPath $script:ManagedCargoHome $UserProfileRoot "Rust Cargo 当前用户安装根"
        Initialize-ManagedDirectoryPath $script:ManagedRustupHome $UserProfileRoot "Rust rustup 当前用户安装根"
        Invoke-RustCommand $rustup @("toolchain", "install", "stable", "--profile", "minimal") | Out-Null
        if ($script:RustCommandExitCode -ne 0) { Stop-Gate 22 "Rust stable 工具链升级失败" }
        Invoke-RustCommand $rustup @("default", "stable") | Out-Null
        if ($script:RustCommandExitCode -ne 0) { Stop-Gate 22 "Rust stable 默认工具链切换失败" }
        $candidateBin = Join-Path $script:ManagedCargoHome "bin"
        $persistedRustBins = [System.Collections.Generic.List[string]]::new()
        if (Test-Path -LiteralPath $candidateBin -PathType Container) {
            $script:CargoBin = $candidateBin
            Add-ProbePathEntry $CargoBin
            $persistedRustBins.Add($candidateBin)
        }
        foreach ($toolPath in @($rustup, (Resolve-GateCommand "rustc"), (Resolve-GateCommand "cargo"))) {
            if ($toolPath) { $persistedRustBins.Add((Split-Path -Parent $toolPath)) }
        }
        Add-PersistedUserPathEntries -Entries @($persistedRustBins)
        $script:RustChange = "upgraded"
        return
    }
    $architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    $target = switch ($architecture) {
        "x64" { "x86_64-pc-windows-msvc" }
        "arm64" { "aarch64-pc-windows-msvc" }
        default { Stop-Gate 22 "rustup 不支持此 Windows 架构：$architecture" }
    }
    $base = if ($env:AFH_RUSTUP_DIST_BASE) { $env:AFH_RUSTUP_DIST_BASE.TrimEnd('/') } else { "https://static.rust-lang.org/rustup/dist" }
    $temporary = New-GateTemporaryDirectory
    $installer = Join-Path $temporary "rustup-init.exe"
    $checksumFile = Join-Path $temporary "rustup-init.exe.sha256"
    $releaseBase = "$base/$target"
    $action = if ($Change -eq "upgraded") { "升级" } else { "安装" }
    [Console]::Error.WriteLine("正在从 $releaseBase $action Rust stable 到标准当前用户位置；PATH 将由门禁统一持久化。")
    Get-OfficialFile "$releaseBase/rustup-init.exe" $installer
    Get-OfficialFile "$releaseBase/rustup-init.exe.sha256" $checksumFile
    $expected = ((Get-Content -LiteralPath $checksumFile -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
    $actual = Get-Sha256File $installer
    if ($actual -ne $expected) { Stop-Gate 22 "rustup-init SHA-256 校验失败" }
    Initialize-ManagedDirectoryPath $script:ManagedCargoHome $UserProfileRoot "Rust Cargo 当前用户安装根"
    Initialize-ManagedDirectoryPath $script:ManagedRustupHome $UserProfileRoot "Rust rustup 当前用户安装根"
    Invoke-RustCommand $installer @("-y", "--profile", "minimal", "--default-toolchain", "stable", "--no-modify-path") | Out-Null
    if ($script:RustCommandExitCode -ne 0) { Stop-Gate 22 "Rust 安装失败" }
    $script:CargoBin = Join-Path $script:ManagedCargoHome "bin"
    Add-ProbePathEntry $script:CargoBin
    Add-PersistedUserPathEntries -Entries @($script:CargoBin)
    $script:RustChange = $Change
}

# 验证 Windows Rust 编译所需 MSVC C++ 工具是否可由当前环境定位。
function Test-MsvcPrerequisite {
    if (Resolve-GateCommand "cl") { return $true }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere -PathType Leaf) {
        $installation = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($LASTEXITCODE -eq 0 -and $installation) { return $true }
    }
    return $false
}

# 下载并运行微软签名的 Build Tools 引导程序，再由调用方重新探测 C++ 工作负载。
function Install-MissingMsvc {
    $temporary = New-GateTemporaryDirectory
    $installer = Join-Path $temporary "vs_BuildTools.exe"
    $source = if ($env:AFH_VS_BUILDTOOLS_URL) { $env:AFH_VS_BUILDTOOLS_URL } else { "https://aka.ms/vs/17/release/vs_BuildTools.exe" }
    [Console]::Error.WriteLine("正在从 $source 安装缺失的 Microsoft Visual Studio Build Tools C++ 工作负载。")
    Get-OfficialFile $source $installer
    if (-not ($TestMode -and $env:AFH_SKIP_AUTHENTICODE -eq "1")) {
        $signature = Get-AuthenticodeSignature -FilePath $installer
        if ($signature.Status -ne "Valid" -or $signature.SignerCertificate.Subject -notmatch "Microsoft Corporation") {
            Stop-Gate 27 "Visual Studio Build Tools 引导程序没有有效的 Microsoft 签名"
        }
    }
    $arguments = @(
        "--quiet",
        "--wait",
        "--norestart",
        "--nocache",
        "--add", "Microsoft.VisualStudio.Workload.VCTools",
        "--includeRecommended"
    )
    $process = Start-Process -FilePath $installer -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -notin @(0, 3010)) {
        Stop-Gate 27 "Visual Studio Build Tools 安装失败，退出码为 $($process.ExitCode)"
    }
    $script:MsvcChange = "installed"
}

# 从官方索引按语义版本排序并选择当前最高 LTS 的最新补丁，校验 zip 后安装/升级当前用户环境。
function Install-MissingNode {
    param([ValidateSet("installed", "upgraded")][string]$Change)
    $architecture = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    $nodeArchitecture = switch ($architecture) {
        "x64" { "x64" }
        "arm64" { "arm64" }
        default { Stop-Gate 26 "Node.js 不支持此 Windows 架构：$architecture" }
    }
    $base = if ($env:AFH_NODE_DIST_BASE) { $env:AFH_NODE_DIST_BASE.TrimEnd('/') } else { "https://nodejs.org/dist" }
    $temporary = New-GateTemporaryDirectory
    $indexPath = Join-Path $temporary "index.json"
    Get-OfficialFile "$base/index.json" $indexPath
    $index = Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json
    $releaseCandidates = @($index | ForEach-Object {
        $candidate = [string]$_.version
        $compatible = $false
        $sortVersion = $null
        if ($candidate -match '^v(\d+)\.(\d+)\.(\d+)$') {
            $major = [int]$Matches[1]
            $minor = [int]$Matches[2]
            $patch = [int]$Matches[3]
            $compatible = ($major -gt 24) -or
                ($major -eq 24 -and ($minor -gt 21 -or ($minor -eq 21 -and $patch -ge 0)))
            $sortVersion = [version]"$major.$minor.$patch"
        }
        $lts = [string]$_.lts
        if ($compatible -and -not [string]::IsNullOrWhiteSpace($lts) -and $lts -ne "False" -and $lts -ne "-") {
            [PSCustomObject]@{ Release = $_; SortVersion = $sortVersion }
        }
    })
    $selectedRelease = $releaseCandidates | Sort-Object -Property SortVersion -Descending | Select-Object -First 1
    if (-not $selectedRelease) { Stop-Gate 26 "Node.js 发布版本索引中没有满足 $NodeRequirement 的 LTS 稳定版" }
    $release = $selectedRelease.Release
    $version = [string]$release.version
    $archiveName = "node-$version-win-$nodeArchitecture.zip"
    $releaseBase = "$base/$version"
    $archive = Join-Path $temporary $archiveName
    $checksums = Join-Path $temporary "SHASUMS256.txt"
    $action = if ($Change -eq "upgraded") { "升级" } else { "安装" }
    Get-OfficialFile "$releaseBase/SHASUMS256.txt" $checksums
    $escapedName = [Regex]::Escape($archiveName)
    $checksumLine = Get-Content -LiteralPath $checksums | Where-Object { $_ -match "^([0-9a-fA-F]{64})\s+$escapedName$" } | Select-Object -First 1
    if (-not $checksumLine) { Stop-Gate 26 "Node.js 校验和列表不包含 $archiveName" }
    $checksumLine -match '^([0-9a-fA-F]{64})' | Out-Null
    $expected = $Matches[1].ToLowerInvariant()
    $markerContent = "managed by agent-first-harness development environment gate`nnode.version=$version`nnode.archive.sha256=$expected"
    $installDirectory = Join-Path $script:ManagedNodeHome $version
    if (Test-Path -LiteralPath $installDirectory) {
        Assert-ManagedDirectoryPath $installDirectory $LocalAppDataRoot "Node.js 版本目录"
        $ownershipMarker = Join-Path $installDirectory ".agent-first-harness-managed"
        $markerItem = if (Test-Path -LiteralPath $ownershipMarker -PathType Leaf) { Get-Item -LiteralPath $ownershipMarker -Force } else { $null }
        $existingMarker = if ($markerItem) { ((Get-Content -LiteralPath $ownershipMarker -Raw) -replace "`r`n", "`n").TrimEnd("`r", "`n") } else { $null }
        if (-not $markerItem -or ($markerItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 -or
            $existingMarker -ne $markerContent) {
            Stop-Gate 26 "Node.js 目标已存在但缺少有效受管所有权标记：$installDirectory"
        }
        $nodeExecutable = Get-NodeExecutableInDirectory $installDirectory
        if (-not $nodeExecutable) {
            Stop-Gate 26 "Node.js 目标已存在但不可用：$installDirectory"
        }
        $npmExecutable = Resolve-ManagedCommandWrapper "npm" $installDirectory 26 "Node.js 版本目录内的 npm wrapper"
        if (-not $npmExecutable) {
            Stop-Gate 26 "Node.js 目标内的 npm 不可用：$installDirectory"
        }
        $installedVersion = ((& $nodeExecutable --version 2>$null) -join "`n")
        if ($LASTEXITCODE -ne 0 -or $installedVersion -ne $version) {
            Stop-Gate 26 "Node.js 目标版本与已选择稳定版不一致：期望 $version，实际 $installedVersion"
        }
        $installedNpmVersion = ((& $npmExecutable --version 2>$null) -join "`n")
        if ($LASTEXITCODE -ne 0 -or $installedNpmVersion -notmatch '^\d+\.\d+\.\d+$') {
            Stop-Gate 26 "Node.js 目标内的 npm 不可用：$installDirectory"
        }
    } else {
        [Console]::Error.WriteLine("正在从 $releaseBase $action 当前最高 LTS 的最新 Node.js $version 到标准用户级目录。")
        Get-OfficialFile "$releaseBase/$archiveName" $archive
        $actual = Get-Sha256File $archive
        if ($actual -ne $expected) { Stop-Gate 26 "Node.js SHA-256 校验失败" }
        Initialize-ManagedDirectoryPath $script:ManagedNodeHome $LocalAppDataRoot "Node.js 当前用户安装根"
        Expand-Archive -LiteralPath $archive -DestinationPath $temporary
        $extracted = Join-Path $temporary "node-$version-win-$nodeArchitecture"
        $nodeExecutable = Get-NodeExecutableInDirectory $extracted
        if (-not $nodeExecutable) {
            Stop-Gate 26 "Node.js 归档不包含预期可执行文件"
        }
        $npmExecutable = Resolve-ManagedCommandWrapper "npm" $extracted 26 "Node.js 归档内的 npm wrapper"
        if (-not $npmExecutable) {
            Stop-Gate 26 "Node.js 归档不包含可用的 npm wrapper"
        }
        $extractedVersion = (& $nodeExecutable --version 2>$null)
        if ($LASTEXITCODE -ne 0 -or $extractedVersion -ne $version) {
            Stop-Gate 26 "Node.js 归档版本与已选择稳定版不一致：期望 $version，实际 $extractedVersion"
        }
        $extractedNpmVersion = ((& $npmExecutable --version 2>$null) -join "`n")
        if ($LASTEXITCODE -ne 0 -or $extractedNpmVersion -notmatch '^\d+\.\d+\.\d+$') {
            Stop-Gate 26 "Node.js 归档不包含可用的 npm wrapper"
        }
        Assert-ProjectedPathDirectoryOwnership $extracted @("node", "npm") "Node.js 归档目录"
        Assert-ProjectedPersistedToolIdentities -PrependedUserEntries @($extracted)
        Move-Item -LiteralPath $extracted -Destination $installDirectory
        [IO.File]::WriteAllText(
            (Join-Path $installDirectory ".agent-first-harness-managed"),
            "$markerContent`n",
            [Text.UTF8Encoding]::new($false)
        )
    }
    $installedNpmExecutable = Resolve-ManagedCommandWrapper "npm" $installDirectory 26 "Node.js 版本目录内的 npm wrapper"
    if (-not $installedNpmExecutable) {
        Stop-Gate 26 "Node.js 目标内的 npm 不可用：$installDirectory"
    }
    $script:NodeBin = $installDirectory
    $script:SelectedNodeVersion = $version
    Add-ProbePathEntry $NodeBin
    Add-PersistedUserPathEntries -Entries @($NodeBin)
    $script:NodeChange = $Change
}

# 仅为 GUI 项目通过 Node 自带 npm 安装或升级满足最低要求的 pnpm。
function Install-MissingPnpm {
    param([ValidateSet("installed", "upgraded")][string]$Change)
    $npm = Resolve-GateCommand "npm"
    if (-not $npm) { Stop-Gate 28 "为 GUI 开发安装 pnpm 需要 npm" }
    Assert-ManagedNodeRootInventory
    Assert-ManagedPnpmWrapperInventory
    Initialize-ManagedDirectoryPath $script:ManagedPnpmHome $RoamingAppDataRoot "npm 当前用户全局前缀"
    $pnpmModules = Join-Path $script:ManagedPnpmHome "node_modules"
    $pnpmPackage = Join-Path $pnpmModules "pnpm"
    Initialize-ManagedDirectoryPath $pnpmModules $RoamingAppDataRoot "npm 当前用户全局包目录"
    Assert-ManagedDirectoryPath $pnpmPackage $RoamingAppDataRoot "pnpm 当前用户全局包目标"
    Assert-ManagedPnpmWrapperInventory
    $action = if ($Change -eq "upgraded") { "升级" } else { "安装" }
    [Console]::Error.WriteLine("正在从官方 npm 软件包仓库把 $PnpmInstallRequirement $action 到标准当前用户全局前缀。")
    if ([IO.Path]::GetExtension($npm) -in @(".cmd", ".bat")) {
        # PowerShell 5.1 调用批处理包装器时不会自动保护 `>`；显式保留引号，避免版本范围被 cmd.exe 当作重定向。
        & $npm install --global --prefix $script:ManagedPnpmHome ('"' + $PnpmInstallRequirement + '"') --registry $PnpmRegistry --ignore-scripts
    } else {
        & $npm install --global --prefix $script:ManagedPnpmHome $PnpmInstallRequirement --registry $PnpmRegistry --ignore-scripts
    }
    if ($LASTEXITCODE -ne 0) { Stop-Gate 28 "pnpm 安装失败" }
    Assert-ManagedPnpmWrapperInventory
    $script:PnpmBin = $script:ManagedPnpmHome
    Add-ProbePathEntry $PnpmBin
    Add-PersistedUserPathEntries -Entries @($PnpmBin)
    $script:PnpmChange = $Change
}

try {
    $git = Resolve-GateCommand "git"
    $rustup = Resolve-GateCommand "rustup"
    $rustc = Resolve-GateCommand "rustc"
    $cargo = Resolve-GateCommand "cargo"

    if ($git) {
        $gitState = Test-GitVersion $git
    } else {
        $GitVersion = "Missing"
        $gitState = "missing"
    }

    if ($rustup -and $rustc -and $cargo) {
        $rustState = Test-RustVersion $rustup $rustc $cargo
    } else {
        $RustupVersion = "Missing"
        $RustVersion = "Missing"
        $CargoVersion = "Missing"
        $RustHost = "Missing"
        $rustState = "missing"
    }
    $node = Resolve-GateCommand "node"
    $npm = Resolve-GateCommand "npm"
    if ($node -and $npm) {
        $nodeState = Test-NodeVersion $node
        Test-NpmVersion $npm
    } else {
        $NodeVersion = "Missing"
        $NpmVersion = "Missing"
        $nodeState = "missing"
    }
    if ($PnpmRequired) {
        $pnpm = Resolve-GateCommand "pnpm"
        if ($pnpm) {
            $pnpmState = Test-PnpmVersion $pnpm
        } else {
            $PnpmVersion = "Missing"
            $pnpmState = "missing"
        }
    } else {
        $PnpmVersion = "Not-required"
        $pnpmState = "not-required"
    }
    $msvcMissing = -not (Test-MsvcPrerequisite)

    if ($CheckOnly) {
        "gate.git.status=$gitState"
        "gate.git.requirement=$GitRequirement"
        "gate.git.version=$GitVersion"
        "gate.rust.status=$rustState"
        "gate.rust.version=$RustVersion"
        "gate.rust.rustup_version=$RustupVersion"
        "gate.rust.cargo_version=$CargoVersion"
        "gate.rust.host=$RustHost"
        "gate.node.status=$nodeState"
        "gate.node.requirement=$NodeRequirement"
        "gate.node.version=$NodeVersion"
        "gate.pnpm.status=$pnpmState"
        "gate.pnpm.requirement=$PnpmRequirement"
        "gate.pnpm.version=$PnpmVersion"
        "gate.msvc.status=$(if ($msvcMissing) { 'missing' } else { 'passed' })"
        if ($gitState -ne "passed" -or $rustState -ne "passed" -or $nodeState -ne "passed" -or
            ($PnpmRequired -and $pnpmState -ne "passed") -or $msvcMissing) { exit 20 }
        exit 0
    }

    # 所有将使用的用户安装根、已有 Node 版本目录和持久解析顺序都在任何环境写入、winget、下载器或安装器前一次性预检。
    $requiresChange = $gitState -ne "passed" -or $rustState -ne "passed" -or
        $nodeState -ne "passed" -or ($PnpmRequired -and $pnpmState -ne "passed") -or $msvcMissing
    if ($rustState -ne "passed") {
        Assert-ManagedDirectoryPath $script:ManagedCargoHome $UserProfileRoot "Rust Cargo 当前用户安装根"
        Assert-ManagedDirectoryPath $script:ManagedRustupHome $UserProfileRoot "Rust rustup 当前用户安装根"
    }
    if ($nodeState -ne "passed" -or ($PnpmRequired -and $pnpmState -ne "passed")) {
        Assert-ManagedNodeRootInventory
    }
    if ($pnpmState -ne "passed" -and $pnpmState -ne "not-required") {
        Assert-ManagedDirectoryPath $script:ManagedPnpmHome $RoamingAppDataRoot "npm 当前用户全局前缀"
        Assert-ManagedPnpmWrapperInventory
        Assert-ManagedDirectoryPath (Join-Path $script:ManagedPnpmHome "node_modules") $RoamingAppDataRoot "npm 当前用户全局包目录"
        Assert-ManagedDirectoryPath (Join-Path $script:ManagedPnpmHome "node_modules\pnpm") $RoamingAppDataRoot "pnpm 当前用户全局包目标"
    }
    $persistenceEnabled = -not ($TestMode -and $env:AFH_SKIP_PERSIST_PATH -eq "1")
    if ($requiresChange -and $persistenceEnabled) {
        $persistentPath = Get-PersistedCombinedPath
        if ($gitState -eq "passed") {
            Assert-PersistedCommandIdentity -Name "git" -CurrentPath $git -ExpectedVersion $GitVersion -PersistentPath $persistentPath
        }
        if ($rustState -eq "passed") {
            Assert-PersistedCommandIdentity -Name "rustup" -CurrentPath $rustup -ExpectedVersion $RustupVersion -PersistentPath $persistentPath
            Assert-PersistedCommandIdentity -Name "rustc" -CurrentPath $rustc -ExpectedVersion $RustVersion -PersistentPath $persistentPath
            Assert-PersistedCommandIdentity -Name "cargo" -CurrentPath $cargo -ExpectedVersion $CargoVersion -PersistentPath $persistentPath
            Assert-PersistedRustHostIdentity -RustcPath $rustc -ExpectedVersion $RustVersion -ExpectedHost $RustHost -PersistentPath $persistentPath
        }
        if ($nodeState -eq "passed") {
            Assert-PersistedCommandIdentity -Name "node" -CurrentPath $node -ExpectedVersion $NodeVersion -PersistentPath $persistentPath
            Assert-PersistedCommandIdentity -Name "npm" -CurrentPath $npm -ExpectedVersion $NpmVersion -PersistentPath $persistentPath
        }
        if ($PnpmRequired -and $pnpmState -eq "passed") {
            Assert-PersistedCommandIdentity -Name "pnpm" -CurrentPath $pnpm -ExpectedVersion $PnpmVersion -PersistentPath $persistentPath
        }

        Assert-MachinePathCommandAlignment -Name "git" -CurrentPath $git -WillInstallToUserRoot $false
        $rustInstallChangesPath = $rustState -ne "passed"
        Assert-MachinePathCommandAlignment -Name "rustup" -CurrentPath $rustup -WillInstallToUserRoot $rustInstallChangesPath
        Assert-MachinePathCommandAlignment -Name "rustc" -CurrentPath $rustc -WillInstallToUserRoot $rustInstallChangesPath
        Assert-MachinePathCommandAlignment -Name "cargo" -CurrentPath $cargo -WillInstallToUserRoot $rustInstallChangesPath
        $nodeInstallChangesPath = $nodeState -ne "passed"
        Assert-MachinePathCommandAlignment -Name "node" -CurrentPath $node -WillInstallToUserRoot $nodeInstallChangesPath
        Assert-MachinePathCommandAlignment -Name "npm" -CurrentPath $npm -WillInstallToUserRoot $nodeInstallChangesPath
        if ($PnpmRequired) {
            Assert-MachinePathCommandAlignment -Name "pnpm" -CurrentPath $pnpm -WillInstallToUserRoot ($pnpmState -ne "passed")
        }

        if ($gitState -ne "passed") {
            Assert-PendingPersistedCommandIdentity -Name "git" -CurrentPath $git -PersistentPath $persistentPath
        }
        if ($rustState -ne "passed") {
            Assert-PendingPersistedCommandIdentity -Name "rustup" -CurrentPath $rustup -PersistentPath $persistentPath
            Assert-PendingPersistedCommandIdentity -Name "rustc" -CurrentPath $rustc -PersistentPath $persistentPath
            Assert-PendingPersistedCommandIdentity -Name "cargo" -CurrentPath $cargo -PersistentPath $persistentPath
        }
        if ($nodeState -ne "passed") {
            Assert-PendingPersistedCommandIdentity -Name "node" -CurrentPath $node -PersistentPath $persistentPath
            Assert-PendingPersistedCommandIdentity -Name "npm" -CurrentPath $npm -PersistentPath $persistentPath
        }
        if ($PnpmRequired -and $pnpmState -ne "passed") {
            Assert-PendingPersistedCommandIdentity -Name "pnpm" -CurrentPath $pnpm -PersistentPath $persistentPath
        }

        # 一次投影所有当前已知会被前置的目录；Rust 升级还会持久化既有工具各自所在目录。
        $plannedRustUserPathEntries = [System.Collections.Generic.List[string]]::new()
        if ($rustState -ne "passed") {
            [void]$plannedRustUserPathEntries.Add((Join-Path $script:ManagedCargoHome "bin"))
            if ($rustState -eq "upgrade-required") {
                foreach ($toolPath in @($rustup, $rustc, $cargo)) {
                    if ($toolPath) {
                        [void]$plannedRustUserPathEntries.Add((Split-Path -Parent $toolPath))
                    }
                }
            }
            foreach ($entry in $plannedRustUserPathEntries) {
                Assert-ProjectedPathDirectoryOwnership $entry @("rustup", "rustc", "cargo") "Rust 持久 PATH 目录"
            }
        }
        $plannedPrependedUserEntries = [System.Collections.Generic.List[string]]::new()
        if ($PnpmRequired -and $pnpmState -ne "passed") {
            [void]$plannedPrependedUserEntries.Add($script:ManagedPnpmHome)
        }
        foreach ($entry in $plannedRustUserPathEntries) {
            [void]$plannedPrependedUserEntries.Add($entry)
        }
        if ($plannedPrependedUserEntries.Count -gt 0) {
            Assert-ProjectedPersistedToolIdentities -PrependedUserEntries @($plannedPrependedUserEntries)
        }
    }
    if ($persistenceEnabled -and $nodeState -ne "passed" -and $nodeState -ne "not-required") {
        Assert-NoMachinePathCommandShadow -Names @("node", "npm") -Label "Node.js/npm 用户级版本"
    }
    if ($persistenceEnabled -and $pnpmState -ne "passed" -and $pnpmState -ne "not-required") {
        Assert-NoMachinePathCommandShadow -Names @("pnpm") -Label "pnpm 用户级版本"
    }
    if ($requiresChange) {
        Assert-DurableRustHomes
    }

    if ($gitState -ne "passed") {
        $change = if ($gitState -eq "missing") { "installed" } else { "upgraded" }
        Install-MissingGit $change
        $git = Resolve-GateCommand "git"
        if (-not $git) { Stop-Gate 29 "Git 安装完成后仍无法调用 git 可执行文件" }
        $gitState = Test-GitVersion $git
        if ($gitState -ne "passed") { Stop-Gate 29 "Git 安装或升级后仍不满足 $GitRequirement" }
    }
    if ($rustState -ne "passed") {
        $change = if ($rustState -eq "missing") { "installed" } else { "upgraded" }
        Install-MissingRust $change
        $rustup = Resolve-GateCommand "rustup"
        $rustc = Resolve-GateCommand "rustc"
        $cargo = Resolve-GateCommand "cargo"
        if (-not $rustup -or -not $rustc -or -not $cargo) { Stop-Gate 22 "Rust 安装完成后仍无法调用 rustup、rustc 和 cargo" }
        $rustState = Test-RustVersion $rustup $rustc $cargo
        if ($rustState -ne "passed") { Stop-Gate 22 "Rust 安装或升级后仍低于 MSRV $MinimumRustMajor.$MinimumRustMinor.$MinimumRustPatch" }
    }
    if ($msvcMissing) {
        Install-MissingMsvc
        if (-not (Test-MsvcPrerequisite)) {
            Stop-Gate 27 "Visual Studio Build Tools 安装完成后 MSVC C++ 工作负载仍不可用"
        }
    }
    if ($nodeState -ne "passed" -and $nodeState -ne "not-required") {
        $change = if ($nodeState -eq "missing") { "installed" } else { "upgraded" }
        Install-MissingNode $change
        $node = Resolve-GateCommand "node"
        $npm = Resolve-ManagedCommandWrapper "npm" $script:NodeBin 26 "Node.js 版本目录内的 npm wrapper"
        if (-not $node -or -not $npm) { Stop-Gate 26 "Node.js 安装完成后仍无法调用 node 和 npm" }
        $nodeState = Test-NodeVersion $node
        Test-NpmVersion $npm
        if ($nodeState -ne "passed") { Stop-Gate 26 "Node.js 安装或升级后仍不满足 $NodeRequirement" }
        if ($NodeVersion -ne $script:SelectedNodeVersion) {
            Stop-Gate 26 "Node.js 安装或升级后的版本与已选择稳定版不一致：期望 $($script:SelectedNodeVersion)，实际 $NodeVersion"
        }
    }
    if ($pnpmState -ne "passed" -and $pnpmState -ne "not-required") {
        $change = if ($pnpmState -eq "missing") { "installed" } else { "upgraded" }
        Install-MissingPnpm $change
        $pnpm = Resolve-ManagedCommandWrapper "pnpm" $script:ManagedPnpmHome 28 "pnpm 用户级执行 wrapper"
        if (-not $pnpm) { Stop-Gate 28 "pnpm 安装完成后仍无法调用 pnpm 可执行文件" }
        $pnpmState = Test-PnpmVersion $pnpm
        if ($pnpmState -ne "passed") { Stop-Gate 28 "pnpm 安装或升级后仍不满足 $PnpmRequirement" }
    }

    $changed = if (@($GitChange, $RustChange, $NodeChange, $PnpmChange, $MsvcChange) | Where-Object { $_ -in @("installed", "upgraded") }) { "true" } else { "false" }
    $freshShellStatus = "not-required"
    if ($changed -eq "true" -and $persistenceEnabled) {
        Test-FreshPowerShellToolDiscovery -RequirePnpm $PnpmRequired
        $freshShellStatus = "passed"
    }
    "gate.git.status=passed"
    "gate.git.requirement=$GitRequirement"
    "gate.git.version=$GitVersion"
    "gate.git.change=$GitChange"
    "gate.rust.status=passed"
    "gate.rust.version=$RustVersion"
    "gate.rust.rustup_version=$RustupVersion"
    "gate.rust.cargo_version=$CargoVersion"
    "gate.rust.host=$RustHost"
    "gate.rust.change=$RustChange"
    "gate.node.status=passed"
    "gate.node.requirement=$NodeRequirement"
    "gate.node.version=$NodeVersion"
    "gate.node.change=$NodeChange"
    "gate.pnpm.status=$(if ($PnpmRequired) { 'passed' } else { 'not-required' })"
    "gate.pnpm.requirement=$PnpmRequirement"
    "gate.pnpm.version=$PnpmVersion"
    "gate.pnpm.change=$PnpmChange"
    "gate.msvc.status=passed"
    "gate.msvc.change=$MsvcChange"
    "gate.changed=$changed"
    "gate.fresh_shell.status=$freshShellStatus"
    $prepend = ConvertTo-NormalizedPathValue -Values @($PnpmBin, $NodeBin, $CargoBin, $GitBin)
    if ($prepend) { "gate.path.prepend=$prepend" }
} finally {
    foreach ($directory in $TemporaryDirectories) {
        if (Test-Path -LiteralPath $directory) { Remove-Item -LiteralPath $directory -Recurse -Force }
    }
}
