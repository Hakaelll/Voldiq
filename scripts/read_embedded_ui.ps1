[CmdletBinding()]
param([Parameter(Mandatory)][string] $ExecutablePath)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$executable = [IO.Path]::GetFullPath($ExecutablePath)
if (-not $executable.StartsWith($repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The UI probe executable must stay inside the repository.'
}
$item = Get-Item -LiteralPath $executable
if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Expected a regular native executable.' }
if (-not (Select-String -LiteralPath $executable -Pattern 'VOLDIQ_EMBEDDED_UI_V1' -SimpleMatch -Quiet)) {
    throw 'This executable has no embedded-UI probe. Rebuild with pnpm tauri build or --features production.'
}
$start = [Diagnostics.ProcessStartInfo]::new()
$start.FileName = $executable
$start.Arguments = '--verify-embedded-ui'
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
$start.RedirectStandardOutput = $true
$start.RedirectStandardError = $true
$process = [Diagnostics.Process]::Start($start)
try {
    $outputTask = $process.StandardOutput.ReadToEndAsync()
    $errorTask = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(10000)) { $process.Kill(); throw 'Embedded-UI probe timed out.' }
    $outputText = $outputTask.GetAwaiter().GetResult()
    $errorText = $errorTask.GetAwaiter().GetResult()
    if ($process.ExitCode -ne 0) { throw "Embedded-UI probe failed: $errorText" }
    $report = $outputText | ConvertFrom-Json
    if ($report.probe -ne 'VOLDIQ_EMBEDDED_UI_V1' -or $report.development -ne $false) { throw 'The native image is not a production UI bundle.' }
    foreach ($asset in Get-ChildItem -LiteralPath (Join-Path $repository 'dist/assets') -File) {
        if ($asset.Extension -notin @('.js', '.css')) { continue }
        $embedded = @($report.assets | Where-Object path -eq ('assets/' + $asset.Name))
        if ($embedded.Count -ne 1 -or $embedded[0].bytes -ne $asset.Length -or $embedded[0].sha256 -ne (Get-FileHash -LiteralPath $asset.FullName -Algorithm SHA256).Hash) {
            throw "The native image does not contain the current frontend asset: $($asset.Name)"
        }
    }
    $report
} finally {
    $process.Dispose()
}
