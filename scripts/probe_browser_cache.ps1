param(
    [ValidateSet('Chrome', 'Edge')][string]$Browser = 'Chrome',
    [string]$LocalDataDirectory = [Environment]::GetFolderPath('LocalApplicationData'),
    [string]$OutputPath,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
$browserParts = if ($Browser -eq 'Chrome') { @('Google', 'Chrome') } else { @('Microsoft', 'Edge') }

function Test-SimpleCacheEnvelope {
    param([byte[]]$Header, [byte[]]$Footer, [long]$Length)
    if ($Header.Length -ne 24 -or $Footer.Length -ne 24 -or $Length -lt 48) { return $false }
    $initialMagic = [Convert]::ToUInt64('fcfb6d1ba7725c30', 16)
    $finalMagic = [Convert]::ToUInt64('f4fa6f45970d41d8', 16)
    return [BitConverter]::ToUInt64($Header, 0) -eq $initialMagic -and
        [BitConverter]::ToUInt64($Footer, 0) -eq $finalMagic -and
        [BitConverter]::ToUInt32($Header, 12) -le ($Length - 48)
}

function Test-BlockCachePrefix {
    param([byte[]]$Prefix, [long]$Length)
    return $Prefix.Length -eq 8 -and $Length -ge 8192 -and
        [BitConverter]::ToUInt32($Prefix, 0) -eq [Convert]::ToUInt32('c104cac3', 16) -and
        [BitConverter]::ToUInt32($Prefix, 4) -eq 0x20000
}

if ($SelfTest) {
    $header = New-Object byte[] 24
    $footer = New-Object byte[] 24
    [BitConverter]::GetBytes([Convert]::ToUInt64('fcfb6d1ba7725c30', 16)).CopyTo($header, 0)
    [BitConverter]::GetBytes([Convert]::ToUInt64('f4fa6f45970d41d8', 16)).CopyTo($footer, 0)
    if (-not (Test-SimpleCacheEnvelope $header $footer 48)) { throw 'Valid envelope rejected' }
    if (Test-SimpleCacheEnvelope $header $footer 47) { throw 'Truncated file accepted' }
    $header[12] = 1
    if (Test-SimpleCacheEnvelope $header $footer 48) { throw 'Out-of-bounds key accepted' }
    $header[12] = 0
    $footer[0] = 0
    if (Test-SimpleCacheEnvelope $header $footer 48) { throw 'Invalid footer accepted' }
    $fixtureRoot = [IO.Path]::Combine([IO.Path]::GetTempPath(), 'voldiq-cache-probe-' + [Guid]::NewGuid())
    $fixtureDirectories = @($fixtureRoot)
    $fixtureFiles = @()
    try {
        foreach ($part in @($browserParts + @('User Data', 'Default', 'Cache', 'Cache_Data'))) {
            $fixtureDirectories += [IO.Path]::Combine($fixtureDirectories[-1], $part)
        }
        foreach ($directory in $fixtureDirectories) { [void][IO.Directory]::CreateDirectory($directory) }
        $cacheDirectory = $fixtureDirectories[-1]
        [BitConverter]::GetBytes([uint32]9).CopyTo($header, 8)
        [BitConverter]::GetBytes([Convert]::ToUInt64('f4fa6f45970d41d8', 16)).CopyTo($footer, 0)
        $privateBody = [Text.Encoding]::UTF8.GetBytes('private-fixture-url-and-body')
        $fixtureFiles += [IO.Path]::Combine($cacheDirectory, '0000000000000000_0')
        [IO.File]::WriteAllBytes($fixtureFiles[-1], [byte[]]($header + $privateBody + $footer))
        $fixtureFiles += [IO.Path]::Combine($cacheDirectory, '1111111111111111_1')
        [IO.File]::WriteAllBytes($fixtureFiles[-1], (New-Object byte[] 48))
        $fixtureFiles += [IO.Path]::Combine($cacheDirectory, 'Cookies')
        [IO.File]::WriteAllBytes($fixtureFiles[-1], [byte[]]($header + $privateBody + $footer))
        $fixtureFiles += [IO.Path]::Combine($fixtureDirectories[4], '2222222222222222_0')
        [IO.File]::WriteAllBytes($fixtureFiles[-1], [byte[]]($header + $privateBody + $footer))
        $fixtureJson = & $PSCommandPath -Browser $Browser -LocalDataDirectory $fixtureRoot
        $fixtureReport = $fixtureJson | ConvertFrom-Json
        if ($fixtureReport.profilesExamined -ne 1 -or $fixtureReport.entriesExamined -ne 2 -or
            $fixtureReport.recognizedEnvelopes -ne 1 -or $fixtureReport.unrecognizedEnvelopes -ne 1 -or
            $fixtureReport.versions.'9' -ne 1 -or $fixtureReport.readFailures -ne 0) {
            throw 'Filesystem fixture report does not match the literal expected counts'
        }
        if ($fixtureJson.Contains($fixtureRoot) -or $fixtureJson.Contains('private-fixture')) {
            throw 'Fixture path or content leaked into the report'
        }
        $block = New-Object byte[] 8192
        [BitConverter]::GetBytes([Convert]::ToUInt32('c104cac3', 16)).CopyTo($block, 0)
        [BitConverter]::GetBytes([uint32]0x20000).CopyTo($block, 4)
        $fixtureFiles += [IO.Path]::Combine($cacheDirectory, 'data_0')
        [IO.File]::WriteAllBytes($fixtureFiles[-1], $block)
        $fixtureFiles += [IO.Path]::Combine($cacheDirectory, 'data_1')
        [IO.File]::WriteAllBytes($fixtureFiles[-1], (New-Object byte[] 8192))
        $blockReport = (& $PSCommandPath -Browser $Browser -LocalDataDirectory $fixtureRoot) | ConvertFrom-Json
        if ($blockReport.blockHeadersExamined -ne 2 -or $blockReport.recognizedBlockPrefixes -ne 1 -or
            $blockReport.unrecognizedBlockPrefixes -ne 1 -or $blockReport.blockReadFailures -ne 0) {
            throw 'Blockfile fixture counts do not match the expected prefix observations'
        }
        for ($n = 1; $n -le 105; $n++) {
            $fixtureFiles += [IO.Path]::Combine($cacheDirectory, ('{0:x16}_0' -f $n))
            [IO.File]::WriteAllBytes($fixtureFiles[-1], [byte[]]($header + $privateBody + $footer))
        }
        $boundedReport = (& $PSCommandPath -Browser $Browser -LocalDataDirectory $fixtureRoot) | ConvertFrom-Json
        if ($boundedReport.fileNamesObserved -ne 100 -or $boundedReport.entriesExamined -gt 100 -or
            ($boundedReport.recognizedEnvelopes + $boundedReport.unrecognizedEnvelopes + $boundedReport.readFailures) -ne $boundedReport.entriesExamined) {
            throw 'Entry observation exceeded or failed to reach the 100-entry bound'
        }
    } finally {
        foreach ($file in $fixtureFiles) { [IO.File]::Delete($file) }
        for ($i = $fixtureDirectories.Count - 1; $i -ge 0; $i--) {
            if ([IO.Directory]::Exists($fixtureDirectories[$i])) { [IO.Directory]::Delete($fixtureDirectories[$i], $false) }
        }
    }
    'Envelope self-check passed. This is not cleanup authorization.'
    return
}

if (-not [IO.Path]::IsPathRooted($LocalDataDirectory)) { throw 'Local data path must be absolute' }
$profileRoot = [IO.Path]::Combine($LocalDataDirectory, $browserParts[0], $browserParts[1], 'User Data')
$examined = 0
$recognized = 0
$unrecognized = 0
$readFailures = 0
$skippedPaths = 0
$profilesExamined = 0
$versions = @{}
$fileNamesObserved = 0
$nameShapes = @{ simpleDense = 0; simpleSparse = 0; blockData = 0; blockExternal = 0; index = 0; other = 0 }
$blockHeadersExamined = 0
$recognizedBlockPrefixes = 0
$unrecognizedBlockPrefixes = 0
$blockReadFailures = 0

if (Test-Path -LiteralPath $profileRoot -PathType Container) {
    $profileDirectories = @(Get-ChildItem -LiteralPath $profileRoot -Directory |
        Where-Object { $_.Name -eq 'Default' -or $_.Name -cmatch '^Profile [0-9]+$' } |
        Sort-Object Name | Select-Object -First 10)
    foreach ($profile in $profileDirectories) {
        $cachePath = [IO.Path]::Combine($profile.FullName, 'Cache', 'Cache_Data')
        if (-not (Test-Path -LiteralPath $cachePath -PathType Container)) { continue }
        $cursor = $cachePath
        $available = $true
        while ($cursor) {
            $directory = Get-Item -LiteralPath $cursor -Force
            if (([int]$directory.Attributes -band 0x441400) -ne 0) { $available = $false; break }
            $cursor = [IO.Path]::GetDirectoryName($cursor)
        }
        if (-not $available) { $skippedPaths++; continue }
        $profilesExamined++
        foreach ($entry in Get-ChildItem -LiteralPath $cachePath -File -Force |
            Select-Object -First (100 - $fileNamesObserved)) {
            $fileNamesObserved++
            $shape = if ($entry.Name -cmatch '^[0-9a-f]{16}_[01]$') { 'simpleDense' }
                elseif ($entry.Name -cmatch '^[0-9a-f]{16}_s$') { 'simpleSparse' }
                elseif ($entry.Name -cmatch '^data_[0-9]+$') { 'blockData' }
                elseif ($entry.Name -cmatch '^f_[0-9a-f]+$') { 'blockExternal' }
                elseif ($entry.Name -eq 'index') { 'index' }
                else { 'other' }
            $nameShapes[$shape]++
            if ($shape -ne 'simpleDense' -and $shape -ne 'blockData') { continue }
            if (([int]$entry.Attributes -band 0x441400) -ne 0) { $skippedPaths++; continue }
            if ($shape -eq 'blockData') { $blockHeadersExamined++ } else { $examined++ }
            $stream = $null
            try {
                $stream = [IO.File]::Open($entry.FullName, [IO.FileMode]::Open,
                    [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete)
                $length = $stream.Length
                if ($shape -eq 'blockData') {
                    $prefix = New-Object byte[] 8
                    if ($stream.Read($prefix, 0, 8) -eq 8 -and (Test-BlockCachePrefix $prefix $length)) {
                        $recognizedBlockPrefixes++
                    } else { $unrecognizedBlockPrefixes++ }
                    continue
                }
                $header = New-Object byte[] 24
                $footer = New-Object byte[] 24
                if ($length -lt 48 -or $stream.Read($header, 0, 24) -ne 24) { $unrecognized++; continue }
                [void]$stream.Seek(-24, [IO.SeekOrigin]::End)
                if ($stream.Read($footer, 0, 24) -ne 24) { $unrecognized++; continue }
                if (Test-SimpleCacheEnvelope $header $footer $length) {
                    $recognized++
                    $version = [string][BitConverter]::ToUInt32($header, 8)
                    if (-not $versions.ContainsKey($version)) { $versions[$version] = 0 }
                    $versions[$version]++
                } else { $unrecognized++ }
            } catch {
                if ($shape -eq 'blockData') { $blockReadFailures++ } else { $readFailures++ }
            }
            finally { if ($null -ne $stream) { $stream.Dispose() } }
        }
        if ($fileNamesObserved -ge 100) { break }
    }
}

$report = [ordered]@{
    checkedUtc = [DateTime]::UtcNow.ToString('o')
    sourceCommit = (git rev-parse HEAD)
    probeSha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash
    browser = "$Browser, default Windows user-data location"
    scope = 'Read-only envelope observation; no keys, URLs, bodies or file paths recorded'
    limits = @{ profiles = 10; fileNames = 100; entries = 100; bytesReadPerEntry = 48 }
    profilesExamined = $profilesExamined
    entriesExamined = $examined
    fileNamesObserved = $fileNamesObserved
    nameShapes = $nameShapes
    blockHeadersExamined = $blockHeadersExamined
    recognizedBlockPrefixes = $recognizedBlockPrefixes
    unrecognizedBlockPrefixes = $unrecognizedBlockPrefixes
    blockReadFailures = $blockReadFailures
    recognizedEnvelopes = $recognized
    unrecognizedEnvelopes = $unrecognized
    readFailures = $readFailures
    skippedPaths = $skippedPaths
    versions = $versions
    cleanupAuthorized = $false
    raceFreeVerification = $false
}
$json = $report | ConvertTo-Json -Depth 4
if ($OutputPath) { [IO.File]::WriteAllText([IO.Path]::GetFullPath($OutputPath), $json + [Environment]::NewLine) }
$json
