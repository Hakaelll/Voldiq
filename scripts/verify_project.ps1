[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$checks = [Collections.Generic.List[object]]::new()
Push-Location $repository
try {
    $commit = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Could not identify the checkout.' }
    $started = [DateTime]::UtcNow
    foreach ($check in @(
        @{ Name = 'Rust formatting'; Program = 'cargo'; Arguments = @('fmt', '--all', '--', '--check') },
        @{ Name = 'Frontend types and accessibility diagnostics'; Program = 'pnpm'; Arguments = @('check') },
        @{ Name = 'Frontend behavior tests'; Program = 'pnpm'; Arguments = @('test') },
        @{ Name = 'Workspace Rust tests'; Program = 'cargo'; Arguments = @('test', '--workspace') },
        @{ Name = 'Strict workspace Clippy'; Program = 'cargo'; Arguments = @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings') },
        @{ Name = 'Production frontend build'; Program = 'pnpm'; Arguments = @('build') }
    )) {
        Write-Host $check.Name
        $clock = [Diagnostics.Stopwatch]::StartNew()
        $checkArguments = $check.Arguments
        & $check.Program @checkArguments | Out-Host
        $code = $LASTEXITCODE
        $clock.Stop()
        if ($code -ne 0) { throw "$($check.Name) failed with exit code $code." }
        $checks.Add([ordered]@{ name = $check.Name; exitCode = $code; elapsedSeconds = [Math]::Round($clock.Elapsed.TotalSeconds, 3) })
    }
    [ordered]@{
        checkedUtc = [DateTime]::UtcNow.ToString('o')
        startedUtc = $started.ToString('o')
        checkoutCommit = $commit
        scope = 'Source checks, automated behavior tests and frontend build. Native UI, performance targets and installer acceptance are separate.'
        checks = $checks.ToArray()
    } | ConvertTo-Json -Depth 4
} finally {
    Pop-Location
}
