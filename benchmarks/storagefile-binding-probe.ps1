param([string]$OutputPath)
$ErrorActionPreference = 'Stop'
if ([IntPtr]::Size -ne 8) { throw 'This probe requires x64 Windows PowerShell.' }
Add-Type -AssemblyName System.Runtime.WindowsRuntime
$storageType = [Windows.Storage.StorageFile, Windows.Storage, ContentType = WindowsRuntime]
$null = [Windows.Storage.FileIO, Windows.Storage, ContentType = WindowsRuntime]
$taskMethod = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.IsGenericMethodDefinition -and
    $_.GetParameters().Count -eq 1 -and
    $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
} | Select-Object -First 1
if (!$taskMethod) { throw 'The WinRT task projection is unavailable.' }

Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
public static class StorageFileProbeIdentity {
    [StructLayout(LayoutKind.Sequential)] struct Information {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFileW(
        string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandle(IntPtr handle, out Information information);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    public static string Read(string path) {
        IntPtr handle = CreateFileW(path, 0x80, 7, IntPtr.Zero, 3, 0x00200000, IntPtr.Zero);
        if (handle == new IntPtr(-1)) throw new IOException("Fixture identity open failed.");
        try {
            Information info;
            if (!GetFileInformationByHandle(handle, out info)) throw new IOException("Fixture identity query failed.");
            return info.Volume.ToString("x8") + ":" + info.IndexHigh.ToString("x8") + info.IndexLow.ToString("x8");
        } finally { CloseHandle(handle); }
    }
}
'@

function Wait-StorageOperation($Operation, [Type]$ResultType) {
    $task = $taskMethod.MakeGenericMethod($ResultType).Invoke($null, @($Operation))
    if (!$task.Wait(10000)) {
        try { $Operation.Cancel() }
        finally { throw [TimeoutException]::new('Controlled WinRT operation timed out.') }
    }
    return $task.GetAwaiter().GetResult()
}
function Read-StorageItem($Item) {
    try {
        $text = Wait-StorageOperation ([Windows.Storage.FileIO]::ReadTextAsync($Item)) ([string])
        if ($text -eq 'voldiq-original') { return 'original' }
        if ($text -eq 'voldiq-replacement') { return 'replacement' }
        return 'unexpected-content'
    } catch {
        $errorValue = $_.Exception
        while ($errorValue.InnerException) { $errorValue = $errorValue.InnerException }
        if ($errorValue -is [TimeoutException]) { throw $errorValue }
        return 'HRESULT_0x{0:x8}' -f ($errorValue.HResult -band 0xffffffffL)
    }
}

$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')
$fixture = [IO.Path]::GetFullPath([IO.Path]::Combine($temporaryRoot, 'voldiq-storagefile-' + [Guid]::NewGuid().ToString('N')))
$null = [IO.Directory]::CreateDirectory($fixture)
$source = [IO.Path]::Combine($fixture, 'source.txt')
$retained = [IO.Path]::Combine($fixture, 'retained.txt')
$cleanupCompleted = $false
try {
    [IO.File]::WriteAllText($source, 'voldiq-original')
    $originalIdentity = [StorageFileProbeIdentity]::Read($source)
    $item = Wait-StorageOperation ([Windows.Storage.StorageFile]::GetFileFromPathAsync($source)) $storageType
    $baseline = Read-StorageItem $item
    if ($baseline -ne 'original') { throw 'The WinRT baseline could not read the owned original.' }
    [IO.File]::Move($source, $retained)
    [IO.File]::WriteAllText($source, 'voldiq-replacement')
    $replacementIdentity = [StorageFileProbeIdentity]::Read($source)
    if ($replacementIdentity -eq $originalIdentity -or [StorageFileProbeIdentity]::Read($retained) -ne $originalIdentity) {
        throw 'The fixture did not establish distinct original and replacement identities.'
    }
    $existingItem = Read-StorageItem $item
    $freshItem = Wait-StorageOperation ([Windows.Storage.StorageFile]::GetFileFromPathAsync($source)) $storageType
    $freshRead = Read-StorageItem $freshItem
    if ($freshRead -ne 'replacement') { throw 'The replacement baseline could not read its owned fixture.' }
    [IO.File]::Delete($source)
    $missingPath = Read-StorageItem $item
    if ([IO.File]::ReadAllText($retained) -ne 'voldiq-original') { throw 'The retained fixture was unexpectedly changed.' }
    $observations = [ordered]@{
        baseline = $baseline
        originalIdentity = $originalIdentity
        replacementIdentity = $replacementIdentity
        existingItemAfterReplacement = $existingItem
        freshItemAfterReplacement = $freshRead
        existingItemAfterReplacementRemoved = $missingPath
        retainedOriginalUnchanged = $true
    }
} finally {
    if ([IO.Path]::GetDirectoryName($fixture) -ne $temporaryRoot -or
        ![IO.Path]::GetFileName($fixture).StartsWith('voldiq-storagefile-') -or
        ([IO.File]::GetAttributes($fixture) -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Refusing cleanup outside the owned temporary fixture.'
    }
    foreach ($name in @('source.txt', 'retained.txt')) {
        $path = [IO.Path]::Combine($fixture, $name)
        if ([IO.File]::Exists($path)) {
            if ([IO.File]::GetAttributes($path) -band [IO.FileAttributes]::ReparsePoint) {
                throw 'Refusing cleanup of a replaced fixture leaf.'
            }
            [IO.File]::Delete($path)
        }
    }
    [IO.Directory]::Delete($fixture, $false)
    $cleanupCompleted = $true
}
$commit = git rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Source commit could not be recorded.' }
$report = [ordered]@{
    checkedUtc = [DateTime]::UtcNow.ToString('o')
    sourceCommit = $commit
    probeSha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash
    windowsVersion = [Environment]::OSVersion.Version.ToString()
    apartment = [Threading.Thread]::CurrentThread.GetApartmentState().ToString()
    scope = 'Owned temporary files only. WinRT object and text reads; no recycling API.'
    observations = $observations
    fixtureCleanupCompleted = $cleanupCompleted
    recyclingVerified = $false
}
$json = $report | ConvertTo-Json -Depth 5
if ($OutputPath) { [IO.File]::WriteAllText([IO.Path]::GetFullPath($OutputPath), $json, [Text.UTF8Encoding]::new($false)) }
$json
