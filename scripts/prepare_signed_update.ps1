[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([string]::IsNullOrWhiteSpace($env:VOLDIQ_UPDATE_PUBLIC_KEY)) {
    throw 'Set VOLDIQ_UPDATE_PUBLIC_KEY to the maintainer release public key before building.'
}
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -and
    [string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PATH)) {
    throw 'Supply the matching private signing key through the Tauri signing environment.'
}

Push-Location $repository
try {
    $version = (Get-Content -LiteralPath src-tauri/tauri.conf.json -Raw | ConvertFrom-Json).version
    if ($version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
        throw 'The stable update channel requires a major.minor.patch version.'
    }
    $overlay = Join-Path $repository "target/signed-update-$([Guid]::NewGuid().ToString('N')).json"
    $overlayConfig = @{
        bundle = @{ createUpdaterArtifacts = $true }
        plugins = @{ updater = @{ pubkey = $env:VOLDIQ_UPDATE_PUBLIC_KEY.Trim() } }
    }
    if (!(Test-Path -LiteralPath target)) { New-Item -ItemType Directory -Path target | Out-Null }
    [IO.File]::WriteAllText($overlay, ($overlayConfig | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    try {
        pnpm tauri build --features production --bundles nsis --config $overlay
        if ($LASTEXITCODE) { throw 'Could not build the signed installer.' }
    } finally {
        Remove-Item -LiteralPath $overlay -Force
    }
    $directory = 'target/release/bundle/nsis'
    $name = "Voldiq_${version}_x64-setup.exe"
    $installer = Get-Item -LiteralPath (Join-Path $directory $name)
    if ($installer.Length -gt 64MB) { throw 'The installer exceeds the in-app download limit.' }
    $signature = (Get-Content -LiteralPath ($installer.FullName + '.sig') -Raw).Trim()
    $decoded = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($signature))
    $comment = ($decoded -split '\r?\n' | Where-Object { $_ -like 'trusted comment: *' })
    if (($comment -split "`t") -cnotcontains "version:$version") {
        throw 'The installer signature must bind the exact release version.'
    }
    $manifest = [ordered]@{
        version = $version
        platforms = [ordered]@{
            'windows-x86_64' = [ordered]@{
                url = "https://github.com/Hakaelll/Voldiq/releases/download/v$version/$name"
                signature = $signature
            }
        }
    }
    $destination = Join-Path $directory 'latest.json'
    [IO.File]::WriteAllText((Join-Path $repository $destination), ($manifest | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    Resolve-Path -LiteralPath $installer.FullName, ($installer.FullName + '.sig'), $destination
} finally {
    Pop-Location
}
