// Owned disposable fixtures only. --undelete enumerates only to select its exact own item; no elevation or empty-bin action.
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <shobjidl.h>
#include <shlobj.h>
#include <shellapi.h>
#include <shlguid.h>
#include <knownfolders.h>
#include <wrl.h>
#include <cstdio>
#include <cstring>
#include <cwchar>
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

static ComPtr<IShellItem> VirtualBinItem(const std::wstring& path, FILE_ID_INFO expected, IContextMenu** menu) {
    ComPtr<IShellItem> bin;
    Check(SHGetKnownFolderItem(FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, nullptr, IID_PPV_ARGS(&bin)));
    ComPtr<IShellFolder2> folder;
    Check(bin->BindToHandler(nullptr, BHID_SFObject, IID_PPV_ARGS(&folder)));
    ComPtr<IEnumIDList> items;
    Check(folder->EnumObjects(nullptr, SHCONTF_FOLDERS | SHCONTF_NONFOLDERS, &items));
    ComPtr<IShellItem> selected;
    for (;;) {
        PITEMID_CHILD child = nullptr;
        ULONG fetched = 0;
        const auto next = items->Next(1, &child, &fetched);
        Check(next);
        if (next == S_FALSE || fetched == 0) break;
        ComPtr<IShellItem> item;
        const auto created = SHCreateItemWithParent(nullptr, folder.Get(), child, IID_PPV_ARGS(&item));
        if (FAILED(created)) { CoTaskMemFree(child); throw created; }
        PWSTR text = nullptr;
        if (FAILED(item->GetDisplayName(SIGDN_FILESYSPATH, &text))) { CoTaskMemFree(child); continue; }
        const bool matches = _wcsicmp(path.c_str(), text) == 0;
        CoTaskMemFree(text);
        if (!matches) { CoTaskMemFree(child); continue; }
        Require(!selected);
        auto metadata = Open(path, FILE_READ_ATTRIBUTES, 7);
        Require(Same(Identity(metadata.get()), expected));
        PCUITEMID_CHILD source = child;
        const auto menuResult = folder->GetUIObjectOf(nullptr, 1, &source, IID_IContextMenu, nullptr, reinterpret_cast<void**>(menu));
        CoTaskMemFree(child);
        Check(menuResult);
        selected = item;
    }
    Require(selected != nullptr);
    return selected;
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
    bool directMove = false, collisionPreserved = false, recoveryUsed = false;
    bool binMetadataPresent = false, binMetadataRemoved = false, orphanMetadataCleaned = false;
    bool virtualItem = false;
    bool undelete = false;
    HRESULT collisionResult = E_PENDING;
    DWORD writeError = 0, deleteFlags = 0;
    UINT deleteCalls = 0, restoreCalls = 0;
    HRESULT perform = E_PENDING, postDelete = E_PENDING, restorePerform = E_PENDING, postRestore = E_PENDING, error = S_OK;
};

static void RestoreObserved(IShellItem* recycled, const std::wstring& directory, bool guarded, Observation& result) {
    auto restoreSink = Microsoft::WRL::Make<Sink>();
    Require(restoreSink != nullptr);
    auto restore = Operation(FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_RENAMEONCOLLISION | FOFX_EARLYFAILURE);
    auto parent = Item(directory);
    Check(restore->MoveItem(recycled, parent.Get(), guarded ? L"guarded.bin" : L"baseline.bin", restoreSink.Get()));
    result.restorePerform = restore->PerformOperations();
    BOOL restoreAborted = TRUE;
    Check(restore->GetAnyOperationsAborted(&restoreAborted));
    result.postRestore = restoreSink->moveResult;
    result.restoreCalls = restoreSink->moveCalls;
    Require(SUCCEEDED(result.restorePerform) && !restoreAborted && result.restoreCalls == 1 && SUCCEEDED(result.postRestore));
}

static void DirectRestore(IShellItem* recycled, const std::wstring& directory, bool guarded,
    const std::wstring& path, const std::wstring& binPath, FILE_ID_INFO original, Observation& result) {
    ComPtr<IShellItem> binParent;
    Check(recycled->GetParent(&binParent));
    ComPtr<ITransferSource> transfer;
    Check(binParent->BindToHandler(nullptr, BHID_Transfer, IID_PPV_ARGS(&transfer)));
    auto destination = Item(directory);
    auto occupied = Open(path, GENERIC_READ | GENERIC_WRITE | DELETE, FILE_SHARE_READ, CREATE_NEW);
    const BYTE marker[] = {9, 8, 7, 6};
    DWORD written = 0;
    WinCheck(WriteFile(occupied.get(), marker, 4, &written, nullptr) != FALSE);
    Require(written == 4);
    const auto replacement = Identity(occupied.get());
    ComPtr<IShellItem> moved;
    const wchar_t* name = guarded ? L"guarded.bin" : L"baseline.bin";
    result.collisionResult = transfer->MoveItem(recycled, destination.Get(), name, TSF_FAIL_EXIST, &moved);
    auto remaining = Open(binPath, FILE_READ_ATTRIBUTES, 7);
    auto replacementAtPath = Open(path, FILE_READ_ATTRIBUTES, 7);
    Require(!moved && Same(Identity(remaining.get()), original) && Same(Identity(replacementAtPath.get()), replacement));
    remaining.reset(); replacementAtPath.reset();
    BYTE actual[4]{}; DWORD count = 0; LARGE_INTEGER zero{};
    WinCheck(SetFilePointerEx(occupied.get(), zero, nullptr, FILE_BEGIN) != FALSE);
    WinCheck(ReadFile(occupied.get(), actual, 4, &count, nullptr) != FALSE);
    Require(count == 4 && std::memcmp(actual, marker, 4) == 0);
    result.collisionPreserved = true;
    FILE_DISPOSITION_INFO disposition{TRUE};
    WinCheck(SetFileInformationByHandle(occupied.get(), FileDispositionInfo, &disposition, sizeof(disposition)) != FALSE);
    occupied.reset();
    Require(!Exists(path));
    result.restorePerform = transfer->MoveItem(recycled, destination.Get(), name, TSF_FAIL_EXIST, &moved);
    Check(result.restorePerform);
    Require(moved != nullptr);
    PWSTR movedPath = nullptr;
    Check(moved->GetDisplayName(SIGDN_FILESYSPATH, &movedPath));
    const std::wstring actualPath(movedPath);
    CoTaskMemFree(movedPath);
    Require(_wcsicmp(actualPath.c_str(), path.c_str()) == 0);
    result.postRestore = result.restorePerform;
    result.directMove = true;
}

static HRESULT RestoreVerb(IContextMenu* menu) {
    const auto popup = CreatePopupMenu();
    WinCheck(popup != nullptr);
    const auto queried = menu->QueryContextMenu(popup, 0, 1, 0x7fff, CMF_NORMAL);
    if (FAILED(queried)) { DestroyMenu(popup); throw queried; }
    CMINVOKECOMMANDINFO info{};
    info.cbSize = sizeof(info);
    info.fMask = CMIC_MASK_FLAG_NO_UI | CMIC_MASK_NOASYNC;
    info.lpVerb = "undelete";
    info.nShow = SW_HIDE;
    const auto result = menu->InvokeCommand(&info);
    DestroyMenu(popup);
    return result;
}

static Observation Experiment(const std::wstring& directory, bool guarded, bool direct, bool directMove, bool undelete) {
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
        auto source = Item(path);
        if (direct) {
            auto parent = Item(directory);
            ComPtr<ITransferSource> transfer;
            Check(parent->BindToHandler(nullptr, BHID_Transfer, IID_PPV_ARGS(&transfer)));
            ComPtr<IShellItem> bin;
            Check(SHGetKnownFolderItem(FOLDERID_RecycleBinFolder, KF_FLAG_DEFAULT, nullptr, IID_PPV_ARGS(&bin)));
            result.perform = transfer->RecycleItem(source.Get(), bin.Get(), TSF_NORMAL, &sink->recycled);
            result.postDelete = result.perform;
            result.aborted = FAILED(result.perform);
        } else {
            auto operation = Operation(FOF_SILENT | FOF_NOCONFIRMATION | FOF_NOERRORUI |
                FOF_NO_CONNECTED_ELEMENTS | FOF_NORECURSION | FOFX_RECYCLEONDELETE | FOFX_EARLYFAILURE);
            Check(operation->DeleteItem(source.Get(), sink.Get()));
            result.perform = operation->PerformOperations();
            BOOL aborted = TRUE;
            Check(operation->GetAnyOperationsAborted(&aborted));
            result.aborted = aborted != FALSE;
            result.postDelete = sink->deleteResult;
            result.deleteCalls = sink->deleteCalls;
            result.deleteFlags = sink->deleteFlags;
        }
        result.sourceAfter = Exists(path);
        result.recycled = SUCCEEDED(result.perform) && !result.aborted && (direct || sink->deleteCalls == 1) &&
            SUCCEEDED(result.postDelete) && sink->recycled != nullptr && !result.sourceAfter;
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
            const auto separator = retainedPath.find_last_of(L"\\/");
            Require(separator != std::wstring::npos && retainedPath.substr(separator + 1, 2) == L"$R");
            std::wstring metadataPath = retainedPath;
            metadataPath[separator + 2] = L'I';
            auto binMetadata = Open(metadataPath, DELETE | FILE_READ_DATA | FILE_READ_ATTRIBUTES, 7);
            const auto expectedMetadata = Identity(binMetadata.get());
            binMetadata.reset();
            result.binMetadataPresent = true;
            auto inBin = Open(retainedPath, FILE_READ_ATTRIBUTES, 7);
            result.binIdentityMatches = Same(original, Identity(inBin.get()));
            inBin.reset();
            Require(result.binIdentityMatches && !Exists(path));
            ComPtr<IContextMenu> restoreMenu;
            if (undelete) {
                sink->recycled = VirtualBinItem(retainedPath, original, &restoreMenu);
                result.virtualItem = true;
            }
            if (undelete) {
                result.restorePerform = RestoreVerb(restoreMenu.Get());
                Check(result.restorePerform);
                result.postRestore = result.restorePerform;
                result.undelete = true;
            } else if (directMove) {
                try {
                    DirectRestore(sink->recycled.Get(), directory, guarded, path, retainedPath, original, result);
                } catch (HRESULT error) {
                    result.error = error;
                    if (!Exists(path)) {
                        auto retained = Open(retainedPath, FILE_READ_ATTRIBUTES, 7);
                        Require(Same(original, Identity(retained.get())));
                        retained.reset();
                        result.recoveryUsed = true;
                        RestoreObserved(sink->recycled.Get(), directory, guarded, result);
                    }
                }
            } else {
                RestoreObserved(sink->recycled.Get(), directory, guarded, result);
            }
            auto restored = Open(path, GENERIC_READ, 7);
            Require(Same(original, Identity(restored.get())));
            ReadOriginal(restored.get());
            result.restored = true;
            result.contentPreserved = true;
            restoreMenu.Reset();
            sink->recycled.Reset();
            result.binMetadataRemoved = !Exists(metadataPath);
            if (!result.binMetadataRemoved) {
                auto currentMetadata = Open(metadataPath, DELETE | FILE_READ_ATTRIBUTES, FILE_SHARE_READ);
                Require(Same(expectedMetadata, Identity(currentMetadata.get())));
                FILE_DISPOSITION_INFO disposition{TRUE};
                WinCheck(SetFileInformationByHandle(currentMetadata.get(), FileDispositionInfo, &disposition, sizeof(disposition)) != FALSE);
                currentMetadata.reset();
                result.orphanMetadataCleaned = !Exists(metadataPath);
                Require(result.orphanMetadataCleaned);
            }
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
        "\"restored\":%s,\"contentPreserved\":%s,\"fixtureFileCleaned\":%s,\"error\":\"0x%08lx\","
        "\"directMove\":%s,\"collisionPreserved\":%s,\"recoveryUsed\":%s,\"collisionHr\":\"0x%08lx\","
        "\"binMetadataPresent\":%s,\"binMetadataRemoved\":%s,\"orphanMetadataCleaned\":%s,\"virtualItem\":%s,\"undelete\":%s}",
        item.guarded ? "true" : "false", static_cast<unsigned long>(item.perform), static_cast<unsigned long>(item.postDelete),
        item.deleteCalls, item.deleteFlags, item.aborted ? "true" : "false", item.sourceAfter ? "true" : "false",
        item.recycled ? "true" : "false", item.binIdentityMatches ? "true" : "false", item.writeError,
        static_cast<unsigned long>(item.restorePerform), static_cast<unsigned long>(item.postRestore), item.restoreCalls,
        item.restored ? "true" : "false", item.contentPreserved ? "true" : "false", item.cleanup ? "true" : "false", static_cast<unsigned long>(item.error),
        item.directMove ? "true" : "false", item.collisionPreserved ? "true" : "false", item.recoveryUsed ? "true" : "false", static_cast<unsigned long>(item.collisionResult),
        item.binMetadataPresent ? "true" : "false", item.binMetadataRemoved ? "true" : "false", item.orphanMetadataCleaned ? "true" : "false", item.virtualItem ? "true" : "false", item.undelete ? "true" : "false");
}

int wmain(int argc, wchar_t** argv) {
    const bool undelete = argc == 2 && std::wcscmp(argv[1], L"--undelete") == 0;
    const bool directMove = argc == 2 && std::wcscmp(argv[1], L"--direct-move") == 0;
    const bool direct = undelete || directMove || (argc == 2 && std::wcscmp(argv[1], L"--direct") == 0);
    if (argc != 1 && !direct) return 2;
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
        Observation baseline = Experiment(fixture, false, direct, directMove, undelete);
        Observation guarded{};
        if (baseline.recycled && baseline.restored && baseline.cleanup && SUCCEEDED(baseline.error))
            guarded = Experiment(fixture, true, direct, directMove, undelete);
        DWORD attributes = GetFileAttributesW(fixture.c_str());
        bool cleanup = attributes != INVALID_FILE_ATTRIBUTES && !(attributes & FILE_ATTRIBUTE_REPARSE_POINT) &&
            RemoveDirectoryW(fixture.c_str()) != FALSE;
        std::printf("{\"directRecycleItem\":%s,\"fixtureName\":\"", direct ? "true" : "false");
        for (wchar_t character : name) std::putchar(static_cast<char>(character));
        std::printf("\",\"baseline\":"); Print(baseline);
        std::printf(",\"guarded\":"); Print(guarded);
        std::printf(",\"fixtureDirectoryCleaned\":%s}\n", cleanup ? "true" : "false");
        exit = cleanup && baseline.recycled && baseline.restored && baseline.cleanup &&
            guarded.recycled && guarded.restored && guarded.cleanup &&
            (!directMove || (baseline.directMove && guarded.directMove && baseline.collisionPreserved && guarded.collisionPreserved)) &&
            SUCCEEDED(baseline.error) && SUCCEEDED(guarded.error) ? 0 : 1;
    } catch (HRESULT error) { std::printf("{\"fatalError\":\"0x%08lx\"}\n", static_cast<unsigned long>(error)); }
    CoUninitialize();
    return exit;
}
