[CmdletBinding()]
param([ValidateRange(1, 60)][int] $TimeoutSeconds = 15)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$executable = Join-Path $repository 'target/release/examples/window_lifecycle_probe.exe'
$item = Get-Item -LiteralPath $executable -Force
if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw 'Expected the regular lifecycle probe executable.'
}
$directory = Join-Path $repository ('target/diagnostics/lifecycle-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($directory) | Out-Null
$stdout = Join-Path $directory 'stdout.txt'
$stderr = Join-Path $directory 'stderr.txt'
$started = [DateTime]::UtcNow
$process = Start-Process -FilePath $executable -WindowStyle Hidden -PassThru `
    -RedirectStandardOutput $stdout -RedirectStandardError $stderr
$exited = $process.WaitForExit($TimeoutSeconds * 1000)
if ($exited) { $process.WaitForExit() }
$output = [IO.File]::ReadAllText($stdout)
$errors = [IO.File]::ReadAllText($stderr)
$exitCode = if ($exited) { $process.ExitCode } else { $null }
$passed = $exited -and $exitCode -eq 0 -and $output.Contains('PROBE profile-removed')
$report = [ordered]@{
    startedUtc = $started.ToString('o')
    executable = $executable
    sha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash
    sources = @('src-tauri/build.rs', 'src-tauri/examples/window_lifecycle_probe.rs',
        'scripts/probe_window_lifecycle.ps1') | ForEach-Object {
        [ordered]@{ path = $_; sha256 = (Get-FileHash -LiteralPath (Join-Path $repository $_) -Algorithm SHA256).Hash }
    }
    processId = $process.Id
    exited = $exited
    exitCode = $exitCode
    passed = $passed
    stdout = $output
    stderr = $errors
    scope = 'Hidden about:blank window; isolated temporary WebView profile; run_return event loop; no Voldiq state or databases.'
    limitation = 'Does not prove shutdown of the populated application or explain another live process.'
}
$reportPath = Join-Path $directory 'report.json'
$report | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $reportPath -Encoding UTF8
[pscustomobject]@{ Report = $reportPath; ProcessId = $process.Id; Exited = $exited; Passed = $passed }
if (-not $passed) {
    throw "Lifecycle probe did not complete successfully. A timed-out process is retained for diagnosis. See $reportPath"
}
