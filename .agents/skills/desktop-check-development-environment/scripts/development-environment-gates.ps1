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
        foreach ($candidateName in @($Name, "$Name.exe", "$Name.cmd", "$Name.bat")) {
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
    if ($Uri.StartsWith("file://", [StringComparison]::OrdinalIgnoreCase) -and $env:AFH_ALLOW_FILE_URLS -eq "1") {
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

# 验证 Rust 为满足 MSRV 的稳定版本，不自动覆盖旧版或预发布工具链。
function Test-RustVersion {
    param([string]$RustcPath, [string]$CargoPath)
    $rustText = (& $RustcPath --version 2>$null)
    if ($LASTEXITCODE -ne 0) { Stop-Gate 21 "rustc 探测失败" }
    $cargoText = (& $CargoPath --version 2>$null)
    if ($LASTEXITCODE -ne 0) { Stop-Gate 21 "cargo 探测失败" }
    if ($rustText -notmatch '^rustc (\d+)\.(\d+)\.\d+(?:\s|$)') {
        Stop-Gate 21 "现有 Rust 工具链不是可识别的稳定发布版：$rustText"
    }
    $rustMajor = [int]$Matches[1]
    $rustMinor = [int]$Matches[2]
    if ($rustMajor -lt $MinimumRustMajor -or ($rustMajor -eq $MinimumRustMajor -and $rustMinor -lt $MinimumRustMinor)) {
        Stop-Gate 21 "现有 Rust 低于 MSRV $MinimumRustMajor.$MinimumRustMinor.0：$rustText"
    }
    $script:RustVersion = $rustText
    $script:CargoVersion = $cargoText
}

# 验证 Node.js 落在 Vite 基线的非连续兼容范围内，拒绝低版本与 25.x 空档。
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
    if (-not $compatible) { Stop-Gate 23 "现有 Node.js $nodeText 不满足兼容范围 $NodeRequirement" }
    $script:NodeVersion = $nodeText
}

# 验证 pnpm 满足最低兼容版本；已存在的更高稳定版本保持不变。
function Test-PnpmVersion {
    param([string]$PnpmPath)
    $pnpmText = (& $PnpmPath --version 2>$null)
    if ($LASTEXITCODE -ne 0) { Stop-Gate 28 "pnpm 探测失败" }
    if ($pnpmText -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
        Stop-Gate 28 "现有 pnpm 不是可识别的稳定发布版：$pnpmText"
    }
    $major = [int]$Matches[1]
    $minor = [int]$Matches[2]
    if ($major -lt 11 -or ($major -eq 11 -and $minor -lt 24)) {
        Stop-Gate 28 "现有 pnpm $pnpmText 低于兼容下界 11.24.0"
    }
    $script:PnpmVersion = $pnpmText
}

# Git 是所有初始化路径的基础工具；满足下界的现有稳定版本（包括更高版本）保持不变。
function Test-GitVersion {
    param([string]$GitPath)
    $gitText = (& $GitPath --version 2>$null)
    if ($LASTEXITCODE -ne 0) { Stop-Gate 29 "Git 探测失败" }
    if ($gitText -notmatch '^git version (\d+)\.(\d+)\.(\d+)(?:\.windows\.\d+)?(?:\s.*)?$') {
        Stop-Gate 29 "现有 Git 不是可识别的稳定发布版：$gitText"
    }
    if ([int]$Matches[1] -lt 2) { Stop-Gate 29 "现有 Git 低于兼容下界 2.0.0：$gitText" }
    $script:GitVersion = $gitText
}

# Windows 只通过既有 winget 的受管 Git.Git 软件包安装，并在当前进程刷新常见 Git 路径。
function Install-MissingGit {
    $winget = Resolve-GateCommand "winget"
    if (-not $winget) { Stop-Gate 29 "Windows 安装 Git 需要既有 winget" }
    [Console]::Error.WriteLine("正在通过既有 winget 安装缺失的 Git.Git。")
    & $winget install --id Git.Git --exact --silent --disable-interactivity --accept-package-agreements --accept-source-agreements
    if ($LASTEXITCODE -ne 0) { Stop-Gate 29 "winget 安装 Git.Git 失败" }
    $candidateBins = @(
        (Join-Path $env:ProgramFiles "Git\cmd"),
        (Join-Path $env:LOCALAPPDATA "Programs\Git\cmd")
    )
    foreach ($candidateBin in $candidateBins) {
        if (Test-Path -LiteralPath (Join-Path $candidateBin "git.exe") -PathType Leaf) {
            $script:GitBin = $candidateBin
            $script:ProbePath = "$GitBin$([IO.Path]::PathSeparator)$ProbePath"
            $env:PATH = $script:ProbePath
            break
        }
    }
    $script:GitChange = "installed"
}

# 下载架构匹配的官方 rustup-init，核对 SHA-256 后安装缺失 stable 工具链。
function Install-MissingRust {
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
    [Console]::Error.WriteLine("正在从 $releaseBase 把缺失的 Rust stable 安装到当前用户的 rustup 目录。")
    Get-OfficialFile "$releaseBase/rustup-init.exe" $installer
    Get-OfficialFile "$releaseBase/rustup-init.exe.sha256" $checksumFile
    $expected = ((Get-Content -LiteralPath $checksumFile -Raw).Trim() -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { Stop-Gate 22 "rustup-init SHA-256 校验失败" }
    & $installer -y --profile minimal --default-toolchain stable
    if ($LASTEXITCODE -ne 0) { Stop-Gate 22 "Rust 安装失败" }
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $HOME ".cargo" }
    $script:CargoBin = Join-Path $cargoHome "bin"
    $script:ProbePath = "$CargoBin$([IO.Path]::PathSeparator)$ProbePath"
    $script:RustChange = "installed"
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
    if ($env:AFH_SKIP_AUTHENTICODE -ne "1") {
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

# 从官方倒序索引选择当前最新兼容稳定版，校验 zip 后安装到当前用户目录并维护用户 PATH。
function Install-MissingNode {
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
    [Console]::Error.WriteLine("正在从 $releaseBase 把缺失的最新兼容稳定 Node.js $version 安装到用户级目录。")
    Get-OfficialFile "$releaseBase/$archiveName" $archive
    Get-OfficialFile "$releaseBase/SHASUMS256.txt" $checksums
    $escapedName = [Regex]::Escape($archiveName)
    $checksumLine = Get-Content -LiteralPath $checksums | Where-Object { $_ -match "^([0-9a-fA-F]{64})\s+$escapedName$" } | Select-Object -First 1
    if (-not $checksumLine) { Stop-Gate 26 "Node.js 校验和列表不包含 $archiveName" }
    $checksumLine -match '^([0-9a-fA-F]{64})' | Out-Null
    $expected = $Matches[1].ToLowerInvariant()
    $actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { Stop-Gate 26 "Node.js SHA-256 校验失败" }
    $nodeHome = if ($env:AFH_NODE_HOME) { $env:AFH_NODE_HOME } else { Join-Path $env:LOCALAPPDATA "AgentFirstHarness\Node" }
    $installDirectory = Join-Path $nodeHome $version
    if (Test-Path -LiteralPath $installDirectory) {
        if (-not (Test-Path -LiteralPath (Join-Path $installDirectory "node.exe") -PathType Leaf)) {
            Stop-Gate 26 "Node.js 目标已存在但不可用：$installDirectory"
        }
    } else {
        New-Item -ItemType Directory -Force -Path $nodeHome | Out-Null
        Expand-Archive -LiteralPath $archive -DestinationPath $temporary
        $extracted = Join-Path $temporary "node-$version-win-$nodeArchitecture"
        if (-not (Test-Path -LiteralPath (Join-Path $extracted "node.exe") -PathType Leaf)) {
            Stop-Gate 26 "Node.js 归档不包含预期可执行文件"
        }
        Move-Item -LiteralPath $extracted -Destination $installDirectory
    }
    $script:NodeBin = $installDirectory
    $script:ProbePath = "$NodeBin$([IO.Path]::PathSeparator)$ProbePath"
    $env:PATH = $script:ProbePath
    if ($env:AFH_SKIP_PERSIST_PATH -ne "1") {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $parts = @($userPath -split [IO.Path]::PathSeparator | Where-Object { $_ })
        if ($parts -notcontains $NodeBin) {
            $newUserPath = (@($NodeBin) + $parts) -join [IO.Path]::PathSeparator
            [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
        }
    }
    $script:NodeChange = "installed"
}

# 仅为 GUI 项目通过 Node 自带 npm 安装满足最低要求的兼容 pnpm 范围。
function Install-MissingPnpm {
    $npm = Resolve-GateCommand "npm"
    if (-not $npm) { Stop-Gate 28 "为 GUI 开发安装 pnpm 需要 npm" }
    $pnpmHome = if ($env:AFH_PNPM_HOME) { $env:AFH_PNPM_HOME } else { Join-Path $env:LOCALAPPDATA "AgentFirstHarness\Pnpm" }
    New-Item -ItemType Directory -Force -Path $pnpmHome | Out-Null
    [Console]::Error.WriteLine("正在从官方 npm 软件包仓库把缺失的 $PnpmInstallRequirement 安装到用户级目录。")
    & $npm install --global --prefix $pnpmHome $PnpmInstallRequirement
    if ($LASTEXITCODE -ne 0) { Stop-Gate 28 "pnpm 安装失败" }
    $script:PnpmBin = $pnpmHome
    $script:ProbePath = "$PnpmBin$([IO.Path]::PathSeparator)$ProbePath"
    $env:PATH = $script:ProbePath
    $script:PnpmChange = "installed"
}

try {
    $git = Resolve-GateCommand "git"
    $rustc = Resolve-GateCommand "rustc"
    $cargo = Resolve-GateCommand "cargo"

    if ($git) {
        Test-GitVersion $git
        $gitMissing = $false
    } else {
        $GitVersion = "Missing"
        $gitMissing = $true
    }

    if ($rustc -and $cargo) {
        Test-RustVersion $rustc $cargo
        $rustMissing = $false
    } else {
        $RustVersion = "Missing"
        $CargoVersion = "Missing"
        $rustMissing = $true
    }
    $node = Resolve-GateCommand "node"
    $pnpm = Resolve-GateCommand "pnpm"
    if ($node) {
        Test-NodeVersion $node
        $nodeMissing = $false
    } else {
        $NodeVersion = "Missing"
        $nodeMissing = $true
    }
    if ($pnpm) {
        Test-PnpmVersion $pnpm
        $pnpmMissing = $false
    } else {
        $PnpmVersion = "Missing"
        $pnpmMissing = $true
    }
    $msvcMissing = -not (Test-MsvcPrerequisite)

    if ($CheckOnly) {
        "gate.git.status=$(if ($gitMissing) { 'missing' } else { 'passed' })"
        "gate.git.requirement=$GitRequirement"
        "gate.git.version=$GitVersion"
        "gate.rust.status=$(if ($rustMissing) { 'missing' } else { 'passed' })"
        "gate.rust.version=$RustVersion"
        "gate.node.status=$(if ($nodeMissing) { 'missing' } else { 'passed' })"
        "gate.node.requirement=$NodeRequirement"
        "gate.node.version=$NodeVersion"
        "gate.pnpm.status=$(if ($pnpmMissing) { 'missing' } else { 'passed' })"
        "gate.pnpm.requirement=$PnpmRequirement"
        "gate.pnpm.version=$PnpmVersion"
        "gate.msvc.status=$(if ($msvcMissing) { 'missing' } else { 'passed' })"
        if ($gitMissing -or $rustMissing -or $nodeMissing -or $pnpmMissing -or $msvcMissing) { exit 20 }
        exit 0
    }

    if ($gitMissing) {
        Install-MissingGit
        $git = Resolve-GateCommand "git"
        if (-not $git) { Stop-Gate 29 "Git 安装完成后仍无法调用 git 可执行文件" }
        Test-GitVersion $git
    }
    if ($rustMissing) {
        Install-MissingRust
        $rustc = Resolve-GateCommand "rustc"
        $cargo = Resolve-GateCommand "cargo"
        if (-not $rustc -or -not $cargo) { Stop-Gate 22 "Rust 安装完成后仍无法调用 rustc 和 cargo" }
        Test-RustVersion $rustc $cargo
    }
    if ($msvcMissing) {
        Install-MissingMsvc
        if (-not (Test-MsvcPrerequisite)) {
            Stop-Gate 27 "Visual Studio Build Tools 安装完成后 MSVC C++ 工作负载仍不可用"
        }
    }
    if ($nodeMissing) {
        Install-MissingNode
        $node = Resolve-GateCommand "node"
        if (-not $node) { Stop-Gate 26 "Node.js 安装完成后仍无法调用 node 可执行文件" }
        Test-NodeVersion $node
    }
    if ($pnpmMissing) {
        Install-MissingPnpm
        $pnpm = Resolve-GateCommand "pnpm"
        if (-not $pnpm) { Stop-Gate 28 "pnpm 安装完成后仍无法调用 pnpm 可执行文件" }
        Test-PnpmVersion $pnpm
    }

    $changed = if ($GitChange -eq "installed" -or $RustChange -eq "installed" -or $NodeChange -eq "installed" -or $PnpmChange -eq "installed" -or $MsvcChange -eq "installed") { "true" } else { "false" }
    "gate.git.status=passed"
    "gate.git.requirement=$GitRequirement"
    "gate.git.version=$GitVersion"
    "gate.git.change=$GitChange"
    "gate.rust.status=passed"
    "gate.rust.version=$RustVersion"
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
    $prepend = (@($PnpmBin, $NodeBin, $CargoBin, $GitBin) | Where-Object { $_ }) -join [IO.Path]::PathSeparator
    if ($prepend) { "gate.path.prepend=$prepend" }
} finally {
    foreach ($directory in $TemporaryDirectories) {
        if (Test-Path -LiteralPath $directory) { Remove-Item -LiteralPath $directory -Recurse -Force }
    }
}
