param([ValidateRange(10000,100000)][int]$FileCount = 100000, [switch]$CancelOnly)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $repo 'target/release/examples/concurrency_benchmark.exe'
$stamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$imagePin = [IO.File]::Open($binary, 'Open', 'Read', 'Read')
$process = $null
$cpuBefore = @(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors,LoadPercentage)
try {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $binary
    $start.Arguments = [string]$FileCount
    if ($CancelOnly) { $start.Arguments += " --cancel-only" }
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    Write-Host "Owned concurrency benchmark running as PID $($process.Id)."
    $output = $process.StandardOutput.ReadToEndAsync()
    $errors = $process.StandardError.ReadToEndAsync()
    $finished = $process.WaitForExit(600000)
    $stdout = if ($finished) { $output.GetAwaiter().GetResult() } else { $null }
    $stderr = if ($finished) { $errors.GetAwaiter().GetResult() } else { $null }
    $csvPath = Join-Path $PSScriptRoot "concurrency-auto-$stamp.csv"
    if ($stdout) { [IO.File]::WriteAllText($csvPath, $stdout, [Text.UTF8Encoding]::new($false)) }
    $sources = [ordered]@{}
    @('crates/voldiq-core/examples/concurrency_benchmark.rs','crates/voldiq-core/src/lib.rs',
      'crates/voldiq-core/src/scanner.rs','crates/voldiq-core/src/scanner/directory.rs',
      'crates/voldiq-core/src/windows/storage.rs','benchmarks/run_concurrency_benchmark.ps1') | ForEach-Object {
        $sources[$_] = (Get-FileHash -LiteralPath (Join-Path $repo $_) -Algorithm SHA256).Hash
    }
    $rows = @(if ($stdout) { $stdout | ConvertFrom-Csv })
    $report = [ordered]@{
        utc = [DateTime]::UtcNow.ToString('o'); checkoutCommit = (git -C $repo rev-parse HEAD)
        executableSha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
        sources = $sources; profile = 'release'; fixtureFiles = $FileCount; cancelOnly = [bool]$CancelOnly
        pid = $process.Id; finished = $finished; exitCode = $(if ($finished) { $process.ExitCode } else { $null })
        rows = $rows.Count; csv = [IO.Path]::GetFileName($csvPath); stderr = $stderr
        cpuBefore = $cpuBefore
        cpuAfter = @(Get-CimInstance Win32_Processor | Select-Object LoadPercentage)
        operatingSystem = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,TotalVisibleMemorySize)
        scope = 'Production directory enumeration with automatic and fixed 1/2/4/8 concurrency, five rotating repetitions each. A separate workload measures cancellation from an independent thread through scanner return after worker joining. A live progress callback only notifies that thread; after a one-millisecond delay the scanner continues independently while the thread requests cancellation. Newly written real files warm metadata. Creation, validation and cleanup are outside scan timers. Preflight worker counts use a canonical path and are separate device resolutions, not scanner telemetry. CPU is this process only. No raw NTFS scan, cold cache, physical disk bandwidth, HDD/network comparison, native UI, concurrent query or whole-app acceptance.'
    }
    $reportPath = Join-Path $PSScriptRoot "concurrency-auto-$stamp.provenance.json"
    [IO.File]::WriteAllText($reportPath, ($report | ConvertTo-Json -Depth 7), [Text.UTF8Encoding]::new($false))
    Write-Output $reportPath
    if (-not $finished) { throw "Benchmark PID $($process.Id) remains live. Inspect before retrying. No process was killed." }
    $expectedRows = if ($CancelOnly) { 25 } else { 50 }
    if ($process.ExitCode -ne 0 -or $rows.Count -ne $expectedRows) { throw "Incomplete benchmark: $stderr" }
    foreach ($group in ($rows | Group-Object phase,setting)) {
        if ($group.Count -ne 5) { throw 'Missing benchmark repetitions.' }
    }
    foreach ($row in $rows) {
        if ([int]$row.errors -ne 0 -or [long]$row.logical -ne [long]$row.files * 128) { throw 'Invalid measured scan totals.' }
        if ($row.phase -eq 'full' -and ([int]$row.files -ne $FileCount -or [int]$row.entries -ne $FileCount + 101)) { throw 'Incomplete full scan.' }
        if ($row.phase -eq 'cancel_external' -and ([int]$row.files -le 0 -or [int]$row.files -ge $FileCount -or [double]$row.cancel_ms -lt 0)) { throw 'Invalid live cancellation sample.' }
    }
} finally {
    if ($process) { $process.Dispose() }
    $imagePin.Dispose()
}
