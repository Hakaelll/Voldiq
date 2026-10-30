param(
    [switch]$AdministratorBoundary,
    [string]$OutputPath
)
$ErrorActionPreference = 'Stop'
$reportPath = $null
if ($OutputPath) {
    $reportPath = [IO.Path]::GetFullPath($OutputPath)
    if ([IO.Path]::GetDirectoryName($reportPath) -ne [IO.Path]::GetFullPath($PSScriptRoot) -or
        [IO.Path]::GetExtension($reportPath) -ne '.json' -or
        (Test-Path -LiteralPath $reportPath)) {
        throw 'Reports must be new JSON files directly inside the probe directory.'
    }
}

# A disposable capability probe. It never opens or recycles user-selected files.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

public static class RecycleBindingProbe {
    public static string CleanupStatus;
    [StructLayout(LayoutKind.Sequential)] struct UnicodeString {
        public ushort Length, MaximumLength;
        public IntPtr Buffer;
    }
    [StructLayout(LayoutKind.Sequential)] struct ObjectAttributes {
        public uint Length;
        public IntPtr RootDirectory, ObjectName;
        public uint Attributes;
        public IntPtr SecurityDescriptor, SecurityQualityOfService;
    }
    [StructLayout(LayoutKind.Sequential)] struct IoStatus { public IntPtr Status, Information; }
    [DllImport("ntdll.dll")] static extern int NtCreateFile(out IntPtr file, uint access,
        ref ObjectAttributes attributes, out IoStatus status, IntPtr allocation,
        uint fileAttributes, uint share, uint disposition, uint options, IntPtr ea, uint eaLength);
    [DllImport("ntdll.dll")] static extern int NtSetInformationFile(IntPtr file, out IoStatus status,
        IntPtr information, uint size, int informationClass);
    [DllImport("ntdll.dll")] static extern int NtSetSecurityObject(IntPtr file, uint information, IntPtr descriptor);
    [DllImport("ntdll.dll")] static extern int NtQueryObject(IntPtr file, int informationClass,
        IntPtr information, uint size, out uint returned);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFileW(
        string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetFileInformationByHandle(
        IntPtr file, int informationClass, IntPtr information, uint size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr ReOpenFile(
        IntPtr original, uint access, uint share, uint flags);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandleEx(
        IntPtr file, int informationClass, byte[] information, uint size);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr file);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool
        ConvertStringSecurityDescriptorToSecurityDescriptorW(string sddl, uint revision, out IntPtr descriptor, out uint size);
    [DllImport("advapi32.dll")] static extern uint GetSecurityInfo(IntPtr handle, uint type,
        uint information, IntPtr owner, IntPtr group, out IntPtr dacl, IntPtr sacl, out IntPtr descriptor);
    [DllImport("advapi32.dll")] static extern uint SetSecurityInfo(IntPtr handle, uint type,
        uint information, IntPtr owner, IntPtr group, IntPtr dacl, IntPtr sacl);

    static IntPtr Open(string path, uint access, uint share, uint flags) {
        var file = CreateFileW(path, access, share, IntPtr.Zero, 3, flags, IntPtr.Zero);
        if (file == new IntPtr(-1)) throw new IOException("Open failed: " + Marshal.GetLastWin32Error());
        return file;
    }
    static uint GrantedAccess(IntPtr handle) {
        IntPtr basic = Marshal.AllocHGlobal(56);
        try {
            int status = NtQueryObject(handle, 0, basic, 56, out uint returned);
            if (status < 0 || returned < 8)
                throw new IOException("Granted-access query failed: 0x" + ((uint)status).ToString("x8"));
            return (uint)Marshal.ReadInt32(basic, 4);
        } finally { Marshal.FreeHGlobal(basic); }
    }
    static string Identity(IntPtr file) {
        byte[] information = new byte[24];
        if (!GetFileInformationByHandleEx(file, 18, information, 24))
            throw new IOException("File identity failed: " + Marshal.GetLastWin32Error());
        return BitConverter.ToString(information);
    }
    static void ReadOriginal(IntPtr file) {
        using (var safe = new Microsoft.Win32.SafeHandles.SafeFileHandle(file, false))
        using (var reader = new FileStream(safe, FileAccess.Read)) {
            reader.Position = 0;
            if (reader.Length != 4 || reader.ReadByte() != 1 || reader.ReadByte() != 2 ||
                reader.ReadByte() != 3 || reader.ReadByte() != 4)
                throw new IOException("Retained content guard no longer reads the original payload.");
        }
    }
    public static string Run(bool administratorBoundary, string userSid, bool callerIsAdministrator) {
        if (IntPtr.Size != 8) throw new PlatformNotSupportedException("This probe requires x64 Windows.");
        var temporaryRoot = Path.GetFullPath(Path.GetTempPath());
        var fixture = Path.GetFullPath(Path.Combine(temporaryRoot, "voldiq-binding-" + Guid.NewGuid().ToString("N")));
        Directory.CreateDirectory(fixture);
        var original = Path.Combine(fixture, "original.bin");
        using (var source = new FileStream(original, FileMode.CreateNew, FileAccess.Write)) {
            source.Write(new byte[] { 1, 2, 3, 4 }, 0, 4);
        }
        IntPtr parent = IntPtr.Zero, leaf = IntPtr.Zero, sealedDirectory = IntPtr.Zero, contentGuard = IntPtr.Zero;
        IntPtr security = IntPtr.Zero, parentDescriptor = IntPtr.Zero, dacl = IntPtr.Zero;
        IntPtr nameBuffer = IntPtr.Zero, unicodeBuffer = IntPtr.Zero, renameBuffer = IntPtr.Zero;
        bool sealedCreated = false;
        try {
            parent = Open(fixture, 0x00020080, 7, 0x02200000);
            uint get = GetSecurityInfo(parent, 1, 4, IntPtr.Zero, IntPtr.Zero, out dacl, IntPtr.Zero, out parentDescriptor);
            if (get != 0) throw new IOException("Read parent ACL failed: " + get);
            // The alternate policy grants writes to enabled administrators, while ordinary owner access stays read-only.
            string policy = administratorBoundary
                ? "O:" + userSid + "D:P(A;;FA;;;BA)(A;;0x001200a9;;;" + userSid + ")(A;;0x001200a9;;;OW)"
                : "D:P(D;;0x000c0116;;;WD)(D;;WD;;;OW)(A;;0x001200e9;;;OW)";
            if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(
                policy,
                1, out security, out uint descriptorSize))
                throw new IOException("SDDL failed: " + Marshal.GetLastWin32Error());
            var name = "sealed";
            nameBuffer = Marshal.StringToHGlobalUni(name);
            var unicode = new UnicodeString { Length = (ushort)(name.Length * 2), MaximumLength = (ushort)((name.Length + 1) * 2), Buffer = nameBuffer };
            unicodeBuffer = Marshal.AllocHGlobal(Marshal.SizeOf<UnicodeString>());
            Marshal.StructureToPtr(unicode, unicodeBuffer, false);
            var attributes = new ObjectAttributes { Length = (uint)Marshal.SizeOf<ObjectAttributes>(),
                RootDirectory = parent, ObjectName = unicodeBuffer, Attributes = 0x40, SecurityDescriptor = security };
            // FILE_CREATE + DIRECTORY + synchronous I/O, requesting an enrollment/recovery capability atomically.
            uint stagingAccess = administratorBoundary ? 0x001600a3u : 0x001600a2u;
            int status = NtCreateFile(out sealedDirectory, stagingAccess, ref attributes, out IoStatus io,
                IntPtr.Zero, 0x10, 3, 2, 0x00200021, IntPtr.Zero, 0);
            if (status < 0) return "sealed_create_status=0x" + ((uint)status).ToString("x8") + "; rename=not_attempted";
            sealedCreated = true;
            leaf = Open(original, 0x00010080, 1, 0x00200000);
            byte[] targetName = Encoding.Unicode.GetBytes("entry.bin");
            int size = 20 + targetName.Length;
            renameBuffer = Marshal.AllocHGlobal(size);
            Marshal.Copy(new byte[size], 0, renameBuffer, size);
            Marshal.WriteIntPtr(renameBuffer, 8, sealedDirectory);
            Marshal.WriteInt32(renameBuffer, 16, targetName.Length);
            Marshal.Copy(targetName, 0, IntPtr.Add(renameBuffer, 20), targetName.Length);
            bool renamed = SetFileInformationByHandle(leaf, 3, renameBuffer, (uint)size);
            int renameError = renamed ? 0 : Marshal.GetLastWin32Error();
            int nativeRename = renamed ? 0 : NtSetInformationFile(leaf, out IoStatus renameIo, renameBuffer, (uint)size, 10);
            uint granted = GrantedAccess(sealedDirectory);
            uint sourceGranted = GrantedAccess(leaf);
            uint parentGranted = GrantedAccess(parent);
            bool replacementDenied;
            try {
                using (var child = new FileStream(Path.Combine(fixture, "sealed", "substitute.bin"), FileMode.CreateNew, FileAccess.Write)) {
                    child.WriteByte(9);
                }
                replacementDenied = false;
            }
            catch (UnauthorizedAccessException) { replacementDenied = true; }
            bool expectedDenial = !administratorBoundary || !callerIsAdministrator;
            if (replacementDenied != expectedDenial) throw new IOException("Child admission did not match the caller's expected authority.");
            bool moved = renamed || nativeRename >= 0;
            string observedPath = moved ? Path.Combine(fixture, "sealed", "entry.bin") : original;
            using (var observed = new FileStream(observedPath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete)) {
                if (observed.Length != 4 || observed.ReadByte() != 1 || observed.ReadByte() != 2 || observed.ReadByte() != 3 || observed.ReadByte() != 4)
                    throw new IOException("Owned source content changed unexpectedly.");
            }

            // Reopen by retained object before releasing the stricter namespace guard.
            string originalIdentity = Identity(leaf);
            contentGuard = ReOpenFile(leaf, 0x80000000, 5, 0x00200000);
            if (contentGuard == new IntPtr(-1))
                throw new IOException("Content guard reopen failed: " + Marshal.GetLastWin32Error());
            if (Identity(contentGuard) != originalIdentity) throw new IOException("Reopen changed file identity.");
            ReadOriginal(contentGuard);
            var beforeRelease = CreateFileW(observedPath, 0x00010000, 7, IntPtr.Zero, 3, 0x00200000, IntPtr.Zero);
            int beforeReleaseError = beforeRelease == new IntPtr(-1) ? Marshal.GetLastWin32Error() : 0;
            if (beforeRelease != new IntPtr(-1)) CloseHandle(beforeRelease);
            if (beforeReleaseError != 32) throw new IOException("Strict guard did not deny delete access with sharing violation.");
            CloseHandle(leaf);
            leaf = IntPtr.Zero;
            var writer = CreateFileW(observedPath, 0x40000000, 7, IntPtr.Zero, 3, 0x00200000, IntPtr.Zero);
            int writeError = writer == new IntPtr(-1) ? Marshal.GetLastWin32Error() : 0;
            if (writer != new IntPtr(-1)) CloseHandle(writer);
            if (writeError != 32) throw new IOException("Content guard did not deny writes with sharing violation.");
            var deleteAccess = Open(observedPath, 0x00010000, 7, 0x00200000);
            CloseHandle(deleteAccess);
            string relocated = Path.Combine(fixture, "guard-relocated.bin");
            File.Move(observedPath, relocated);
            if (Identity(contentGuard) != originalIdentity) throw new IOException("Move changed retained file identity.");
            ReadOriginal(contentGuard);
            if (File.Exists(observedPath)) throw new IOException("Source name remains after guard-compatible move.");
            return "administrator_boundary=" + administratorBoundary + "; caller_is_administrator=" + callerIsAdministrator +
                "; sealed_create_status=0x00000000; granted_access=0x" + granted.ToString("x8") +
                "; staging_requested_access=0x" + stagingAccess.ToString("x8") + "; source_requested_access=0x00010080; source_granted_access=0x" + sourceGranted.ToString("x8") +
                "; parent_requested_access=0x00020080; parent_granted_access=0x" + parentGranted.ToString("x8") +
                "; rename=" + renamed + "; rename_error=" + renameError + "; native_rename_status=0x" +
                ((uint)nativeRename).ToString("x8") + "; source_moved=" + moved + "; source_content_preserved=True; new_child_denied=" + replacementDenied +
                "; content_guard_same_identity=True; strict_guard_delete_error=" + beforeReleaseError +
                "; content_guard_write_error=" + writeError + "; delete_access_after_handoff=True; guard_compatible_move=True; retained_content_after_move=True";
        } finally {
            if (sealedCreated) {
                uint restore = SetSecurityInfo(sealedDirectory, 1, 0x80000004, IntPtr.Zero, IntPtr.Zero, dacl, IntPtr.Zero);
                int nativeRestore = restore == 0 ? 0 : NtSetSecurityObject(sealedDirectory, 0x80000004, parentDescriptor);
                CleanupStatus = "acl_restore_error=" + restore + "; native_acl_restore_status=0x" + ((uint)nativeRestore).ToString("x8");
                Console.WriteLine(CleanupStatus);
            }
            if (leaf != IntPtr.Zero && leaf != new IntPtr(-1)) CloseHandle(leaf);
            if (contentGuard != IntPtr.Zero && contentGuard != new IntPtr(-1)) CloseHandle(contentGuard);
            if (sealedDirectory != IntPtr.Zero && sealedDirectory != new IntPtr(-1)) CloseHandle(sealedDirectory);
            if (parent != IntPtr.Zero) CloseHandle(parent);
            if (security != IntPtr.Zero) LocalFree(security);
            if (parentDescriptor != IntPtr.Zero) LocalFree(parentDescriptor);
            foreach (var buffer in new[] { nameBuffer, unicodeBuffer, renameBuffer }) if (buffer != IntPtr.Zero) Marshal.FreeHGlobal(buffer);
            var resolvedFixture = Path.GetFullPath(fixture);
            if (!String.Equals(Path.GetDirectoryName(resolvedFixture), temporaryRoot.TrimEnd(Path.DirectorySeparatorChar), StringComparison.OrdinalIgnoreCase) ||
                !Path.GetFileName(resolvedFixture).StartsWith("voldiq-binding-", StringComparison.Ordinal) ||
                (File.GetAttributes(resolvedFixture) & FileAttributes.ReparsePoint) != 0)
                throw new IOException("Fixture escaped its temporary root.");
            var sealedPath = Path.Combine(resolvedFixture, "sealed");
            if (Directory.Exists(sealedPath)) {
                if ((File.GetAttributes(sealedPath) & FileAttributes.ReparsePoint) != 0)
                    throw new IOException("Refusing to clean a replaced staging directory.");
                File.Delete(Path.Combine(sealedPath, "entry.bin"));
                File.Delete(Path.Combine(sealedPath, "substitute.bin"));
                Directory.Delete(sealedPath, false);
            }
            File.Delete(Path.Combine(resolvedFixture, "original.bin"));
            File.Delete(Path.Combine(resolvedFixture, "guard-relocated.bin"));
            Directory.Delete(resolvedFixture, false);
        }
    }
}
'@

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
try {
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    $isAdministrator = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    $result = $null
    $failure = $null
    try {
        $result = [RecycleBindingProbe]::Run($AdministratorBoundary.IsPresent, $identity.User.Value, $isAdministrator)
    } catch {
        $failure = $_.Exception.GetBaseException().Message
    }
    if ($reportPath) {
        $sha = [Security.Cryptography.SHA256]::Create()
        try {
            $actorHash = [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($identity.User.Value))).Replace('-', '')
        } finally { $sha.Dispose() }
        $record = [ordered]@{
            checkedUtc = [DateTime]::UtcNow.ToString('o')
            sourceCommit = (git rev-parse HEAD)
            probeSha256 = (Get-FileHash -LiteralPath $PSCommandPath -Algorithm SHA256).Hash
            administratorBoundary = $AdministratorBoundary.IsPresent
            callerIsAdministrator = $isAdministrator
            actorSidSha256 = $actorHash
            result = $result
            failure = $failure
            cleanupStatus = [RecycleBindingProbe]::CleanupStatus
            fixtureCleanupCompleted = ($null -eq $failure)
            recyclingVerified = $false
            scope = 'Only owned disposable files and the explicitly requested new report are written. No Shell/recycling API, service installation or personal-file operations.'
        }
        $stream = [IO.File]::Open($reportPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try {
            $writer = [IO.StreamWriter]::new($stream, [Text.UTF8Encoding]::new($false))
            try { $writer.WriteLine(($record | ConvertTo-Json -Depth 4)) } finally { $writer.Dispose() }
        } finally { $stream.Dispose() }
    }
    if ($failure) { throw $failure }
    $result
} finally {
    $identity.Dispose()
}
