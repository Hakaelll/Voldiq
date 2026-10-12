[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$configuration = Get-Content -LiteralPath (Join-Path $repository 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$release = Join-Path $repository 'target/release'
$nativePath = Join-Path $release 'voldiq.exe'
$setupPath = Join-Path $release "bundle/nsis/Voldiq_$($configuration.version)_x64-setup.exe"
$nsisPath = Join-Path $release 'nsis/x64/installer.nsi'
$nsis = Get-Content -LiteralPath $nsisPath -Raw

function Read-PeMachine([string] $Path) {
    $stream = [IO.File]::OpenRead($Path)
    $reader = [IO.BinaryReader]::new($stream)
    try {
        if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5A4D) {
            throw "Invalid DOS executable header: $Path"
        }
        $stream.Position = 0x3C
        $offset = $reader.ReadUInt32()
        if ($offset -gt $stream.Length - 24) { throw "PE header is outside the executable: $Path" }
        $stream.Position = $offset
        if ($reader.ReadUInt32() -ne 0x4550) { throw "Invalid PE signature: $Path" }
        '0x{0:X4}' -f $reader.ReadUInt16()
    } finally {
        $reader.Dispose()
    }
}

function Read-Define([string] $Name) {
    $pattern = '(?m)^!define ' + [regex]::Escape($Name) + ' "([^"\r\n]*)"\r?$'
    $matches = [regex]::Matches($nsis, $pattern)
    if ($matches.Count -ne 1) { throw "Expected one NSIS definition for $Name." }
    $matches[0].Groups[1].Value
}

function Describe-File([string] $Path) {
    $file = Get-Item -LiteralPath $Path
    if ($file.Length -eq 0) { throw "Empty distribution artifact: $Path" }
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    [ordered]@{
        name = $file.Name
        bytes = $file.Length
        sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
        peMachine = Read-PeMachine $Path
        productVersion = $file.VersionInfo.ProductVersion
        signature = [string]$signature.Status
        signer = if ($null -ne $signature.SignerCertificate) { $signature.SignerCertificate.Subject } else { $null }
    }
}

if ($configuration.bundle.windows.webviewInstallMode.type -ne 'offlineInstaller') {
    throw 'The application configuration does not embed the offline WebView2 installer.'
}
if ((Read-Define 'INSTALLMODE') -ne 'currentUser' -or (Read-Define 'INSTALLWEBVIEW2MODE') -ne 'offlineInstaller') {
    throw 'Generated installer mode does not match current-user offline packaging.'
}
if ([IO.Path]::GetFullPath((Read-Define 'MAINBINARYSRCPATH')) -ne $nativePath) {
    throw 'Generated installer references another native executable.'
}
if (-not $nsis.Contains('File "/oname=$TEMP\MicrosoftEdgeWebView2RuntimeInstaller.exe" "${WEBVIEW2INSTALLERPATH}"')) {
    throw 'The generated NSIS source lacks the offline runtime embedding instruction.'
}

$iconPath = Join-Path $repository 'src-tauri/icons/icon.ico'
foreach ($name in 'INSTALLERICON', 'UNINSTALLERICON') {
    if ([IO.Path]::GetFullPath((Read-Define $name)) -ne $iconPath) {
        throw "Generated $name does not use the supplied artwork."
    }
}
$icon = [IO.File]::ReadAllBytes($iconPath)
if ($icon.Length -lt 6 -or [BitConverter]::ToUInt16($icon, 0) -ne 0 -or [BitConverter]::ToUInt16($icon, 2) -ne 1) {
    throw 'Invalid ICO header.'
}
$frameCount = [BitConverter]::ToUInt16($icon, 4)
if ($icon.Length -lt 6 + $frameCount * 16) { throw 'Truncated ICO directory.' }
$frames = for ($frame = 0; $frame -lt $frameCount; $frame++) {
    $offset = 6 + $frame * 16
    $width = if ($icon[$offset] -eq 0) { 256 } else { [int]$icon[$offset] }
    $height = if ($icon[$offset + 1] -eq 0) { 256 } else { [int]$icon[$offset + 1] }
    $length = [BitConverter]::ToUInt32($icon, $offset + 8)
    $data = [BitConverter]::ToUInt32($icon, $offset + 12)
    if ($width -ne $height -or $length -eq 0 -or $data -lt 6 + $frameCount * 16 -or [uint64]$data + $length -gt $icon.Length) {
        throw 'Invalid ICO frame bounds or dimensions.'
    }
    $width
}
if (($frames | Sort-Object) -join ',' -ne '16,24,32,48,64,128,256') {
    throw 'The artwork does not contain the seven required ICO sizes.'
}

$native = Describe-File $nativePath
if ($native.peMachine -ne '0x8664' -or $native.productVersion -ne $configuration.version) {
    throw 'Native executable architecture or product version differs from the configuration.'
}
$runtimePath = Read-Define 'WEBVIEW2INSTALLERPATH'
$runtime = Describe-File $runtimePath
if ($runtime.signature -ne 'Valid' -or $runtime.signer -notmatch '(^|,\s*)O=Microsoft Corporation(,|$)') {
    throw 'The bundled runtime must have a valid Microsoft Corporation Authenticode signature.'
}
$setup = Describe-File $setupPath
$commit = & git -C $repository rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Could not identify the current checkout commit.' }
[ordered]@{
    checkedUtc = [DateTime]::UtcNow.ToString('o')
    checkoutCommit = $commit
    buildCommitVerified = $false
    scope = 'Static artifact and generated-source checks; installation and embedded payload extraction are not verified.'
    native = $native
    installer = $setup
    runtime = $runtime
    installMode = 'currentUser'
    webviewMode = 'offlineInstaller'
    iconSizes = @($frames)
} | ConvertTo-Json -Depth 5
