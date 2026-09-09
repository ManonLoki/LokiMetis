#Requires -Version 5.1
[CmdletBinding()]
param(
    [switch]$CheckOnly
)

$ErrorActionPreference = "Stop"
# MSRV 的唯一事实来源见 docs/RUST_GUI_TEMPLATE.md；修改此值时必须同步更新 development-environment-gates.sh。
$MinimumRustMajor = 1
$MinimumRustMinor = 95
$NodeRequirement = "^24.15.0 || >=26.0.0"
$PnpmRequirement = ">=11.24.0"
$PnpmInstallRequirement = "pnpm@>=11.24.0"
$GitRequirement = ">=2.0.0"
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
$TemporaryDirectories = [System.Collections.Generic.List[string]]::new()

# 使用稳定退出码结束门禁，调用方可以据此区分具体失败阶段。
function Stop-Gate {
    param([int]$Code, [string]$Message)
    [Console]::Error.WriteLine("错误：$Message")
    exit $Code
}

# 只在门禁探测路径中解析工具，便于隔离机器已有环境并验证缺失分支。
function Resolve-GateCommand {
    param([string]$Name)
    foreach ($directory in ($script:ProbePath -split [IO.Path]::PathSeparator)) {
        if ([string]::IsNullOrWhiteSpace($directory)) { continue }
        # npm/pnpm 同时发布 POSIX 无扩展名 shim 与 Windows 包装器；Windows 只解析本机可执行形态。
        foreach ($candidateName in @("$Name.exe", "$Name.cmd", "$Name.bat")) {
            $candidate = Join-Path $directory $candidateName
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
        }
    }
    return $null
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

# 所有 Rust 命令都在固定的当前用户受管根内运行；调用结束后精确恢复父进程环境。
function Invoke-ManagedRustCommand {
    param(
        [string]$Path,
        [string[]]$ArgumentList = @(),
        [switch]$SuppressStderr
    )
    $savedCargoHome = [Environment]::GetEnvironmentVariable("CARGO_HOME", "Process")
    $savedRustupHome = [Environment]::GetEnvironmentVariable("RUSTUP_HOME", "Process")
    try {
        $env:CARGO_HOME = $script:ManagedCargoHome
        $env:RUSTUP_HOME = $script:ManagedRustupHome
        if ($SuppressStderr) {
            $output = (& $Path @ArgumentList 2>$null)
        } else {
            $output = (& $Path @ArgumentList)
        }
        $script:ManagedRustExitCode = $LASTEXITCODE
        return $output
    } finally {
        if ($null -eq $savedCargoHome) { Remove-Item Env:CARGO_HOME -ErrorAction SilentlyContinue } else { $env:CARGO_HOME = $savedCargoHome }
        if ($null -eq $savedRustupHome) { Remove-Item Env:RUSTUP_HOME -ErrorAction SilentlyContinue } else { $env:RUSTUP_HOME = $savedRustupHome }
    }
}

# 验证 rustup/rustc/cargo 属于同一可解释稳定工具链，并以 rustc -vV 锁定 release 与 host。
function Test-RustVersion {
    param([string]$RustupPath, [string]$RustcPath, [string]$CargoPath)
    $rustupText = Invoke-ManagedRustCommand $RustupPath @("--version") -SuppressStderr
    if ($script:ManagedRustExitCode -ne 0) { Stop-Gate 21 "rustup 探测失败" }
    if ($rustupText -notmatch '^rustup (\d+)\.(\d+)\.(\d+)(?:\s|$)') {
        Stop-Gate 21 "现有 rustup 不是可识别的稳定发布版：$rustupText"
    }
    $rustText = Invoke-ManagedRustCommand $RustcPath @("--version") -SuppressStderr
    if ($script:ManagedRustExitCode -ne 0) { Stop-Gate 21 "rustc 探测失败" }
    $cargoText = Invoke-ManagedRustCommand $CargoPath @("--version") -SuppressStderr
    if ($script:ManagedRustExitCode -ne 0) { Stop-Gate 21 "cargo 探测失败" }
    if ($rustText -notmatch '^rustc (\d+)\.(\d+)\.(\d+)(?:\s|$)') {
        Stop-Gate 21 "现有 Rust 工具链不是可识别的稳定发布版：$rustText"
    }
    $rustMajor = [int]$Matches[1]
    $rustMinor = [int]$Matches[2]
    $rustRelease = "$($Matches[1]).$($Matches[2]).$($Matches[3])"
    if ($cargoText -notmatch '^cargo (\d+)\.(\d+)\.(\d+)(?:\s|$)') {
        Stop-Gate 21 "现有 Cargo 不是可识别的稳定发布版：$cargoText"
    }
    $cargoMajor = [int]$Matches[1]
    $cargoMinor = [int]$Matches[2]
    if ($cargoMajor -ne $rustMajor -or $cargoMinor -ne $rustMinor) {
        Stop-Gate 21 "rustc 与 cargo 不属于同一 stable 工具链"
    }
    $verboseText = ((Invoke-ManagedRustCommand $RustcPath @("-vV") -SuppressStderr) -join "`n")
    if ($script:ManagedRustExitCode -ne 0) { Stop-Gate 21 "rustc -vV 探测失败" }
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
    if ($rustMajor -lt $MinimumRustMajor -or ($rustMajor -eq $MinimumRustMajor -and $rustMinor -lt $MinimumRustMinor) -or
        $cargoMajor -lt $MinimumRustMajor -or ($cargoMajor -eq $MinimumRustMajor -and $cargoMinor -lt $MinimumRustMinor)) {
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
    "AFH_TEST_MACHINE_PATH"
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
if (-not $UserProfileRoot -and $TestMode) { $UserProfileRoot = $env:USERPROFILE }
if (-not $LocalAppDataRoot -and $TestMode) { $LocalAppDataRoot = $env:LOCALAPPDATA }
if (-not $UserProfileRoot -or -not $LocalAppDataRoot) { Stop-Gate 24 "无法解析当前用户的受管安装根" }
$script:ManagedCargoHome = Join-Path $UserProfileRoot ".cargo"
$script:ManagedRustupHome = Join-Path $UserProfileRoot ".rustup"
$script:ManagedNodeHome = Join-Path $LocalAppDataRoot "AgentFirstHarness\Node"
$script:ManagedPnpmHome = Join-Path $LocalAppDataRoot "AgentFirstHarness\Pnpm"
if ($TestMode) {
    if ($env:AFH_MANAGED_CARGO_HOME) { $script:ManagedCargoHome = $env:AFH_MANAGED_CARGO_HOME }
    if ($env:AFH_MANAGED_RUSTUP_HOME) { $script:ManagedRustupHome = $env:AFH_MANAGED_RUSTUP_HOME }
    if ($env:AFH_NODE_HOME) { $script:ManagedNodeHome = $env:AFH_NODE_HOME }
    if ($env:AFH_PNPM_HOME) { $script:ManagedPnpmHome = $env:AFH_PNPM_HOME }
}
foreach ($managedRoot in @($script:ManagedCargoHome, $script:ManagedRustupHome, $script:ManagedNodeHome, $script:ManagedPnpmHome)) {
    if (-not [IO.Path]::IsPathRooted($managedRoot)) { Stop-Gate 24 "受管安装根必须是绝对路径：$managedRoot" }
}

# 逐级拒绝 reparse point 与非目录组件；生产受管根必须留在对应的当前用户目录内。
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
        Stop-Gate 24 "$Label 必须位于当前用户受管目录内：$fullPath"
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
            return (Get-Content -LiteralPath $env:AFH_TEST_USER_PATH_FILE -Raw).TrimEnd("`r", "`n")
        }
        return ""
    }
    $value = [Environment]::GetEnvironmentVariable("Path", "User")
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

# 把安装目录原子写入用户级 PATH，移除空项、重复项和同一受管根下的旧版本目录。
function Add-PersistedUserPathEntries {
    param(
        [string[]]$Entries,
        [string[]]$OwnedRoots = @()
    )
    if ($TestMode -and $env:AFH_SKIP_PERSIST_PATH -eq "1") { return }

    $ownedKeys = @($OwnedRoots | ForEach-Object { Get-PathEntryKey $_ } | Where-Object { $_ })
    $result = [System.Collections.Generic.List[string]]::new()
    $seen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)

    foreach ($entry in @($Entries) + @((Get-PersistedUserPath) -split [IO.Path]::PathSeparator)) {
        $key = Get-PathEntryKey $entry
        if (-not $key) { continue }
        $isReplacementTarget = $false
        if ($entry -notin $Entries) {
            foreach ($ownedKey in $ownedKeys) {
                if ($key -eq $ownedKey -or $key.StartsWith("$ownedKey\", [StringComparison]::OrdinalIgnoreCase)) {
                    $isReplacementTarget = $true
                    break
                }
            }
        }
        if ($isReplacementTarget) { continue }
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

# 从持久 User/Machine PATH 启动一个全新 PowerShell，并实际调用受管工具证明新会话可发现它们。
function Test-FreshPowerShellToolDiscovery {
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
        @("npm", $npm, $NpmVersion),
        @("pnpm", $pnpm, $PnpmVersion)
    )) {
        $encodedPath = & $encodeFreshValue ([IO.Path]::GetFullPath([string]$spec[1]))
        $encodedVersion = & $encodeFreshValue ([string]$spec[2])
        $checkRows.Add(('    @("{0}", "--version", "{1}", "{2}")' -f $spec[0], $encodedPath, $encodedVersion))
    }
    $checksLiteral = $checkRows -join ",`n"
    $encodedCargoHome = & $encodeFreshValue $script:ManagedCargoHome
    $encodedRustupHome = & $encodeFreshValue $script:ManagedRustupHome
    $encodedRustHost = & $encodeFreshValue $RustHost
    $childScript = @"
`$ErrorActionPreference = "Stop"
function Decode-AfhValue([string]`$Value) { [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String(`$Value)) }
`$env:CARGO_HOME = Decode-AfhValue "$encodedCargoHome"
`$env:RUSTUP_HOME = Decode-AfhValue "$encodedRustupHome"
`$expectedRustHost = Decode-AfhValue "$encodedRustHost"
if (`$env:AFH_TEST_MODE -eq "1" -and `$env:AFH_TEST_USER_PATH_FILE) {
    `$userPath = [IO.File]::ReadAllText(`$env:AFH_TEST_USER_PATH_FILE).TrimEnd("`r", "`n")
    `$machinePath = `$env:AFH_TEST_MACHINE_PATH
} else {
    `$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    `$machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
}
`$pathEntries = [System.Collections.Generic.List[string]]::new()
`$pathSeen = [System.Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach (`$pathValue in @(`$userPath, `$machinePath)) {
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
`$checks = @(
$checksLiteral
)
foreach (`$check in `$checks) {
    `$name = [string]`$check[0]
    `$argument = [string]`$check[1]
    `$expectedPath = Decode-AfhValue ([string]`$check[2])
    `$expectedVersion = Decode-AfhValue ([string]`$check[3])
    `$resolved = Get-Command -Name `$name -CommandType Application,ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not `$resolved) { exit 41 }
    `$resolvedPath = [IO.Path]::GetFullPath([string]`$resolved.Source)
    if (-not [string]::Equals(`$resolvedPath, `$expectedPath, [StringComparison]::OrdinalIgnoreCase)) { exit 42 }
    `$actualVersion = ((& `$resolved.Source `$argument 2>`$null) -join "`n")
    if (`$LASTEXITCODE -ne 0) { exit 42 }
    if (`$actualVersion -ne `$expectedVersion) { exit 43 }
    if (`$name -eq "rustc") {
        `$verbose = ((& `$resolved.Source -vV 2>`$null) -join "`n")
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
        Stop-Gate 24 "新的 PowerShell 无法从持久 User/Machine PATH 复探全部受管工具"
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

# 验证 Node.js 落在 Vite 基线范围内；低版本或 25.x 返回待升级状态。
function Test-NodeVersion {
    param([string]$NodePath)
    $nodeText = (& $NodePath --version 2>$null)
    if ($LASTEXITCODE -ne 0) { Stop-Gate 23 "Node.js 探测失败" }
    if ($nodeText -notmatch '^v(\d+)\.(\d+)\.(\d+)$') {
        Stop-Gate 23 "现有 Node.js 不是可识别的稳定发布版：$nodeText"
    }
    $major = [int]$Matches[1]
    $minor = [int]$Matches[2]
    $patch = [int]$Matches[3]
    $compatible = ($major -eq 24 -and ($minor -gt 15 -or ($minor -eq 15 -and $patch -ge 0))) -or
        ($major -ge 26)
    $script:NodeVersion = $nodeText
    if (-not $compatible) { return "upgrade-required" }
    return "passed"
}

# Node 路线同时要求可解析的稳定 npm，保证全局安装 pnpm 与新会话复探使用同一工具。
function Test-NpmVersion {
    param([string]$NpmPath)
    $npmText = (& $NpmPath --version 2>$null)
    if ($LASTEXITCODE -ne 0) { Stop-Gate 23 "npm 探测失败" }
    if ($npmText -notmatch '^(\d+)\.(\d+)\.(\d+)$') { Stop-Gate 23 "现有 npm 不是可识别的稳定发布版：$npmText" }
    $script:NpmVersion = $npmText
}

# 验证 pnpm 满足最低版本；明确低于下界时返回待升级状态。
function Test-PnpmVersion {
    param([string]$PnpmPath)
    $pnpmText = (& $PnpmPath --version 2>$null)
    if ($LASTEXITCODE -ne 0) { Stop-Gate 28 "pnpm 探测失败" }
    if ($pnpmText -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
        Stop-Gate 28 "现有 pnpm 不是可识别的稳定发布版：$pnpmText"
    }
    $major = [int]$Matches[1]
    $minor = [int]$Matches[2]
    $script:PnpmVersion = $pnpmText
    if ($major -lt 11 -or ($major -eq 11 -and $minor -lt 24)) {
        return "upgrade-required"
    }
    return "passed"
}

# Git 是所有初始化路径的基础工具；明确低于下界时返回待升级状态。
function Test-GitVersion {
    param([string]$GitPath)
    $gitText = (& $GitPath --version 2>$null)
    if ($LASTEXITCODE -ne 0) { Stop-Gate 29 "Git 探测失败" }
    if ($gitText -notmatch '^git version (\d+)\.(\d+)\.(\d+)(?:\.windows\.\d+)?(?:\s.*)?$') {
        Stop-Gate 29 "现有 Git 不是可识别的稳定发布版：$gitText"
    }
    $script:GitVersion = $gitText
    $gitMajor = [int]$Matches[1]
    $gitMinor = [int]$Matches[2]
    if ($gitMajor -lt 2) { return "upgrade-required" }
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
        Initialize-ManagedDirectoryPath $script:ManagedCargoHome $UserProfileRoot "Rust Cargo 受管安装根"
        Initialize-ManagedDirectoryPath $script:ManagedRustupHome $UserProfileRoot "Rust rustup 受管安装根"
        Invoke-ManagedRustCommand $rustup @("toolchain", "install", "stable", "--profile", "minimal") | Out-Null
        if ($script:ManagedRustExitCode -ne 0) { Stop-Gate 22 "Rust stable 工具链升级失败" }
        Invoke-ManagedRustCommand $rustup @("default", "stable") | Out-Null
        if ($script:ManagedRustExitCode -ne 0) { Stop-Gate 22 "Rust stable 默认工具链切换失败" }
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
    [Console]::Error.WriteLine("正在从 $releaseBase $action Rust stable 到当前用户的 rustup 目录。")
    Get-OfficialFile "$releaseBase/rustup-init.exe" $installer
    Get-OfficialFile "$releaseBase/rustup-init.exe.sha256" $checksumFile
    $expected = ((Get-Content -LiteralPath $checksumFile -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
    $actual = Get-Sha256File $installer
    if ($actual -ne $expected) { Stop-Gate 22 "rustup-init SHA-256 校验失败" }
    Initialize-ManagedDirectoryPath $script:ManagedCargoHome $UserProfileRoot "Rust Cargo 受管安装根"
    Initialize-ManagedDirectoryPath $script:ManagedRustupHome $UserProfileRoot "Rust rustup 受管安装根"
    Invoke-ManagedRustCommand $installer @("-y", "--profile", "minimal", "--default-toolchain", "stable", "--no-modify-path") | Out-Null
    if ($script:ManagedRustExitCode -ne 0) { Stop-Gate 22 "Rust 安装失败" }
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

# 从官方倒序索引选择当前最新合格稳定版，校验 zip 后安装/升级当前用户环境。
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
    $release = @($index | Where-Object {
        $candidate = [string]$_.version
        $compatible = $false
        if ($candidate -match '^v(\d+)\.(\d+)\.(\d+)$') {
            $major = [int]$Matches[1]
            $minor = [int]$Matches[2]
            $patch = [int]$Matches[3]
            $compatible = ($major -eq 24 -and ($minor -gt 15 -or ($minor -eq 15 -and $patch -ge 0))) -or ($major -ge 26)
        }
        $compatible
    }) | Select-Object -First 1
    if (-not $release) { Stop-Gate 26 "Node.js 发布版本索引中没有满足 $NodeRequirement 的稳定版" }
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
        $installedVersion = (& $nodeExecutable --version 2>$null)
        if ($LASTEXITCODE -ne 0 -or $installedVersion -ne $version) {
            Stop-Gate 26 "Node.js 目标版本与已选择稳定版不一致：期望 $version，实际 $installedVersion"
        }
    } else {
        [Console]::Error.WriteLine("正在从 $releaseBase $action 当前最新合格稳定 Node.js $version 到用户级目录。")
        Get-OfficialFile "$releaseBase/$archiveName" $archive
        $actual = Get-Sha256File $archive
        if ($actual -ne $expected) { Stop-Gate 26 "Node.js SHA-256 校验失败" }
        Initialize-ManagedDirectoryPath $script:ManagedNodeHome $LocalAppDataRoot "Node.js 受管安装根"
        Expand-Archive -LiteralPath $archive -DestinationPath $temporary
        $extracted = Join-Path $temporary "node-$version-win-$nodeArchitecture"
        $nodeExecutable = Get-NodeExecutableInDirectory $extracted
        if (-not $nodeExecutable) {
            Stop-Gate 26 "Node.js 归档不包含预期可执行文件"
        }
        $extractedVersion = (& $nodeExecutable --version 2>$null)
        if ($LASTEXITCODE -ne 0 -or $extractedVersion -ne $version) {
            Stop-Gate 26 "Node.js 归档版本与已选择稳定版不一致：期望 $version，实际 $extractedVersion"
        }
        Move-Item -LiteralPath $extracted -Destination $installDirectory
        [IO.File]::WriteAllText(
            (Join-Path $installDirectory ".agent-first-harness-managed"),
            "$markerContent`n",
            [Text.UTF8Encoding]::new($false)
        )
    }
    $script:NodeBin = $installDirectory
    $script:SelectedNodeVersion = $version
    Add-ProbePathEntry $NodeBin
    Add-PersistedUserPathEntries -Entries @($NodeBin) -OwnedRoots @($script:ManagedNodeHome)
    $script:NodeChange = $Change
}

# 仅为 GUI 项目通过 Node 自带 npm 安装或升级满足最低要求的 pnpm。
function Install-MissingPnpm {
    param([ValidateSet("installed", "upgraded")][string]$Change)
    $npm = Resolve-GateCommand "npm"
    if (-not $npm) { Stop-Gate 28 "为 GUI 开发安装 pnpm 需要 npm" }
    Initialize-ManagedDirectoryPath $script:ManagedPnpmHome $LocalAppDataRoot "pnpm 受管安装根"
    $action = if ($Change -eq "upgraded") { "升级" } else { "安装" }
    [Console]::Error.WriteLine("正在从官方 npm 软件包仓库把 $PnpmInstallRequirement $action 到用户级目录。")
    if ([IO.Path]::GetExtension($npm) -in @(".cmd", ".bat")) {
        # PowerShell 5.1 调用批处理包装器时不会自动保护 `>`；显式保留引号，避免版本范围被 cmd.exe 当作重定向。
        & $npm install --global --prefix $script:ManagedPnpmHome ('"' + $PnpmInstallRequirement + '"') --registry $PnpmRegistry --ignore-scripts
    } else {
        & $npm install --global --prefix $script:ManagedPnpmHome $PnpmInstallRequirement --registry $PnpmRegistry --ignore-scripts
    }
    if ($LASTEXITCODE -ne 0) { Stop-Gate 28 "pnpm 安装失败" }
    $script:PnpmBin = $script:ManagedPnpmHome
    Add-ProbePathEntry $PnpmBin
    Add-PersistedUserPathEntries -Entries @($PnpmBin) -OwnedRoots @($script:ManagedPnpmHome)
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
    $pnpm = Resolve-GateCommand "pnpm"
    if ($node -and $npm) {
        $nodeState = Test-NodeVersion $node
        Test-NpmVersion $npm
    } else {
        $NodeVersion = "Missing"
        $NpmVersion = "Missing"
        $nodeState = "missing"
    }
    if ($pnpm) {
        $pnpmState = Test-PnpmVersion $pnpm
    } else {
        $PnpmVersion = "Missing"
        $pnpmState = "missing"
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
        if ($gitState -ne "passed" -or $rustState -ne "passed" -or $nodeState -ne "passed" -or $pnpmState -ne "passed" -or $msvcMissing) { exit 20 }
        exit 0
    }

    # 所有将使用的用户安装根在任何 winget、下载器或安装器启动前一次性预检。
    if ($rustState -ne "passed") {
        Assert-ManagedDirectoryPath $script:ManagedCargoHome $UserProfileRoot "Rust Cargo 受管安装根"
        Assert-ManagedDirectoryPath $script:ManagedRustupHome $UserProfileRoot "Rust rustup 受管安装根"
    }
    if ($nodeState -ne "passed") {
        Assert-ManagedDirectoryPath $script:ManagedNodeHome $LocalAppDataRoot "Node.js 受管安装根"
    }
    if ($pnpmState -ne "passed") {
        Assert-ManagedDirectoryPath $script:ManagedPnpmHome $LocalAppDataRoot "pnpm 受管安装根"
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
        if ($rustState -ne "passed") { Stop-Gate 22 "Rust 安装或升级后仍低于 MSRV $MinimumRustMajor.$MinimumRustMinor.0" }
    }
    if ($msvcMissing) {
        Install-MissingMsvc
        if (-not (Test-MsvcPrerequisite)) {
            Stop-Gate 27 "Visual Studio Build Tools 安装完成后 MSVC C++ 工作负载仍不可用"
        }
    }
    if ($nodeState -ne "passed") {
        $change = if ($nodeState -eq "missing") { "installed" } else { "upgraded" }
        Install-MissingNode $change
        $node = Resolve-GateCommand "node"
        $npm = Resolve-GateCommand "npm"
        if (-not $node -or -not $npm) { Stop-Gate 26 "Node.js 安装完成后仍无法调用 node 和 npm" }
        $nodeState = Test-NodeVersion $node
        Test-NpmVersion $npm
        if ($nodeState -ne "passed") { Stop-Gate 26 "Node.js 安装或升级后仍不满足 $NodeRequirement" }
        if ($NodeVersion -ne $script:SelectedNodeVersion) {
            Stop-Gate 26 "Node.js 安装或升级后的版本与已选择稳定版不一致：期望 $($script:SelectedNodeVersion)，实际 $NodeVersion"
        }
    }
    if ($pnpmState -ne "passed") {
        $change = if ($pnpmState -eq "missing") { "installed" } else { "upgraded" }
        Install-MissingPnpm $change
        $pnpm = Resolve-GateCommand "pnpm"
        if (-not $pnpm) { Stop-Gate 28 "pnpm 安装完成后仍无法调用 pnpm 可执行文件" }
        $pnpmState = Test-PnpmVersion $pnpm
        if ($pnpmState -ne "passed") { Stop-Gate 28 "pnpm 安装或升级后仍不满足 $PnpmRequirement" }
    }

    $changed = if (@($GitChange, $RustChange, $NodeChange, $PnpmChange, $MsvcChange) | Where-Object { $_ -in @("installed", "upgraded") }) { "true" } else { "false" }
    $freshShellStatus = "not-required"
    $persistenceEnabled = -not ($TestMode -and $env:AFH_SKIP_PERSIST_PATH -eq "1")
    if ($changed -eq "true" -and $persistenceEnabled) {
        Test-FreshPowerShellToolDiscovery
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
    "gate.pnpm.status=passed"
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
