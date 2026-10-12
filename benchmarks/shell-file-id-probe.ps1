$ErrorActionPreference = 'Stop'

# Controlled Shell identity experiment. No recycling or application files are involved.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Threading;
using System.Collections.Generic;

public static class ShellFileIdProbe {
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
    public struct FindData {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
        public uint SizeHigh, SizeLow, Reserved0, Reserved1;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=260)] public string Name;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst=14)] public string ShortName;
    }
    [StructLayout(LayoutKind.Sequential)] struct FileInformation {
        public uint Attributes;
        public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [ComVisible(true), Guid("01E18D10-4D8B-11D2-855D-006008059367"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IBindData {
        [PreserveSig] int SetFindData(ref FindData data);
        [PreserveSig] int GetFindData(out FindData data);
    }
    [ComVisible(true), Guid("3ACF075F-71DB-4AFA-81F0-3FC4FDF2A5B8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IBindData2 {
        [PreserveSig] int SetFindData(ref FindData data);
        [PreserveSig] int GetFindData(out FindData data);
        [PreserveSig] int SetFileID(long id);
        [PreserveSig] int GetFileID(out long id);
        [PreserveSig] int SetJunctionCLSID(ref Guid clsid);
        [PreserveSig] int GetJunctionCLSID(out Guid clsid);
    }
    [ComVisible(true), ClassInterface(ClassInterfaceType.None)]
    public sealed class BindData : IBindData, IBindData2 {
        public FindData Data;
        public long Id;
        public int FindReads, IdReads, IdWrites;
        public List<long> ReturnedIds = new List<long>();
        public int SetFindData(ref FindData data) { Data = data; return 0; }
        public int GetFindData(out FindData data) { FindReads++; data = Data; return 0; }
        public int SetFileID(long id) { IdWrites++; Id = id; return 0; }
        public int GetFileID(out long id) { IdReads++; id = Id; ReturnedIds.Add(id); return 0; }
        public int SetJunctionCLSID(ref Guid clsid) { return unchecked((int)0x80004001); }
        public int GetJunctionCLSID(out Guid clsid) { clsid = Guid.Empty; return unchecked((int)0x80004001); }
    }
    [ComImport, Guid("43826D1E-E718-42EE-BC55-A1E261C37BFE"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    public interface IShellItem {
        [PreserveSig] int BindToHandler(IBindCtx context, ref Guid handler, ref Guid iid, out IStream stream);
        [PreserveSig] int GetParent(out IShellItem parent);
        [PreserveSig] int GetDisplayName(uint mode, out IntPtr name);
        [PreserveSig] int GetAttributes(uint mask, out uint attributes);
        [PreserveSig] int Compare(IShellItem other, uint hint, out int order);
    }
    [DllImport("ole32.dll")] static extern int CoInitializeEx(IntPtr reserved, uint mode);
    [DllImport("ole32.dll")] static extern void CoUninitialize();
    [DllImport("ole32.dll")] static extern int CreateBindCtx(uint reserved, out IBindCtx context);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode)] static extern int SHCreateItemFromParsingName(
        string path, IBindCtx context, ref Guid iid, out IShellItem item);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr CreateFileW(
        string path, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetFileInformationByHandle(IntPtr file, out FileInformation info);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern IntPtr FindFirstFileW(string path, out FindData data);
    [DllImport("kernel32.dll")] static extern bool FindClose(IntPtr handle);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);

    static long Identity(string path) {
        var handle = CreateFileW(path, 0x80, 7, IntPtr.Zero, 3, 0x00200000, IntPtr.Zero);
        if (handle == new IntPtr(-1)) throw new IOException("Open: " + Marshal.GetLastWin32Error());
        try {
            FileInformation information;
            if (!GetFileInformationByHandle(handle, out information)) throw new IOException("Identity: " + Marshal.GetLastWin32Error());
            return unchecked((long)(((ulong)information.IndexHigh << 32) | information.IndexLow));
        } finally { CloseHandle(handle); }
    }
    static IShellItem Item(string path, BindData data) {
        IBindCtx context;
        Marshal.ThrowExceptionForHR(CreateBindCtx(0, out context));
        try {
            context.RegisterObjectParam("File System Bind Data", data);
            Guid iid = typeof(IShellItem).GUID;
            IShellItem item;
            Marshal.ThrowExceptionForHR(SHCreateItemFromParsingName(path, context, ref iid, out item));
            return item;
        } finally { Marshal.ReleaseComObject(context); }
    }
    static string Read(IShellItem item) {
        Guid handler = new Guid("1CEBB3AB-7C10-499A-A417-92CA16C4CB83");
        Guid iid = typeof(IStream).GUID;
        IStream stream;
        int status = item.BindToHandler(null, ref handler, ref iid, out stream);
        if (status < 0) return "HRESULT_0x" + unchecked((uint)status).ToString("x8");
        try {
            var bytes = new byte[32];
            var count = Marshal.AllocHGlobal(4);
            try {
                Marshal.WriteInt32(count, 0);
                stream.Read(bytes, bytes.Length, count);
                int read = Marshal.ReadInt32(count);
                if (read < 0 || read > bytes.Length) throw new IOException("Invalid stream length.");
                return System.Text.Encoding.ASCII.GetString(bytes, 0, read);
            } finally { Marshal.FreeHGlobal(count); }
        } finally { Marshal.ReleaseComObject(stream); }
    }
    static void CheckFixture(string fixture, string temporaryRoot) {
        if (!Path.GetFullPath(fixture).StartsWith(temporaryRoot + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase)
            || !Path.GetFileName(fixture).StartsWith("voldiq-shell-id-", StringComparison.Ordinal)
            || (File.GetAttributes(fixture) & FileAttributes.ReparsePoint) != 0)
            throw new IOException("Fixture no longer belongs to the expected temporary directory.");
    }
    static string Experiment() {
        var temporaryRoot = Path.GetFullPath(Path.GetTempPath()).TrimEnd(Path.DirectorySeparatorChar);
        var fixture = Path.Combine(temporaryRoot, "voldiq-shell-id-" + Guid.NewGuid().ToString("N"));
        var source = Path.Combine(fixture, "source.dat");
        var retained = Path.Combine(fixture, "retained.dat");
        Directory.CreateDirectory(fixture);
        IShellItem existing = null, reparsed = null;
        try {
            File.WriteAllBytes(source, System.Text.Encoding.ASCII.GetBytes("original"));
            long originalId = Identity(source);
            FindData findData;
            var find = FindFirstFileW(source, out findData);
            if (find == new IntPtr(-1)) throw new IOException("Find: " + Marshal.GetLastWin32Error());
            FindClose(find);
            var data = new BindData { Data = findData, Id = originalId };
            existing = Item(source, data);
            var baseline = Read(existing);
            if (baseline != "original") throw new IOException("Baseline stream failed: " + baseline);
            CheckFixture(fixture, temporaryRoot);
            File.Move(source, retained);
            File.WriteAllBytes(source, System.Text.Encoding.ASCII.GetBytes("replacement"));
            long replacementId = Identity(source);
            if (replacementId == originalId || Identity(retained) != originalId) throw new IOException("Fixture identities were not distinct and retained.");
            string existingRead = Read(existing);
            if (data.Id != originalId) throw new IOException("Shell changed the supplied identity before reparsing.");
            reparsed = Item(source, data);
            string reparsedRead = Read(reparsed);
            foreach (long returnedId in data.ReturnedIds)
                if (returnedId != originalId) throw new IOException("Shell received an unexpected file ID.");
            if (File.ReadAllText(retained) != "original" || File.ReadAllText(source) != "replacement") throw new IOException("Probe changed fixture content.");
            CheckFixture(fixture, temporaryRoot);
            File.Delete(source);
            string missingRead = Read(existing);
            if (File.ReadAllText(retained) != "original") throw new IOException("Retained original was changed.");
            return "os=" + Environment.OSVersion.Version + "; filesystem=" + new DriveInfo(Path.GetPathRoot(fixture)).DriveFormat
                + "; baseline=" + baseline + "; existing_item_stream=" + existingRead + "; reparsed_stale_id_stream=" + reparsedRead
                + "; missing_path_stream=" + missingRead
                + "; find_data_reads=" + data.FindReads + "; file_id_reads=" + data.IdReads
                + "; file_id_writes=" + data.IdWrites
                + "; returned_file_ids=" + string.Join(",", data.ReturnedIds.ConvertAll(id => "0x" + unchecked((ulong)id).ToString("x16")))
                + "; original_id=0x" + unchecked((ulong)originalId).ToString("x16")
                + "; replacement_id=0x" + unchecked((ulong)replacementId).ToString("x16");
        } finally {
            if (reparsed != null) Marshal.ReleaseComObject(reparsed);
            if (existing != null) Marshal.ReleaseComObject(existing);
            CheckFixture(fixture, temporaryRoot);
            File.Delete(source);
            File.Delete(retained);
            Directory.Delete(fixture, false);
        }
    }
    public static string Run() {
        string result = null;
        Exception failure = null;
        var thread = new Thread(() => {
            int status = CoInitializeEx(IntPtr.Zero, 2);
            if (status < 0) { failure = Marshal.GetExceptionForHR(status); return; }
            try { result = Experiment(); } catch (Exception error) { failure = error; }
            finally { CoUninitialize(); }
        });
        thread.SetApartmentState(ApartmentState.STA);
        thread.Start();
        thread.Join();
        if (failure != null) throw new InvalidOperationException("Shell file ID probe failed.", failure);
        return result;
    }
}
'@

[ShellFileIdProbe]::Run()
