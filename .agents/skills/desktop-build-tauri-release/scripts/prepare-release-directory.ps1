param(
    [Parameter(Mandatory = $false)]
    [string]$ProjectRoot = "."
)

# 在独立 Git 项目根中原子替换精确 release 目录，并拒绝 Windows 重解析点。
$ErrorActionPreference = "Stop"

function Remove-TreeWithoutFollowingReparsePoint {
    param([Parameter(Mandatory = $true)][string]$LiteralPath)

    if (-not (Test-Path -LiteralPath $LiteralPath)) {
        return
    }
    $item = Get-Item -LiteralPath $LiteralPath -Force
    if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        if ($item.PSIsContainer) {
            [IO.Directory]::Delete($item.FullName, $false)
        } else {
            [IO.File]::Delete($item.FullName)
        }
        return
    }
    if ($item.PSIsContainer) {
        foreach ($child in @(Get-ChildItem -LiteralPath $item.FullName -Force)) {
            Remove-TreeWithoutFollowingReparsePoint -LiteralPath $child.FullName
        }
    }
    Remove-Item -LiteralPath $item.FullName -Force -ErrorAction Stop
}

$canonicalRoot = (Resolve-Path -LiteralPath $ProjectRoot).Path
$gitTopOutput = @(& git -C $canonicalRoot rev-parse --show-toplevel 2>$null)
$gitTopExitCode = $LASTEXITCODE
$gitTop = $gitTopOutput | Select-Object -First 1
if ($gitTopExitCode -ne 0 -or [string]::IsNullOrWhiteSpace($gitTop)) {
    throw "release 准备失败：项目根目录不在 Git 仓库中"
}
$canonicalGitTop = (Resolve-Path -LiteralPath $gitTop.Trim()).Path
if (-not [StringComparer]::OrdinalIgnoreCase.Equals($canonicalGitTop, $canonicalRoot)) {
    throw "release 准备失败：项目根目录不是独立 Git 顶层目录"
}
$releasePath = Join-Path $canonicalRoot "release"
if (Test-Path -LiteralPath $releasePath) {
    $releaseItem = Get-Item -LiteralPath $releasePath -Force
    if (-not $releaseItem.PSIsContainer) {
        throw "release 准备失败：release 已存在但不是目录"
    }
    if (($releaseItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "release 准备失败：release 是重解析点"
    }
}
$sourceCommitOutput = @(& git -C $canonicalRoot rev-parse --verify 'HEAD^{commit}' 2>$null)
$sourceCommitExitCode = $LASTEXITCODE
$sourceCommit = $sourceCommitOutput | Select-Object -First 1
if ($sourceCommitExitCode -ne 0 -or [string]::IsNullOrWhiteSpace($sourceCommit)) {
    throw "release 准备失败：HEAD 不能解析为源码提交"
}
$sourceCommit = $sourceCommit.Trim()
if ($sourceCommit -cnotmatch '^[0-9a-f]{40}$') {
    throw "release 准备失败：HEAD 必须是 40 位小写源码提交"
}
$initialStatus = @(& git -C $canonicalRoot status --porcelain=v1 --untracked-files=all)
if ($LASTEXITCODE -ne 0) { throw "release 准备失败：无法检查工作树状态" }
if ($initialStatus.Count -ne 0) {
    throw "release 准备失败：工作树不干净；请先完成并提交发布范围"
}

$stagingParent = Join-Path $canonicalRoot (".release-clean." + [Guid]::NewGuid().ToString("N"))
[IO.Directory]::CreateDirectory($stagingParent) | Out-Null
try {
    if (Test-Path -LiteralPath $releasePath) {
        [IO.Directory]::Move($releasePath, (Join-Path $stagingParent "previous-release"))
    }

    if (Test-Path -LiteralPath $releasePath) {
        throw "release 准备失败：原子刷新期间 release 发生变化"
    }
    [IO.Directory]::CreateDirectory($releasePath) | Out-Null

    $releaseItem = Get-Item -LiteralPath $releasePath -Force
    if (($releaseItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "release 准备失败：刷新后的 release 是重解析点"
    }
    $canonicalRelease = (Resolve-Path -LiteralPath $releasePath).Path
    $expectedRelease = [IO.Path]::GetFullPath((Join-Path $canonicalRoot "release"))
    if (-not [StringComparer]::OrdinalIgnoreCase.Equals($canonicalRelease, $expectedRelease)) {
        throw "release 准备失败：release 越出项目根目录"
    }
    if (@(Get-ChildItem -LiteralPath $canonicalRelease -Force).Count -ne 0) {
        throw "release 准备失败：刷新后的 release 不为空"
    }

    Remove-TreeWithoutFollowingReparsePoint -LiteralPath $stagingParent
    $finalCommitOutput = @(& git -C $canonicalRoot rev-parse --verify 'HEAD^{commit}' 2>$null)
    $finalCommitExitCode = $LASTEXITCODE
    $finalCommit = $finalCommitOutput | Select-Object -First 1
    if ($finalCommitExitCode -ne 0 -or [string]::IsNullOrWhiteSpace($finalCommit)) {
        throw "release 准备失败：清理后无法复核 HEAD"
    }
    $finalStatus = @(& git -C $canonicalRoot status --porcelain=v1 --untracked-files=all)
    if ($LASTEXITCODE -ne 0) { throw "release 准备失败：清理后无法复核工作树状态" }
    if ($finalCommit.Trim() -cne $sourceCommit -or $finalStatus.Count -ne 0) {
        throw "release 准备失败：清理期间 HEAD 或工作树发生变化"
    }

    Write-Output "release.path=$canonicalRelease"
    Write-Output "release.cleaned=true"
    Write-Output "release.source_commit=$sourceCommit"
} finally {
    Remove-TreeWithoutFollowingReparsePoint -LiteralPath $stagingParent
}
