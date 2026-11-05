[CmdletBinding()]
param([switch] $Apply)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$target = Join-Path $repository 'target'
$release = Join-Path $target 'release'

function Assert-RegularDirectory([string] $Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Expected a regular workspace directory: $Path"
    }
}

function Measure-Release {
    $bytes = 0L
    $files = 0L
    $pending = [Collections.Generic.Stack[string]]::new()
    $pending.Push($release)
    while ($pending.Count -gt 0) {
        $directory = $pending.Pop()
        Assert-RegularDirectory $directory
        foreach ($item in Get-ChildItem -LiteralPath $directory -Force) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Refusing to clean a release tree containing a reparse point: $($item.FullName)"
            }
            if ($item.PSIsContainer) { $pending.Push($item.FullName) }
            else { $bytes += $item.Length; $files++ }
        }
    }
    [ordered]@{ bytes = $bytes; files = $files }
}

function Assert-IdleBuild {
    $builders = @(Get-Process cargo,rustc,link,makensis -ErrorAction SilentlyContinue)
    if ($builders.Count -gt 0) {
        $active = ($builders | ForEach-Object { "$($_.Name):$($_.Id)" }) -join ', '
        throw "A Rust build or installer process is active. Finish it before cleaning. Processes: $active"
    }
    foreach ($process in Get-Process voldiq,voldiq-cli -ErrorAction SilentlyContinue) {
        if (-not $process.Path) { throw 'Cannot determine a running Voldiq image; cleanup stopped.' }
        $image = [IO.Path]::GetFullPath($process.Path)
        if ($image.StartsWith($release + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'A release executable is running. Use a hash-addressed preview before cleaning.'
        }
    }
    $lockPath = Join-Path $release '.cargo-lock'
    if (Test-Path -LiteralPath $lockPath) {
        $lock = [IO.File]::Open($lockPath, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        $lock.Dispose()
    }
}

foreach ($directory in @($repository, $target, $release)) { Assert-RegularDirectory $directory }
if ([IO.Path]::GetDirectoryName($target) -ne $repository -or [IO.Path]::GetDirectoryName($release) -ne $target) {
    throw 'The release target must be inside this workspace.'
}
Assert-IdleBuild
$before = Measure-Release
$configuration = Get-Content -LiteralPath (Join-Path $repository 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
if ($configuration.version -notmatch '^\d+\.\d+\.\d+$') { throw 'Unexpected distribution version.' }
$relativePaths = @('voldiq.exe', 'voldiq-cli.exe', "bundle/nsis/Voldiq_$($configuration.version)_x64-setup.exe", 'nsis/x64/installer.nsi')
$preserved = @(foreach ($relative in $relativePaths) {
    $path = Join-Path $release $relative
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Build the deliverable before cleaning: $relative" }
    $file = Get-Item -LiteralPath $path
    [ordered]@{ path = $relative; bytes = $file.Length; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash }
})

$cargoOutput = @()
if ($Apply) {
    $backup = Join-Path $target ('cache-preserve-' + [Guid]::NewGuid().ToString('N'))
    if (Test-Path -LiteralPath $backup) { throw 'Preservation directory already exists.' }
    [IO.Directory]::CreateDirectory($backup) | Out-Null
    $restored = $false
    $cleanAttempted = $false
    try {
        foreach ($file in $preserved) {
            $copy = Join-Path $backup $file.path
            [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($copy)) | Out-Null
            Copy-Item -LiteralPath (Join-Path $release $file.path) -Destination $copy
            if ((Get-FileHash -LiteralPath $copy -Algorithm SHA256).Hash -ne $file.sha256) { throw 'Preserved artifact hash differs.' }
        }
        Assert-IdleBuild
        $null = Measure-Release
        $cleanAttempted = $true
        $cargoOutput = @(& cargo clean --offline --release --manifest-path (Join-Path $repository 'Cargo.toml') --target-dir $target 2>&1 | ForEach-Object { "$_" })
        if ($LASTEXITCODE -ne 0) { throw 'Cargo release cleanup failed; preserved files remain in the backup.' }
    } finally {
        # A pre-clean abort must not overwrite a build that started after preservation.
        if ($cleanAttempted) {
            foreach ($file in $preserved) {
                $copy = Join-Path $backup $file.path
                if (-not (Test-Path -LiteralPath $copy)) { continue }
                if ((Get-FileHash -LiteralPath $copy -Algorithm SHA256).Hash -ne $file.sha256) { throw "Invalid backup retained at $backup" }
                $destination = Join-Path $release $file.path
                [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
                Copy-Item -LiteralPath $copy -Destination $destination -Force
                if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $file.sha256) { throw "Restore failed; backup retained at $backup" }
            }
            $restored = $true
        }
    }
    if ($restored) {
        foreach ($file in $preserved) { Remove-Item -LiteralPath (Join-Path $backup $file.path) }
        [IO.Directory]::Delete((Join-Path $backup 'bundle/nsis'), $false)
        [IO.Directory]::Delete((Join-Path $backup 'bundle'), $false)
        [IO.Directory]::Delete((Join-Path $backup 'nsis/x64'), $false)
        [IO.Directory]::Delete((Join-Path $backup 'nsis'), $false)
        [IO.Directory]::Delete($backup, $false)
    }
}
$after = Measure-Release
[ordered]@{
    checkedUtc = [DateTime]::UtcNow.ToString('o')
    applied = [bool]$Apply
    scope = 'Cargo release artifacts in this workspace only. Logical file bytes, not physical allocation or observed volume free-space change.'
    target = $release
    before = $before
    after = $after
    removedBytes = $before.bytes - $after.bytes
    preserved = $preserved
    cargoOutput = $cargoOutput
    otherCachesTouched = $false
} | ConvertTo-Json -Depth 5
