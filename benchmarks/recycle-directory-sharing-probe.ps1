param([string]$OutputPath)
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class RecycleDirectorySharingProbe {
    [StructLayout(LayoutKind.Sequential)] struct IoStatus { public IntPtr Status, Information; }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFileW(
        string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("ntdll.dll")] static extern int NtSetInformationFile(IntPtr file, out IoStatus status,
        IntPtr information, uint size, int informationClass);

    public sealed class Observation {
        public string Mode;
        public uint RequestedAccess, Share;
        public int RenameStatus, DirectoryOpenError, ChildCreateError;
        public bool FreshAddHandleOpened, NewChildCreated, OriginalAtDestination;
    }

    static IntPtr Open(string path, uint access, uint share) {
        return CreateFileW(path, access, share, IntPtr.Zero, 3, 0x02200000, IntPtr.Zero);
    }
    public static Observation[] Run() {
        if (IntPtr.Size != 8) throw new PlatformNotSupportedException("x64 Windows required");
        var temp = Path.GetFullPath(Path.GetTempPath()).TrimEnd(Path.DirectorySeparatorChar);
        var fixture = Path.GetFullPath(Path.Combine(temp, "voldiq-sharing-" + Guid.NewGuid().ToString("N")));
        Directory.CreateDirectory(fixture);
        var observations = new List<Observation>();
        try {
            for (int mode = 0; mode < 3; mode++) {
                var directory = Path.Combine(fixture, "stage-" + mode);
                var source = Path.Combine(fixture, "source-" + mode + ".bin");
                var target = Path.Combine(directory, "entry.bin");
                var substitute = Path.Combine(directory, "substitute.bin");
                Directory.CreateDirectory(directory);
                File.WriteAllBytes(source, new byte[] { 1, 2, 3, 4 });
                uint access = mode == 0 ? 0x00100080u : 0x001200a2u;
                uint share = mode == 2 ? 0u : 1u;
                IntPtr parent = IntPtr.Zero, leaf = IntPtr.Zero, buffer = IntPtr.Zero;
                try {
                    parent = Open(directory, access, share);
                    if (parent == new IntPtr(-1)) throw new IOException("Directory open: " + Marshal.GetLastWin32Error());
                    leaf = Open(source, 0x00010080, 1);
                    if (leaf == new IntPtr(-1)) throw new IOException("Source open: " + Marshal.GetLastWin32Error());
                    byte[] name = Encoding.Unicode.GetBytes("entry.bin");
                    int size = 20 + name.Length;
                    buffer = Marshal.AllocHGlobal(size);
                    Marshal.Copy(new byte[size], 0, buffer, size);
                    Marshal.WriteIntPtr(buffer, 8, parent);
                    Marshal.WriteInt32(buffer, 16, name.Length);
                    Marshal.Copy(name, 0, IntPtr.Add(buffer, 20), name.Length);
                    var result = new Observation {
                        Mode = mode == 0 ? "attributes-read-share" : mode == 1 ? "add-file-read-share" : "add-file-no-sharing",
                        RequestedAccess = access, Share = share,
                        RenameStatus = NtSetInformationFile(leaf, out IoStatus io, buffer, (uint)size, 10)
                    };
                    var fresh = Open(directory, 0x001200a2, 7);
                    result.FreshAddHandleOpened = fresh != new IntPtr(-1);
                    result.DirectoryOpenError = result.FreshAddHandleOpened ? 0 : Marshal.GetLastWin32Error();
                    if (result.FreshAddHandleOpened) CloseHandle(fresh);
                    try { File.WriteAllBytes(substitute, new byte[] { 9 }); result.NewChildCreated = true; }
                    catch (IOException error) { result.ChildCreateError = error.HResult; }
                    catch (UnauthorizedAccessException error) { result.ChildCreateError = error.HResult; }
                    if (result.RenameStatus >= 0) {
                        using (var reader = new FileStream(target, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete)) {
                            byte[] content = new byte[4];
                            result.OriginalAtDestination = reader.Length == 4 && reader.Read(content, 0, 4) == 4 && content[0] == 1 && content[3] == 4;
                        }
                        if (!result.OriginalAtDestination || File.Exists(source)) throw new IOException("Rename fixture lost its original identity/content expectation");
                    }
                    observations.Add(result);
                } finally {
                    if (leaf != IntPtr.Zero && leaf != new IntPtr(-1)) CloseHandle(leaf);
                    if (parent != IntPtr.Zero && parent != new IntPtr(-1)) CloseHandle(parent);
                    if (buffer != IntPtr.Zero) Marshal.FreeHGlobal(buffer);
                }
            }
        } finally {
            if (!String.Equals(Path.GetDirectoryName(fixture), temp, StringComparison.OrdinalIgnoreCase) ||
                !Path.GetFileName(fixture).StartsWith("voldiq-sharing-", StringComparison.Ordinal) ||
                (File.GetAttributes(fixture) & FileAttributes.ReparsePoint) != 0) throw new IOException("Unexpected fixture root");
            for (int mode = 0; mode < 3; mode++) {
                var directory = Path.Combine(fixture, "stage-" + mode);
                if (Directory.Exists(directory)) {
                    if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0) throw new IOException("Replaced fixture directory");
                    File.Delete(Path.Combine(directory, "entry.bin"));
                    File.Delete(Path.Combine(directory, "substitute.bin"));
                    Directory.Delete(directory, false);
                }
                File.Delete(Path.Combine(fixture, "source-" + mode + ".bin"));
            }
            Directory.Delete(fixture, false);
        }
        return observations.ToArray();
    }
}
'@

$observations = [RecycleDirectorySharingProbe]::Run()
$report = [ordered]@{
    checkedUtc = [DateTime]::UtcNow.ToString('o')
    sourceCommit = (git rev-parse HEAD)
    probeSha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash
    scope = 'Owned disposable files only. No Shell or recycling call.'
    observations = $observations
    fixtureCleanupCompleted = $true
    recyclingVerified = $false
}
$json = $report | ConvertTo-Json -Depth 4
if ($OutputPath) { [IO.File]::WriteAllText([IO.Path]::GetFullPath($OutputPath), $json + [Environment]::NewLine) }
$json
