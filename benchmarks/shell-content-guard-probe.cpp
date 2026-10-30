// Owned disposable fixtures only. No arguments, elevation, bin enumeration or empty-bin action.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shobjidl.h>
#include <shellapi.h>
#include <wrl.h>
#include <cstdio>
#include <cstring>
#include <memory>
#include <string>

using Microsoft::WRL::ComPtr;
using Microsoft::WRL::RuntimeClass;
using Microsoft::WRL::RuntimeClassFlags;
using Microsoft::WRL::ClassicCom;

static void Check(HRESULT hr) { if (FAILED(hr)) throw hr; }
static void Require(bool condition) { if (!condition) throw E_UNEXPECTED; }
static void WinCheck(bool success) { if (!success) throw HRESULT_FROM_WIN32(GetLastError()); }
struct Close { void operator()(void* handle) const { if (handle && handle != INVALID_HANDLE_VALUE) CloseHandle(handle); } };
using Handle = std::unique_ptr<void, Close>;

static Handle Open(const std::wstring& path, DWORD access, DWORD share, DWORD creation = OPEN_EXISTING) {
    HANDLE handle = CreateFileW(path.c_str(), access, share, nullptr, creation, FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
    WinCheck(handle != INVALID_HANDLE_VALUE);
    return Handle(handle);
}
static FILE_ID_INFO Identity(HANDLE handle) {
    FILE_ID_INFO id{};
    WinCheck(GetFileInformationByHandleEx(handle, FileIdInfo, &id, sizeof(id)) != FALSE);
    return id;
}
static bool Same(FILE_ID_INFO left, FILE_ID_INFO right) {
    return left.VolumeSerialNumber == right.VolumeSerialNumber &&
        std::memcmp(left.FileId.Identifier, right.FileId.Identifier, 16) == 0;
}
static bool Exists(const std::wstring& path) {
    if (GetFileAttributesW(path.c_str()) != INVALID_FILE_ATTRIBUTES) return true;
    DWORD error = GetLastError();
    if (error != ERROR_FILE_NOT_FOUND) throw HRESULT_FROM_WIN32(error);
    return false;
}
static void ReadOriginal(HANDLE handle) {
    LARGE_INTEGER zero{}, size{};
    WinCheck(GetFileSizeEx(handle, &size) != FALSE);
    Require(size.QuadPart == 4);
    WinCheck(SetFilePointerEx(handle, zero, nullptr, FILE_BEGIN) != FALSE);
    BYTE bytes[4]{};
    DWORD count = 0;
    WinCheck(ReadFile(handle, bytes, 4, &count, nullptr) != FALSE);
    Require(count == 4 && bytes[0] == 1 && bytes[1] == 2 && bytes[2] == 3 && bytes[3] == 4);
}
static ComPtr<IShellItem> Item(const std::wstring& path) {
    ComPtr<IShellItem> item;
    Check(SHCreateItemFromParsingName(path.c_str(), nullptr, IID_PPV_ARGS(&item)));
    return item;
}

class Sink final : public RuntimeClass<RuntimeClassFlags<ClassicCom>, IFileOperationProgressSink> {
public:
    HRESULT deleteResult = E_PENDING, moveResult = E_PENDING;
    UINT deleteCalls = 0, moveCalls = 0;
    DWORD deleteFlags = 0;
    ComPtr<IShellItem> recycled;
    IFACEMETHODIMP StartOperations() override { return S_OK; }
    IFACEMETHODIMP FinishOperations(HRESULT) override { return S_OK; }
    IFACEMETHODIMP PreRenameItem(DWORD, IShellItem*, LPCWSTR) override { return E_ABORT; }
    IFACEMETHODIMP PostRenameItem(DWORD, IShellItem*, LPCWSTR, HRESULT, IShellItem*) override { return S_OK; }
    IFACEMETHODIMP PreMoveItem(DWORD flags, IShellItem*, IShellItem*, LPCWSTR) override {
        return (flags & TSF_OVERWRITE_EXIST) ? E_ABORT : S_OK;
    }
    IFACEMETHODIMP PostMoveItem(DWORD, IShellItem*, IShellItem*, LPCWSTR, HRESULT hr, IShellItem*) override {
        moveCalls++; moveResult = hr; return S_OK;
    }
    IFACEMETHODIMP PreCopyItem(DWORD, IShellItem*, IShellItem*, LPCWSTR) override { return E_ABORT; }
    IFACEMETHODIMP PostCopyItem(DWORD, IShellItem*, IShellItem*, LPCWSTR, HRESULT, IShellItem*) override { return S_OK; }
    IFACEMETHODIMP PreDeleteItem(DWORD flags, IShellItem*) override {
        deleteFlags = flags;
        return (flags & TSF_DELETE_RECYCLE_IF_POSSIBLE) ? S_OK : E_ABORT;
    }
    IFACEMETHODIMP PostDeleteItem(DWORD, IShellItem*, HRESULT hr, IShellItem* created) override {
        deleteCalls++; deleteResult = hr; recycled = created; return S_OK;
    }
    IFACEMETHODIMP PreNewItem(DWORD, IShellItem*, LPCWSTR) override { return E_ABORT; }
    IFACEMETHODIMP PostNewItem(DWORD, IShellItem*, LPCWSTR, LPCWSTR, DWORD, HRESULT, IShellItem*) override { return S_OK; }
    IFACEMETHODIMP UpdateProgress(UINT, UINT) override { return S_OK; }
    IFACEMETHODIMP ResetTimer() override { return S_OK; }
    IFACEMETHODIMP PauseTimer() override { return S_OK; }
    IFACEMETHODIMP ResumeTimer() override { return S_OK; }
};

static ComPtr<IFileOperation> Operation(DWORD flags) {
    ComPtr<IFileOperation> operation;
    Check(CoCreateInstance(CLSID_FileOperation, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&operation)));
    Check(operation->SetOperationFlags(flags));
    return operation;
}

struct Observation {
    bool guarded = false, recycled = false, restored = false, contentPreserved = false, cleanup = false;
    bool binIdentityMatches = false, sourceAfter = true, aborted = true;
    DWORD writeError = 0, deleteFlags = 0;
    UINT deleteCalls = 0, restoreCalls = 0;
    HRESULT perform = E_PENDING, postDelete = E_PENDING, restorePerform = E_PENDING, postRestore = E_PENDING, error = S_OK;
};

static Observation Experiment(const std::wstring& directory, bool guarded) {
    Observation result{};
    result.guarded = guarded;
    const auto path = directory + (guarded ? L"\\guarded.bin" : L"\\baseline.bin");
    FILE_ID_INFO original{};
    bool created = false;
    Handle guard;
    try {
        {
            auto writer = Open(path, GENERIC_WRITE | FILE_READ_ATTRIBUTES, 0, CREATE_NEW);
            const BYTE bytes[] = {1, 2, 3, 4};
            DWORD count = 0;
            WinCheck(WriteFile(writer.get(), bytes, 4, &count, nullptr) != FALSE);
            Require(count == 4);
            original = Identity(writer.get());
            created = true;
        }
        if (guarded) {
            auto strict = Open(path, DELETE | FILE_READ_ATTRIBUTES, FILE_SHARE_READ);
            Require(Same(original, Identity(strict.get())));
            HANDLE reopened = ReOpenFile(strict.get(), GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_DELETE, FILE_FLAG_OPEN_REPARSE_POINT);
            WinCheck(reopened != INVALID_HANDLE_VALUE);
            guard.reset(reopened);
            Require(Same(original, Identity(guard.get())));
            strict.reset();
            HANDLE writer = CreateFileW(path.c_str(), GENERIC_WRITE, 7, nullptr, OPEN_EXISTING, FILE_FLAG_OPEN_REPARSE_POINT, nullptr);
            result.writeError = writer == INVALID_HANDLE_VALUE ? GetLastError() : 0;
            if (writer != INVALID_HANDLE_VALUE) CloseHandle(writer);
            Require(result.writeError == ERROR_SHARING_VIOLATION);
        }
        auto sink = Microsoft::WRL::Make<Sink>();
        Require(sink != nullptr);
        auto operation = Operation(FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI |
            FOF_NO_CONNECTED_ELEMENTS | FOF_NORECURSION | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE);
        auto source = Item(path);
        Check(operation->DeleteItem(source.Get(), sink.Get()));
        result.perform = operation->PerformOperations();
        BOOL aborted = TRUE;
        Check(operation->GetAnyOperationsAborted(&aborted));
        result.aborted = aborted != FALSE;
        result.postDelete = sink->deleteResult;
        result.deleteCalls = sink->deleteCalls;
        result.deleteFlags = sink->deleteFlags;
        result.sourceAfter = Exists(path);
        result.recycled = SUCCEEDED(result.perform) && !result.aborted && sink->deleteCalls == 1 &&
            SUCCEEDED(sink->deleteResult) && sink->recycled != nullptr && !result.sourceAfter;
        if (guarded) {
            Require(Same(original, Identity(guard.get())));
            ReadOriginal(guard.get());
        }
        result.contentPreserved = guarded;
        if (sink->recycled) {
            PWSTR binPath = nullptr;
            Check(sink->recycled->GetDisplayName(SIGDN_FILESYSPATH, &binPath));
            std::wstring retainedPath(binPath);
            CoTaskMemFree(binPath);
            auto inBin = Open(retainedPath, FILE_READ_ATTRIBUTES, 7);
            result.binIdentityMatches = Same(original, Identity(inBin.get()));
            inBin.reset();
            Require(result.binIdentityMatches && !Exists(path));
            auto restoreSink = Microsoft::WRL::Make<Sink>();
            Require(restoreSink != nullptr);
            auto restore = Operation(FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_RENAMEONCOLLISION | FOFX_EARLYFAILURE);
            auto parent = Item(directory);
            Check(restore->MoveItem(sink->recycled.Get(), parent.Get(), guarded ? L"guarded.bin" : L"baseline.bin", restoreSink.Get()));
            result.restorePerform = restore->PerformOperations();
            BOOL restoreAborted = TRUE;
            Check(restore->GetAnyOperationsAborted(&restoreAborted));
            result.postRestore = restoreSink->moveResult;
            result.restoreCalls = restoreSink->moveCalls;
            Require(SUCCEEDED(result.restorePerform) && !restoreAborted && result.restoreCalls == 1 && SUCCEEDED(result.postRestore));
            auto restored = Open(path, GENERIC_READ, 7);
            Require(Same(original, Identity(restored.get())));
            ReadOriginal(restored.get());
            result.restored = true;
            result.contentPreserved = true;
        } else {
            Require(!result.recycled && result.sourceAfter);
            auto remaining = Open(path, GENERIC_READ, 7);
            Require(Same(original, Identity(remaining.get())));
            ReadOriginal(remaining.get());
            result.contentPreserved = true;
        }
    } catch (HRESULT error) { result.error = error; }
    guard.reset();
    try {
        if (created && Exists(path)) {
            auto owned = Open(path, DELETE | FILE_READ_ATTRIBUTES, FILE_SHARE_READ);
            Require(Same(original, Identity(owned.get())));
            FILE_DISPOSITION_INFO disposition{TRUE};
            WinCheck(SetFileInformationByHandle(owned.get(), FileDispositionInfo, &disposition, sizeof(disposition)) != FALSE);
        }
        result.cleanup = !Exists(path) && (result.restored || result.sourceAfter);
    } catch (HRESULT error) { if (SUCCEEDED(result.error)) result.error = error; }
    return result;
}

static void Print(const Observation& item) {
    std::printf("{\"guarded\":%s,\"performHr\":\"0x%08lx\",\"postDeleteHr\":\"0x%08lx\","
        "\"deleteCallbacks\":%u,\"deleteFlags\":%lu,\"aborted\":%s,\"sourceRemained\":%s,"
        "\"recycled\":%s,\"binIdentityMatched\":%s,\"writeOpenError\":%lu,"
        "\"restorePerformHr\":\"0x%08lx\",\"postRestoreHr\":\"0x%08lx\",\"restoreCallbacks\":%u,"
        "\"restored\":%s,\"contentPreserved\":%s,\"fixtureFileCleaned\":%s,\"error\":\"0x%08lx\"}",
        item.guarded ? "true" : "false", static_cast<unsigned long>(item.perform), static_cast<unsigned long>(item.postDelete),
        item.deleteCalls, item.deleteFlags, item.aborted ? "true" : "false", item.sourceAfter ? "true" : "false",
        item.recycled ? "true" : "false", item.binIdentityMatches ? "true" : "false", item.writeError,
        static_cast<unsigned long>(item.restorePerform), static_cast<unsigned long>(item.postRestore), item.restoreCalls,
        item.restored ? "true" : "false", item.contentPreserved ? "true" : "false", item.cleanup ? "true" : "false", static_cast<unsigned long>(item.error));
}

int wmain(int argc, wchar_t**) {
    if (argc != 1) return 2;
    HRESULT initialized = CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    if (FAILED(initialized)) return 3;
    int exit = 1;
    try {
        wchar_t temporary[32768]{}, suffix[40]{};
        DWORD length = GetTempPathW(32768, temporary);
        Require(length > 0 && length < 32768);
        GUID guid{};
        Check(CoCreateGuid(&guid));
        Require(StringFromGUID2(guid, suffix, 40) > 0);
        const std::wstring name = std::wstring(L"voldiq-shell-guard-") + suffix;
        const std::wstring fixture = std::wstring(temporary) + name;
        WinCheck(CreateDirectoryW(fixture.c_str(), nullptr) != FALSE);
        Observation baseline = Experiment(fixture, false);
        Observation guarded{};
        if (baseline.recycled && baseline.restored && baseline.cleanup && SUCCEEDED(baseline.error))
            guarded = Experiment(fixture, true);
        DWORD attributes = GetFileAttributesW(fixture.c_str());
        bool cleanup = attributes != INVALID_FILE_ATTRIBUTES && !(attributes & FILE_ATTRIBUTE_REPARSE_POINT) &&
            RemoveDirectoryW(fixture.c_str()) != FALSE;
        std::printf("{\"fixtureName\":\"");
        for (wchar_t character : name) std::putchar(static_cast<char>(character));
        std::printf("\",\"baseline\":"); Print(baseline);
        std::printf(",\"guarded\":"); Print(guarded);
        std::printf(",\"fixtureDirectoryCleaned\":%s}\n", cleanup ? "true" : "false");
        exit = cleanup && baseline.cleanup && guarded.cleanup && SUCCEEDED(baseline.error) && SUCCEEDED(guarded.error) ? 0 : 1;
    } catch (HRESULT error) { std::printf("{\"fatalError\":\"0x%08lx\"}\n", static_cast<unsigned long>(error)); }
    CoUninitialize();
    return exit;
}
