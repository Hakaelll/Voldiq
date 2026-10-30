[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))

function Assert-Directory([string] $Path) {
    $item = Get-Item -LiteralPath $Path -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Expected a regular directory: $Path"
    }
}

function Assert-Preview([string] $Path, [string] $ExpectedHash) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
        (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -ne $ExpectedHash) {
        throw "Existing preview does not match its content hash: $Path"
    }
}

$target = Join-Path $repository 'target'
$release = Join-Path $target 'release'
Assert-Directory $target
Assert-Directory $release
$sourcePath = Join-Path $release 'voldiq.exe'
$sourceItem = Get-Item -LiteralPath $sourcePath -Force
if ($sourceItem.PSIsContainer -or ($sourceItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'The release executable must be a regular file.'
}

# Keep the source unchanged while hashing/copying, without locking its pathname against replacement.
$source = [IO.File]::Open($sourcePath, [IO.FileMode]::Open, [IO.FileAccess]::Read,
    ([IO.FileShare]::Read -bor [IO.FileShare]::Delete))
$temporary = $null
try {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $hash = [BitConverter]::ToString($sha.ComputeHash($source)).Replace('-', '') }
    finally { $sha.Dispose() }

    $previews = Join-Path $target 'preview'
    [IO.Directory]::CreateDirectory($previews) | Out-Null
    Assert-Directory $previews
    $directory = Join-Path $previews $hash
    [IO.Directory]::CreateDirectory($directory) | Out-Null
    Assert-Directory $directory
    $preview = Join-Path $directory 'voldiq.exe'
    $reused = Test-Path -LiteralPath $preview
    if (-not $reused) {
        $temporary = Join-Path $directory ('.' + [Guid]::NewGuid().ToString('N') + '.partial')
        $destination = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew,
            [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $source.Position = 0
            $source.CopyTo($destination)
            $destination.Flush($true)
        } finally { $destination.Dispose() }
        try { [IO.File]::Move($temporary, $preview) }
        catch [IO.IOException] {
            if (-not (Test-Path -LiteralPath $preview)) { throw }
            $reused = $true
        }
    }
    Assert-Preview $preview $hash
    [pscustomobject]@{ Path = $preview; Sha256 = $hash; Bytes = $source.Length; Reused = $reused }
} finally {
    $source.Dispose()
    if ($temporary -and [IO.File]::Exists($temporary)) { [IO.File]::Delete($temporary) }
}
