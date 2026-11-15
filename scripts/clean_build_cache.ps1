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

function Assert-NoRunningReleaseImage {
    foreach ($process in Get-Process voldiq,voldiq-cli -ErrorAction SilentlyContinue) {
        if (-not $process.Path) { throw 'Cannot determine a running Voldiq image; cleanup stopped.' }
        $image = [IO.Path]::GetFullPath($process.Path)
        if ($image.StartsWith($release + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'A release executable is running. Use a hash-addressed preview before cleaning.'
        }
    }
}

foreach ($directory in @($repository, $target, $release)) { Assert-RegularDirectory $directory }
if ([IO.Path]::GetDirectoryName($target) -ne $repository -or [IO.Path]::GetDirectoryName($release) -ne $target) {
    throw 'The release target must be inside this workspace.'
}

$locks = [Collections.Generic.List[IO.FileStream]]::new()
$removed = [Collections.Generic.List[string]]::new()
try {
    Assert-NoRunningReleaseImage
    # Cargo holds these files open while using this build or artifact directory.
    # Hold Windows-exclusive handles through all deletion and verification.
    foreach ($name in @('.cargo-lock', '.cargo-artifact-lock', '.cargo-build-lock')) {
        $path = Join-Path $release $name
        $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        if ($item -and ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint))) {
            throw 'Expected an ordinary Cargo lock file.'
        }
        if (!$Apply -and !$item) { continue }
        $mode = if ($Apply) { [IO.FileMode]::OpenOrCreate } else { [IO.FileMode]::Open }
        try {
            $locks.Add([IO.File]::Open($path, $mode, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None))
        } catch [IO.IOException] {
            throw "A Rust build or installer process is active in Voldiq's release profile, or its lock is unavailable: $name"
        }
    }
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
    if ($Apply) {
        $directories = @('.fingerprint', 'build', 'deps', 'examples', 'incremental')
        $files = @('voldiq.d', 'voldiq.pdb', 'voldiq_cli.pdb')
        foreach ($name in $directories + $files) {
            $path = [IO.Path]::GetFullPath((Join-Path $release $name))
            if ([IO.Path]::GetDirectoryName($path) -ne $release) { throw 'Unexpected cache entry path.' }
            $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
            if (!$item) { continue }
            if ($item.FullName -ne $path -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
                $item.PSIsContainer -ne ($name -in $directories)) {
                throw 'Expected a regular cache entry in the release profile.'
            }
            if ($item.PSIsContainer) {
                Remove-Item -LiteralPath $path -Recurse -Force
            } else {
                Remove-Item -LiteralPath $path -Force
            }
            $removed.Add($name)
        }
    }
    foreach ($file in $preserved) {
        if ((Get-FileHash -LiteralPath (Join-Path $release $file.path) -Algorithm SHA256).Hash -ne $file.sha256) {
            throw "A deliverable changed during cleanup: $($file.path)"
        }
    }
    $after = Measure-Release
} finally {
    foreach ($lock in $locks) { $lock.Dispose() }
}
[ordered]@{
    checkedUtc = [DateTime]::UtcNow.ToString('o')
    applied = [bool]$Apply
    scope = 'Cargo release artifacts in this workspace only. Logical file bytes, not physical allocation or observed volume free-space change.'
    target = $release
    before = $before
    after = $after
    removedBytes = $before.bytes - $after.bytes
    preserved = $preserved
    removedPaths = $removed.ToArray()
    otherCachesTouched = $false
} | ConvertTo-Json -Depth 5
