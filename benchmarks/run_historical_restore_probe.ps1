$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $repo 'target/release/examples/historical_restore_probe.exe'
$imagePin = [IO.File]::Open($binary, 'Open', 'Read', 'Read')
$controller = $null
try {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $binary
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $controller = [Diagnostics.Process]::Start($start)
    $output = $controller.StandardOutput.ReadToEndAsync()
    $errors = $controller.StandardError.ReadToEndAsync()
    $finished = $controller.WaitForExit(45000)
    $sources = [ordered]@{}
    @(
        'crates/voldiq-core/examples/historical_restore_probe.rs',
        'crates/voldiq-core/src/cleanup/recycle/historical.rs',
        'crates/voldiq-core/src/cleanup/journal/relocation.rs',
        'crates/voldiq-core/src/windows/cleanup_io.rs',
        'crates/voldiq-core/src/windows/cleanup_stage/admission.rs',
        'benchmarks/run_historical_restore_probe.ps1'
    ) | ForEach-Object { $sources[$_] = (Get-FileHash -LiteralPath (Join-Path $repo $_) -Algorithm SHA256).Hash }
    $stdout = if ($finished) { $output.GetAwaiter().GetResult() } else { $null }
    $stderr = if ($finished) { $errors.GetAwaiter().GetResult() } else { $null }
    $report = [ordered]@{
        utc = [DateTime]::UtcNow.ToString('o')
        executableSha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
        sources = $sources
        controllerPid = $controller.Id
        finished = $finished
        exitCode = $(if ($finished) { $controller.ExitCode } else { $null })
        stderr = $stderr
        observation = $(if ($stdout) { $stdout | ConvertFrom-Json } else { $null })
        scope = 'One owned temporary four-byte file, ordinary direct Shell recycling, closed source guards and journal, fresh historical review and original-location restoration. No UAC, protected admission, other bin item or service. Physical bin metadata retirement is not claimed.'
    }
    $reportPath = Join-Path $PSScriptRoot ('historical-bin-restore-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff') + '.json')
    $stream = [IO.File]::Open($reportPath, 'CreateNew', 'Write', 'None')
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($report | ConvertTo-Json -Depth 8) + "`n")
        $stream.Write($bytes, 0, $bytes.Length)
    } finally { $stream.Dispose() }
    Write-Output $reportPath
    if (-not $finished) { throw "Controller $($controller.Id) remains live. Inspect before any retry. No process was killed." }
    if ($controller.ExitCode -ne 0) { throw "Owned historical fixture failed: $stderr" }
    @('reopenedBinRestored','observedVersionSurvivedRestart','collisionPreservedBoth',
      'sourceWritesDeniedDuringReview','originalIdentityAndBytesRestored',
      'durableRestorationSurvivedRestart','replayRejected','ownedFixtureRemoved') | ForEach-Object {
        if ($report.observation.$_ -ne $true) { throw "Missing successful observation: $_" }
    }
} finally {
    if ($controller) { $controller.Dispose() }
    $imagePin.Dispose()
}
