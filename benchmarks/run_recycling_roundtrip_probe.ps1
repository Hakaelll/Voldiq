param([switch]$RequestUac)

$ErrorActionPreference = 'Stop'
if (-not $RequestUac) { throw 'This owned-fixture experiment requires an explicit -RequestUac invocation.' }
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
try {
    if ([Security.Principal.WindowsPrincipal]::new($identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Run the controller with an ordinary Windows token.'
    }
} finally { $identity.Dispose() }
$repo = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $repo 'target/release/examples/stage_admission_probe.exe'
$imagePin = [IO.File]::Open($binary, 'Open', 'Read', 'Read')
$controller = $null
try {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $binary
    $start.Arguments = '--recycle-and-restore'
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $controller = [Diagnostics.Process]::Start($start)
    $output = $controller.StandardOutput.ReadToEndAsync()
    $errors = $controller.StandardError.ReadToEndAsync()
    $finished = $controller.WaitForExit(60000)
    $sources = [ordered]@{}
    @(
        'crates/voldiq-core/examples/stage_admission_probe.rs',
        'crates/voldiq-core/src/cleanup.rs',
        'crates/voldiq-core/src/cleanup/helper.rs',
        'crates/voldiq-core/src/cleanup/journal.rs',
        'crates/voldiq-core/src/cleanup/journal/relocation.rs',
        'crates/voldiq-core/src/cleanup/recycle.rs',
        'crates/voldiq-core/src/cleanup/recycle/restore.rs',
        'crates/voldiq-core/src/windows/cleanup_channel.rs',
        'crates/voldiq-core/src/windows/cleanup_io.rs',
        'crates/voldiq-core/src/windows/security.rs',
        'crates/voldiq-core/src/windows/cleanup_stage.rs',
        'crates/voldiq-core/src/windows/cleanup_stage/admission.rs',
        'benchmarks/run_recycling_roundtrip_probe.ps1'
    ) | ForEach-Object { $sources[$_] = (Get-FileHash -LiteralPath (Join-Path $repo $_) -Algorithm SHA256).Hash }
    $stdout = if ($finished) { $output.GetAwaiter().GetResult() } else { $null }
    $stderr = if ($finished) { $errors.GetAwaiter().GetResult() } else { $null }
    $observation = if ($stdout) { $stdout | ConvertFrom-Json } else { $null }
    $report = [ordered]@{
        utc = [DateTime]::UtcNow.ToString('o')
        executableSha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
        sources = $sources
        controllerPid = $controller.Id
        finished = $finished
        exitCode = $(if ($finished) { $controller.ExitCode } else { $null })
        stderr = $stderr
        observation = $observation
        scope = 'One own temporary four-byte file. Production native UAC launcher and elevated CLI admit the retained source. Ordinary Rust recycling and handle-bound original-location restoration; occupied-name and replay checks. No other bin item or service is modified. Physical bin metadata retirement is not claimed.'
    }
    $reportPath = Join-Path $PSScriptRoot ('recycling-roundtrip-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff') + '.json')
    $stream = [IO.File]::Open($reportPath, 'CreateNew', 'Write', 'None')
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($report | ConvertTo-Json -Depth 8) + "`n")
        $stream.Write($bytes, 0, $bytes.Length)
    } finally { $stream.Dispose() }
    Write-Output $reportPath
    if (-not $finished) { throw "Controller $($controller.Id) remains live. Inspect it and its helper before retrying. No process was killed." }
    if ($controller.ExitCode -ne 0) { throw "Owned fixture failed: $stderr" }
    @('nativeLauncherVerified','productionElevatedEntryVerified','protectedAdmissionVerified',
        'rustRecyclingVerified','rustRestorationVerified','restoreCollisionPreservedBothFiles',
        'restoreReplayRejected','originalIdentityAndContentsVerified','journalSurvivedRestart',
        'ownedFilesystemFixtureCleaned','binPayloadAbsentAfterRestoration','recycledVersionSurvivedRestart') | ForEach-Object {
            if ($observation.$_ -ne $true) { throw "Required roundtrip observation missing: $_" }
        }
    Write-Output ($observation | ConvertTo-Json -Compress)
} finally {
    if ($controller) { $controller.Dispose() }
    $imagePin.Dispose()
}
