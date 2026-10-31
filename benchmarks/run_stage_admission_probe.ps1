param([switch]$RequestUac)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$binary = Join-Path $repo 'target\release\examples\stage_admission_probe.exe'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this controller with an ordinary Windows token.'
}

# Keep the selected image immutable across UAC and both process identity checks.
$imagePin = [IO.File]::Open($binary, 'Open', 'Read', 'Read')
$controller = $null
$helper = $null
try {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $binary
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    if ($RequestUac) { $start.Arguments = '--external-helper' }
    $controller = [Diagnostics.Process]::Start($start)
    $errors = $controller.StandardError.ReadToEndAsync()

    if ($RequestUac) {
        $helloRead = $controller.StandardOutput.ReadLineAsync()
        if (-not $helloRead.Wait(10000)) {
            throw "Controller $($controller.Id) has not announced its channel. Inspect this process before retrying."
        }
        $hello = $helloRead.Result | ConvertFrom-Json
        if ($hello.parent -ne $controller.Id -or
            $hello.pipe -notmatch '^\\\\\.\\pipe\\voldiq-cleanup-[0-9a-fA-F]{8}(-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}$') {
            throw 'Invalid controller announcement.'
        }
        # This switch is an explicit, interactive request. Never retry a UAC launch.
        $helper = Start-Process -FilePath $binary -ArgumentList @('helper', $hello.pipe, $controller.Id) -Verb RunAs -WindowStyle Hidden -PassThru
        $controller.StandardInput.WriteLine($helper.Id)
        $controller.StandardInput.Flush()
    }
    $controller.StandardInput.Close()
    $output = $controller.StandardOutput.ReadToEndAsync()
    if (-not $controller.WaitForExit(15000)) {
        throw "Controller $($controller.Id) remains live. Inspect it and its helper before retrying; no process was killed."
    }
    if ($helper -and -not $helper.WaitForExit(10000)) {
        throw "Helper $($helper.Id) remains live. Inspect it before retrying; no process was killed."
    }
    $stderr = $errors.GetAwaiter().GetResult()
    $stdout = $output.GetAwaiter().GetResult()
    if ($controller.ExitCode -ne 0) { throw "Controller failed: $stderr" }
    if ($helper -and $helper.ExitCode -ne 0) { throw "Helper failed with exit code $($helper.ExitCode)." }
    $observation = $stdout | ConvertFrom-Json
    if ($observation.helperElevated -ne [bool]$RequestUac -or
        -not $observation.sameUserImageAndRoleVerified -or
        -not $observation.journalIntentAndResultSurvivedRestart -or
        -not $observation.unadmittedSourceRejectedForRecycling -or
        -not $observation.shellApartmentInitialized -or
        -not $observation.protectedStageShellBindingVerified -or
        -not $observation.productionAdmissionProtocolVerified -or
        -not $observation.helperReleasedStrictSourceWhileLive -or
        -not $observation.parentAuthorityReopenedAndNarrowed -or
        -not $observation.emptyStageRemovedByVerifiedHandle -or
        -not $observation.wrongCleanupTargetPreserved -or
        -not $observation.originalIdentityAndContentsVerified -or
        -not $observation.ownedFixtureCleaned) {
        throw 'Fixture did not confirm its required invariants.'
    }
    if ($RequestUac) {
        if (-not $observation.nativeAdmissionSucceeded -or
            -not $observation.reviewedSourceReplayRejected -or
            -not $observation.rollbackCollisionPreservedBothFiles -or
            -not $observation.returnedByOrdinaryController) {
            throw 'Elevated admission or ordinary rollback was not verified.'
        }
    } elseif ($observation.nativeAdmissionSucceeded -or $observation.nativeError -ne 5) {
        throw 'Ordinary admission did not fail with access denied.'
    }

    $sources = @(
        'crates/voldiq-core/examples/stage_admission_probe.rs',
        'crates/voldiq-core/src/windows/cleanup_channel.rs',
        'crates/voldiq-core/src/windows/cleanup_io.rs',
        'crates/voldiq-core/src/windows/security.rs',
        'crates/voldiq-core/src/windows/cleanup_stage.rs',
        'crates/voldiq-core/src/windows/cleanup_stage/admission.rs',
        'crates/voldiq-core/src/cleanup.rs',
        'crates/voldiq-core/src/cleanup/journal.rs',
        'crates/voldiq-core/src/cleanup/journal/relocation.rs',
        'crates/voldiq-core/src/cleanup/recycle.rs',
        'crates/voldiq-core/src/cleanup/helper.rs',
        'benchmarks/run_stage_admission_probe.ps1'
    )
    $hashes = [ordered]@{}
    foreach ($source in $sources) {
        $hashes[$source] = (Get-FileHash -LiteralPath (Join-Path $repo $source) -Algorithm SHA256).Hash
    }
    $report = [ordered]@{
        utc = [DateTime]::UtcNow.ToString('o')
        controllerElevated = $false
        executableSha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
        sources = $hashes
        observation = $observation
        scope = 'Owned temporary fixture and durable admission/rollback ledger only. No Shell recycling or application transaction is claimed.'
    }
    $suffix = if ($RequestUac) { 'elevated' } else { 'ordinary' }
    $reportPath = Join-Path $PSScriptRoot ("stage-admission-$suffix-" + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff') + '.json')
    $file = [IO.File]::Open($reportPath, 'CreateNew', 'Write', 'None')
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($report | ConvertTo-Json -Depth 8) + "`n")
        $file.Write($bytes, 0, $bytes.Length)
    } finally { $file.Dispose() }
    Write-Output $reportPath
    Write-Output ($observation | ConvertTo-Json -Compress)
} finally {
    if ($controller) {
        $controller.StandardInput.Close()
        $controller.Dispose()
    }
    if ($helper) { $helper.Dispose() }
    $imagePin.Dispose()
    $identity.Dispose()
}
