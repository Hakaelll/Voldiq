$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $repo 'target/release/examples/overview_benchmark.exe'
$stamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$cpuBefore = Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors,LoadPercentage
$system = Get-CimInstance Win32_OperatingSystem | Select-Object Caption,Version,TotalVisibleMemorySize,FreePhysicalMemory
$imagePin = [IO.File]::Open($binary, 'Open', 'Read', 'Read')
$results = @()
try {
    foreach ($mode in @('measurement','diagnostic')) {
        $start = [Diagnostics.ProcessStartInfo]::new()
        $start.FileName = $binary
        if ($mode -eq 'diagnostic') { $start.Arguments = '--diagnostic' }
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $process = [Diagnostics.Process]::Start($start)
        try {
            $output = $process.StandardOutput.ReadToEndAsync()
            $errors = $process.StandardError.ReadToEndAsync()
            if (-not $process.WaitForExit(60000)) {
                throw "Benchmark $($process.Id) remains live; inspect it before retrying."
            }
            $stdout = $output.GetAwaiter().GetResult()
            $stderr = $errors.GetAwaiter().GetResult()
            $rows = @($stdout | ConvertFrom-Csv)
            $csvPath = Join-Path $PSScriptRoot "overview-release-$stamp-$mode.csv"
            $stream = [IO.File]::Open($csvPath, 'CreateNew', 'Write', 'None')
            try {
                $bytes = [Text.UTF8Encoding]::new($false).GetBytes($stdout)
                $stream.Write($bytes,0,$bytes.Length)
            } finally { $stream.Dispose() }
            $results += [ordered]@{ mode = $mode; pid = $process.Id; exitCode = $process.ExitCode;
                rows = $rows.Count; csv = [IO.Path]::GetFileName($csvPath); stderr = $stderr;
                processCpuMilliseconds = $process.TotalProcessorTime.TotalMilliseconds }
            if ($process.ExitCode -ne 0 -or $rows.Count -ne 45) {
                throw "Benchmark failed or returned incomplete rows: $mode"
            }
            foreach ($group in ($rows | Group-Object shape,method)) {
                if ($group.Count -ne 5) { throw 'Missing repetitions.' }
            }
        } finally { $process.Dispose() }
    }
} finally { $imagePin.Dispose() }
$sources = [ordered]@{}
@('src-tauri/src/overview.rs','src-tauri/src/query_commands.rs',
    'src-tauri/examples/overview_benchmark.rs','crates/voldiq-core/src/index.rs',
    'crates/voldiq-core/src/index/rank.rs','benchmarks/run_overview_benchmark.ps1') | ForEach-Object {
        $sources[$_] = (Get-FileHash -LiteralPath (Join-Path $repo $_) -Algorithm SHA256).Hash
    }
$report = [ordered]@{ utc = [DateTime]::UtcNow.ToString('o'); executableSha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash;
    checkoutCommit = (git -C $repo rev-parse HEAD); profile = 'release'; sources = $sources;
    cpuBefore = $cpuBefore; cpuAfter = (Get-CimInstance Win32_Processor | Select-Object LoadPercentage);
    operatingSystem = $system; runs = $results;
    claim = 'Release latency baseline for the production synchronous overview composition, bounded branch pass and JSON serialization on three synthetic million-file index shapes. No before/after optimization claim.';
    scope = 'One million in-memory files per shape: 16 equal-sized sibling folders, 1000 siblings with varied sizes, and a 64-level chain queried at depth 32. Construction, full-sort reference verification and warmup are outside query timers. Five repetitions per method, alternating overview/branch order. Component diagnostics run in a separate process. No disk scan, native IPC, concurrent indexing, Canvas or total-app responsiveness is measured. Desktop background load is uncontrolled. Thread CPU counters may be quantized.' }
$path = Join-Path $PSScriptRoot "overview-release-$stamp.provenance.json"
$stream = [IO.File]::Open($path, 'CreateNew', 'Write', 'None')
try {
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($report | ConvertTo-Json -Depth 8) + "`n")
    $stream.Write($bytes,0,$bytes.Length)
} finally { $stream.Dispose() }
Write-Output $path
