param([ValidateRange(10000,100000)][int]$FileCount = 100000)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $repo 'target/release/examples/live_query_benchmark.exe'
$stamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$imagePin = [IO.File]::Open($binary, 'Open', 'Read', 'Read')
$process = $null
$cpuBefore = @(Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors,LoadPercentage)
try {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $binary
    $start.Arguments = [string]$FileCount
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    Write-Host "Owned live-query benchmark running as PID $($process.Id)."
    $output = $process.StandardOutput.ReadToEndAsync()
    $errors = $process.StandardError.ReadToEndAsync()
    $finished = $process.WaitForExit(600000)
    $stdout = if ($finished) { $output.GetAwaiter().GetResult() } else { $null }
    $stderr = if ($finished) { $errors.GetAwaiter().GetResult() } else { $null }
    $rawPath = Join-Path $PSScriptRoot "live-query-$stamp.jsonl"
    if ($stdout) { [IO.File]::WriteAllText($rawPath, $stdout, [Text.UTF8Encoding]::new($false)) }
    $sources = [ordered]@{}
    @('src-tauri/examples/live_query_benchmark.rs','src-tauri/src/overview.rs',
      'crates/voldiq-core/src/lib.rs','crates/voldiq-core/src/index.rs','crates/voldiq-core/src/index/rank.rs',
      'crates/voldiq-core/src/scanner.rs','crates/voldiq-core/src/scanner/directory.rs',
      'benchmarks/run_live_query_benchmark.ps1') | ForEach-Object {
        $sources[$_] = (Get-FileHash -LiteralPath (Join-Path $repo $_) -Algorithm SHA256).Hash
    }
    $rows = @(if ($stdout) { $stdout -split "`n" | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json } })
    $report = [ordered]@{
        utc = [DateTime]::UtcNow.ToString('o'); checkoutCommit = (git -C $repo rev-parse HEAD)
        executableSha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
        sources = $sources; profile = 'release'; fixtureFiles = $FileCount
        pid = $process.Id; finished = $finished; exitCode = $(if ($finished) { $process.ExitCode } else { $null })
        rows = $rows.Count; raw = [IO.Path]::GetFileName($rawPath); stderr = $stderr
        cpuBefore = $cpuBefore
        cpuAfter = @(Get-CimInstance Win32_Processor | Select-Object LoadPercentage)
        operatingSystem = (Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,TotalVisibleMemorySize)
        scope = 'Five alternating quiet/observed pairs of real directory scans. An observer rotates root listing, global file search and the shared production overview, sleeping 100ms after each response. Read-lock wait, query work, lock hold including bounded assertions, and JSON serialization are measured separately. Overview answer boxing is included. Fixture creation, final validation, observer joining and cleanup are outside scan timers. No Tauri IPC, DOM, frame timing, cold/HDD/raw-NTFS or whole-app acceptance.'
    }
    $reportPath = Join-Path $PSScriptRoot "live-query-$stamp.provenance.json"
    [IO.File]::WriteAllText($reportPath, ($report | ConvertTo-Json -Depth 7), [Text.UTF8Encoding]::new($false))
    Write-Output $reportPath
    if (-not $finished) { throw "Benchmark PID $($process.Id) remains live. Inspect before retrying. No process was killed." }
    if ($process.ExitCode -ne 0) { throw "Live-query benchmark failed: $stderr" }
    $scans = @($rows | Where-Object kind -eq 'scan')
    $queries = @($rows | Where-Object kind -eq 'query')
    if ($scans.Count -ne 10 -or $queries.Count -lt 15 -or $rows.Count -ne $scans.Count + $queries.Count) { throw 'Incomplete live-query observations.' }
    foreach ($round in 1..5) {
        $pair = @($scans | Where-Object round -eq $round)
        if ($pair.Count -ne 2 -or @($pair | Where-Object loaded -eq $true).Count -ne 1) { throw 'Missing quiet/observed pair.' }
        foreach ($method in @('root','search','overview')) {
            if (@($queries | Where-Object { $_.round -eq $round -and $_.method -eq $method }).Count -lt 1) { throw 'Missing live query method.' }
        }
    }
    foreach ($scan in $scans) {
        if ($scan.errors -ne 0 -or $scan.files -ne $FileCount -or $scan.entries -ne $FileCount + 101) { throw 'Invalid final scan totals.' }
    }
    foreach ($query in $queries) {
        if ($query.json_bytes -le 0 -or $query.published_files -gt $FileCount -or
            $query.held_ms + 0.000001 -lt $query.query_ms -or
            $query.response_ms + 0.000001 -lt $query.wait_ms + $query.held_ms + $query.serialize_ms) { throw 'Invalid query observation.' }
    }
} finally {
    if ($process) { $process.Dispose() }
    $imagePin.Dispose()
}
