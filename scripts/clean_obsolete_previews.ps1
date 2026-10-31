[CmdletBinding()]
param([switch] $Apply)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$target = Join-Path $repository 'target'
$previews = Join-Path $target 'preview'
foreach ($directory in @($repository, $target, $previews)) {
    $item = Get-Item -LiteralPath $directory -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Expected a regular workspace directory: $directory"
    }
}
$current = (Get-FileHash -LiteralPath (Join-Path $target 'release/voldiq.exe') -Algorithm SHA256).Hash
$running = @(Get-Process voldiq -ErrorAction SilentlyContinue | ForEach-Object {
    if (-not $_.Path) { throw 'Cannot determine the running Voldiq image; cache cleanup stopped.' }
    [IO.Path]::GetFullPath($_.Path)
})
$removedBytes = 0L
$records = @(foreach ($directory in Get-ChildItem -LiteralPath $previews -Directory -Force) {
    $absolute = [IO.Path]::GetFullPath($directory.FullName)
    if ([IO.Path]::GetDirectoryName($absolute) -ne $previews -or
        ($directory.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        $directory.Name -notmatch '^[A-Fa-f0-9]{64}$') { continue }
    $files = @(Get-ChildItem -LiteralPath $absolute -Force)
    if ($files.Count -ne 1 -or $files[0].Name -ne 'voldiq.exe' -or $files[0].PSIsContainer -or
        ($files[0].Attributes -band [IO.FileAttributes]::ReparsePoint)) { continue }
    $executable = [IO.Path]::GetFullPath($files[0].FullName)
    if ([IO.Path]::GetDirectoryName($executable) -ne $absolute) { throw 'Preview escaped its directory.' }
    $hash = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash
    if ($hash -ne $directory.Name) { continue }
    $status = 'WouldRemove'
    if ($hash -eq $current) { $status = 'KeptCurrent' }
    elseif ($running -contains $executable) { $status = 'KeptRunning' }
    elseif ($Apply) {
        try {
            # Remove only the verified file and its empty directory; never recurse through unknown contents.
            Remove-Item -LiteralPath $executable -Force
            $removedBytes += $files[0].Length
            [IO.Directory]::Delete($absolute, $false)
            $status = 'Removed'
        } catch { $status = 'SkippedLockedOrChanged' }
    }
    [ordered]@{ path = $absolute; bytes = $files[0].Length; status = $status }
})
[ordered]@{ applied = [bool]$Apply; removedBytes = $removedBytes; previews = $records } | ConvertTo-Json -Depth 4
