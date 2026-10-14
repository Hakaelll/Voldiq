$ErrorActionPreference = 'Stop'

# A disposable capability probe. It never opens or recycles user-selected files.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

public static class RecycleBindingProbe {
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
    public static string Run() {
        if (IntPtr.Size != 8) throw new PlatformNotSupportedException("This probe requires x64 Windows.");
        var temporaryRoot = Path.GetFullPath(Path.GetTempPath());
        var fixture = Path.GetFullPath(Path.Combine(temporaryRoot, "voldiq-binding-" + Guid.NewGuid().ToString("N")));
        Directory.CreateDirectory(fixture);
        var original = Path.Combine(fixture, "original.bin");
        File.WriteAllBytes(original, new byte[] { 1, 2, 3, 4 });
        IntPtr parent = IntPtr.Zero, leaf = IntPtr.Zero, sealedDirectory = IntPtr.Zero;
        IntPtr security = IntPtr.Zero, parentDescriptor = IntPtr.Zero, dacl = IntPtr.Zero;
        IntPtr nameBuffer = IntPtr.Zero, unicodeBuffer = IntPtr.Zero, renameBuffer = IntPtr.Zero;
        bool sealedCreated = false;
        try {
            parent = Open(fixture, 0x00020080, 7, 0x02200000);
            uint get = GetSecurityInfo(parent, 1, 4, IntPtr.Zero, IntPtr.Zero, out dacl, IntPtr.Zero, out parentDescriptor);
            if (get != 0) throw new IOException("Read parent ACL failed: " + get);
            // Deny new children, attribute mutation and security mutation from creation, including owner rights.
            if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(
                "D:P(D;;0x000c0116;;;WD)(D;;WD;;;OW)(A;;0x001200e9;;;OW)",
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
            int status = NtCreateFile(out sealedDirectory, 0x001600a2, ref attributes, out IoStatus io,
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
            try { File.WriteAllBytes(Path.Combine(fixture, "sealed", "substitute.bin"), new byte[] { 9 }); replacementDenied = false; }
            catch (UnauthorizedAccessException) { replacementDenied = true; }
            if (!replacementDenied) throw new IOException("Sealed directory allowed an unexpected new child.");
            return "sealed_create_status=0x00000000; granted_access=0x" + granted.ToString("x8") +
                "; staging_requested_access=0x001600a2; source_requested_access=0x00010080; source_granted_access=0x" + sourceGranted.ToString("x8") +
                "; parent_requested_access=0x00020080; parent_granted_access=0x" + parentGranted.ToString("x8") +
                "; rename=" + renamed + "; rename_error=" + renameError + "; native_rename_status=0x" +
                ((uint)nativeRename).ToString("x8") + "; new_child_denied=" + replacementDenied;
        } finally {
            if (sealedCreated) {
                uint restore = SetSecurityInfo(sealedDirectory, 1, 0x80000004, IntPtr.Zero, IntPtr.Zero, dacl, IntPtr.Zero);
                int nativeRestore = restore == 0 ? 0 : NtSetSecurityObject(sealedDirectory, 0x80000004, parentDescriptor);
                Console.WriteLine("acl_restore_error=" + restore + "; native_acl_restore_status=0x" + ((uint)nativeRestore).ToString("x8"));
            }
            if (leaf != IntPtr.Zero && leaf != new IntPtr(-1)) CloseHandle(leaf);
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
            Directory.Delete(resolvedFixture, false);
        }
    }
}
'@

[RecycleBindingProbe]::Run()
