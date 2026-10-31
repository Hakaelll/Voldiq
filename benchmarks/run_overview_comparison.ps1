$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$baseline = Join-Path $repo 'target/probes/overview-before.exe'
$candidate = Join-Path $repo 'target/release/examples/overview_benchmark.exe'
$baselineRecord = Get-Content -Raw -LiteralPath (Join-Path $repo 'target/probes/overview-before.json') | ConvertFrom-Json
if ((Get-FileHash -LiteralPath $baseline -Algorithm SHA256).Hash -ne $baselineRecord.executableSha256 -or
    (Get-FileHash -LiteralPath (Join-Path $repo 'src-tauri/examples/overview_benchmark.rs') -Algorithm SHA256).Hash -ne $baselineRecord.harnessSha256) {
    throw 'The baseline binary or shared harness changed.'
}
$stamp = [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$cpuBefore = Get-CimInstance Win32_Processor | Select-Object Name,NumberOfCores,NumberOfLogicalProcessors,LoadPercentage
$pins = @()
$runs = @()
function Invoke-Probe([string]$Binary, [string]$Side, [int]$Run, [bool]$Diagnostic = $false) {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $Binary
    if ($Diagnostic) { $start.Arguments = '--diagnostic' }
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($start)
    try {
        $output = $process.StandardOutput.ReadToEndAsync()
        $errors = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(60000)) { throw "Process $($process.Id) remains live; inspect it before retrying." }
        $stdout = $output.GetAwaiter().GetResult()
        $stderr = $errors.GetAwaiter().GetResult()
        $csv = "overview-comparison-$stamp-$Side-$Run.csv"
        $stream = [IO.File]::Open((Join-Path $PSScriptRoot $csv), 'CreateNew', 'Write', 'None')
        try {
            $bytes = [Text.UTF8Encoding]::new($false).GetBytes($stdout)
            $stream.Write($bytes,0,$bytes.Length)
        } finally { $stream.Dispose() }
        $rows = @($stdout | ConvertFrom-Csv)
        if ($process.ExitCode -ne 0 -or $rows.Count -ne 45) { throw "Incomplete or failed comparison run: $Side $Run. $stderr" }
        [pscustomobject]@{ side = $Side; run = $Run; diagnostic = $Diagnostic; csv = $csv;
            exitCode = $process.ExitCode; rows = $rows.Count; processCpuMilliseconds = $process.TotalProcessorTime.TotalMilliseconds; stderr = $stderr }
    } finally { $process.Dispose() }
}
try {
    $pins += [IO.File]::Open($baseline, 'Open', 'Read', 'Read')
    $pins += [IO.File]::Open($candidate, 'Open', 'Read', 'Read')
    for ($run = 1; $run -le 5; $run++) {
        $order = if ($run % 2 -eq 1) { @('before','after') } else { @('after','before') }
        foreach ($side in $order) {
            $binary = if ($side -eq 'before') { $baseline } else { $candidate }
            $runs += Invoke-Probe $binary $side $run
        }
    }
    $runs += Invoke-Probe $candidate 'diagnostic-after' 1 $true
} finally { foreach ($pin in $pins) { $pin.Dispose() } }
$report = [ordered]@{
    utc = [DateTime]::UtcNow.ToString('o'); profile = 'release'; baseline = $baselineRecord;
    candidate = @{ executableSha256 = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash;
        coreIndexSha256 = (Get-FileHash -LiteralPath (Join-Path $repo 'crates/voldiq-core/src/index.rs') -Algorithm SHA256).Hash };
    overviewModuleSha256 = (Get-FileHash -LiteralPath (Join-Path $repo 'src-tauri/src/overview.rs') -Algorithm SHA256).Hash;
    runnerSha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash;
    cpuBefore = $cpuBefore; cpuAfter = (Get-CimInstance Win32_Processor | Select-Object LoadPercentage);
    runs = $runs;
    scope = 'Five paired release-process runs per version, alternating before/after order; each process performs five measured queries per method and shape after warmup. Same harness and synthetic million-file datasets. Independent exact-ID/full-sort and Unicode/ASCII extension checks run outside timers. Report process-block medians rather than treating all inner repetitions as independent. Separate candidate component diagnostic. Desktop background load is uncontrolled. No filesystem scan, IPC, Canvas or whole-app latency is measured.'
}
$path = Join-Path $PSScriptRoot "overview-comparison-$stamp.provenance.json"
$stream = [IO.File]::Open($path, 'CreateNew', 'Write', 'None')
try {
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($report | ConvertTo-Json -Depth 8) + "`n")
    $stream.Write($bytes,0,$bytes.Length)
} finally { $stream.Dispose() }
Write-Output $path
