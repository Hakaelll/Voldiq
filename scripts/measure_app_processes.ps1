[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateRange(1,2147483647)][int] $TargetPid,
    [Parameter(Mandatory)][string] $OutputPath,
    [ValidateRange(1,60)][int] $IntervalSeconds = 5,
    [ValidateRange(5,60)][int] $Samples = 5
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$output = [IO.Path]::GetFullPath($OutputPath)
if (-not $output.StartsWith($repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Measurement output must stay inside the repository.'
}
if (Test-Path -LiteralPath $output) { throw 'Measurement output already exists.' }
$processOutput = [IO.Path]::ChangeExtension($output, 'processes.csv')
if (Test-Path -LiteralPath $processOutput) { throw 'Per-process output already exists.' }
$invariant = [Globalization.CultureInfo]::InvariantCulture
$root = [Diagnostics.Process]::GetProcessById($TargetPid)
if ($root.ProcessName -ne 'voldiq') { throw 'Target must be the Voldiq native process.' }
$rootStarted = $root.StartTime.ToUniversalTime().Ticks
$known = @{}
$previous = @{}
$rows = [Collections.Generic.List[object]]::new()
$processRows = [Collections.Generic.List[object]]::new()
$clock = [Diagnostics.Stopwatch]::StartNew()
$previousTime = 0.0

for ($sample = 0; $sample -le $Samples; $sample++) {
    if ($sample -gt 0) { Start-Sleep -Seconds $IntervalSeconds }
    $root.Refresh()
    if ($root.HasExited -or $root.StartTime.ToUniversalTime().Ticks -ne $rootStarted) {
        throw 'Native process stopped or changed; measurement is incomplete.'
    }
    $observedUtc = [DateTime]::UtcNow.ToString('o')
    $processes = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,Name,CreationDate)
    $byId = @{}
    foreach ($process in $processes) { $byId[[int]$process.ProcessId] = $process }
    if (-not $byId.ContainsKey($TargetPid)) { throw 'Native process disappeared from the process tree.' }
    $tree = [Collections.Generic.HashSet[int]]::new()
    [void]$tree.Add($TargetPid)
    foreach ($id in $known.Keys) {
        if ($byId.ContainsKey($id) -and $byId[$id].CreationDate.ToUniversalTime().Ticks -eq $known[$id]) {
            [void]$tree.Add($id)
        }
    }
    do {
        $changed = $false
        foreach ($process in $processes) {
            $id = [int]$process.ProcessId
            $parent = [int]$process.ParentProcessId
            if (-not $tree.Contains($id) -and $tree.Contains($parent) -and
                $byId.ContainsKey($parent) -and $null -ne $process.CreationDate -and
                $process.CreationDate -ge $byId[$parent].CreationDate) {
                if ($tree.Add($id)) { $changed = $true }
            }
        }
    } while ($changed)

    $current = @{}
    [long]$nativePrivate = 0
    [long]$webviewPrivate = 0
    [long]$workingSet = 0
    [double]$cpuDelta = 0
    $new = 0
    $errors = 0
    foreach ($id in $tree) {
        $metadata = $byId[$id]
        if ($id -ne $TargetPid -and $metadata.Name -ne 'msedgewebview2.exe') { continue }
        try {
            $process = [Diagnostics.Process]::GetProcessById($id)
            try {
                $process.Refresh()
                $started = $process.StartTime.ToUniversalTime().Ticks
                if ([Math]::Abs($started - $metadata.CreationDate.ToUniversalTime().Ticks) -gt 10) {
                    throw 'Process identity changed during collection.'
                }
                $key = "$id/$started"
                $cpu = $process.TotalProcessorTime.TotalSeconds
                $private = $process.PrivateMemorySize64
                $resident = $process.WorkingSet64
                $workingSet += $resident
                if ($id -eq $TargetPid) { $nativePrivate += $private } else { $webviewPrivate += $private }
                $current[$key] = $cpu
                $known[$id] = $metadata.CreationDate.ToUniversalTime().Ticks
                if ($previous.ContainsKey($key)) {
                    $delta = $cpu - $previous[$key]
                    if ($delta -lt 0) { throw 'CPU counter moved backwards.' }
                    $cpuDelta += $delta
                } else { $new++ }
                $processRows.Add([pscustomobject][ordered]@{
                    sample = $sample
                    observedUtc = $observedUtc
                    processId = $id
                    parentProcessId = $metadata.ParentProcessId
                    name = $metadata.Name
                    startedUtc = $process.StartTime.ToUniversalTime().ToString('o')
                    cpuSeconds = $cpu.ToString('R', $invariant)
                    privateBytes = $private
                    workingSetBytes = $resident
                })
            } finally { $process.Dispose() }
        } catch {
            if ($id -eq $TargetPid) { throw }
            $errors++
        }
    }
    if ($current.Count -lt 2) { throw 'No associated WebView2 process was measured.' }
    $now = $clock.Elapsed.TotalSeconds
    $elapsed = $now - $previousTime
    $exited = @($previous.Keys | Where-Object { -not $current.ContainsKey($_) }).Count
    $cpuPercent = if ($sample -eq 0) { [double]::NaN } else { 100 * $cpuDelta / $elapsed }
    $rows.Add([pscustomobject][ordered]@{
        sample = $sample
        observedUtc = $observedUtc
        intervalSeconds = $elapsed.ToString('R', $invariant)
        processCount = $current.Count
        nativePrivateBytes = $nativePrivate
        webviewPrivateBytes = $webviewPrivate
        totalPrivateBytes = $nativePrivate + $webviewPrivate
        summedWorkingSetBytes = $workingSet
        survivingCpuSeconds = $cpuDelta.ToString('R', $invariant)
        survivingOneCoreCpuPercent = $cpuPercent.ToString('R', $invariant)
        newlyMeasuredProcesses = $new
        noLongerMeasuredProcesses = $exited
        readErrors = $errors
    })
    $previous = $current
    $previousTime = $now
}
$root.Refresh()
if ($root.HasExited -or $root.StartTime.ToUniversalTime().Ticks -ne $rootStarted) { throw 'Native process changed before completion.' }
$root.Dispose()
$processRows | Export-Csv -LiteralPath $processOutput -NoTypeInformation -Encoding utf8 -NoClobber
$rows | Export-Csv -LiteralPath $output -NoTypeInformation -Encoding utf8 -NoClobber
$rows | Select-Object sample,processCount,totalPrivateBytes,summedWorkingSetBytes,survivingOneCoreCpuPercent,newlyMeasuredProcesses,noLongerMeasuredProcesses,readErrors
