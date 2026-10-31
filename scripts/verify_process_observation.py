"""Check production-window provenance and independently recompute process counters."""
import csv
import hashlib
import json
import math
import statistics
import sys
from datetime import datetime
from pathlib import Path


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def read_csv(path):
    with path.open(encoding="utf-8-sig", newline="") as source:
        return list(csv.DictReader(source))


def identity(row):
    return int(row["processId"]), row["startedUtc"]


def spread(values):
    return {"median": statistics.median(values), "minimum": min(values), "maximum": max(values)}


def verify(path):
    path = path.resolve()
    assert path.is_relative_to(Path(__file__).resolve().parent.parent), "Keep captures in the repository"
    rows = read_csv(path)
    process_path = path.with_suffix(".processes.csv")
    provenance_path = path.with_suffix(".provenance.json")
    processes = read_csv(process_path)
    provenance = json.loads(provenance_path.read_text(encoding="utf-8-sig"))
    assert provenance["embeddedUi"]["probe"] == "VOLDIQ_EMBEDDED_UI_V1"
    assert provenance["embeddedUi"]["development"] is False
    # Archived preview images may be removed by routine compilation-cache cleanup.
    executable = Path(provenance["executable"])
    if executable.exists():
        assert digest(executable).upper() == provenance["executableSha256"]
    assert len(rows) >= 6, "Need a baseline and at least five intervals"
    assert [int(row["sample"]) for row in rows] == list(range(len(rows)))
    target = provenance["targetPid"]
    profile_before = {item["pid"]: item for item in provenance["profileWebviewsBefore"]}
    profile_after = {item["pid"]: item for item in provenance["profileWebviewsAfter"]}
    for metadata in [*profile_before.values(), *profile_after.values()]:
        metadata["startedUtc"] = datetime.fromisoformat(metadata["startedUtc"])
    assert profile_before == profile_after, "Profile membership changed"
    for pid in profile_before:
        seen = set()
        while pid != target:
            assert pid in profile_before and pid not in seen, "WebView is not a root descendant"
            seen.add(pid)
            pid = profile_before[pid]["parent"]
    expected_pids = {target, *profile_before}
    assert len(processes) == len(rows) * len(expected_pids)
    previous = {}
    identities = None
    for sample, row in enumerate(rows):
        members = [item for item in processes if int(item["sample"]) == sample]
        current = {identity(item): float(item["cpuSeconds"]) for item in members}
        assert len(current) == len(expected_pids) == len(members)
        assert {int(item["processId"]) for item in members} == expected_pids
        assert int(row["processCount"]) == len(expected_pids)
        assert not int(row["readErrors"])
        assert not int(row["noLongerMeasuredProcesses"])
        assert int(row["newlyMeasuredProcesses"]) == (len(expected_pids) if sample == 0 else 0)
        if identities is None:
            identities = set(current)
        assert set(current) == identities, "Process identity changed"
        native = 0
        webview = 0
        for item in members:
            pid = int(item["processId"])
            assert item["observedUtc"] == row["observedUtc"]
            if pid == target:
                assert item["name"] == "voldiq.exe"
                assert datetime.fromisoformat(item["startedUtc"]) == datetime.fromisoformat(provenance["targetStartedUtc"])
                native += int(item["privateBytes"])
            else:
                assert item["name"] == "msedgewebview2.exe"
                metadata = profile_before[pid]
                assert int(item["parentProcessId"]) == metadata["parent"]
                # CIM rounds creation time to microseconds; Process exposes 100 ns ticks.
                assert datetime.fromisoformat(item["startedUtc"]) == metadata["startedUtc"]
                webview += int(item["privateBytes"])
        assert int(row["nativePrivateBytes"]) == native
        assert int(row["webviewPrivateBytes"]) == webview
        assert int(row["totalPrivateBytes"]) == native + webview
        assert int(row["summedWorkingSetBytes"]) == sum(int(item["workingSetBytes"]) for item in members)
        if sample:
            assert all(value >= previous[key] for key, value in current.items())
            delta = sum(value - previous[key] for key, value in current.items())
            assert delta >= 0
            assert math.isclose(float(row["survivingCpuSeconds"]), delta, abs_tol=1e-9)
            expected_cpu = 100 * delta / float(row["intervalSeconds"])
            assert math.isclose(float(row["survivingOneCoreCpuPercent"]), expected_cpu, abs_tol=1e-9)
        previous = current
    measured = rows[1:]
    counters = ("nativePrivateBytes", "webviewPrivateBytes", "totalPrivateBytes", "summedWorkingSetBytes")
    result = {
        "scope": provenance["intendedClaim"],
        "baselineSamples": 1,
        "measuredIntervals": len(measured),
        "identitiesPerSample": len(expected_pids),
        "counterRows": len(processes),
        "readErrors": 0,
        "availabilityChangesAfterBaseline": 0,
        "summary": {key: spread([int(row[key]) for row in measured]) for key in counters},
        "intervalSeconds": spread([float(row["intervalSeconds"]) for row in measured]),
        "oneCoreCpuPercent": spread([float(row["survivingOneCoreCpuPercent"]) for row in measured]),
        "webviewPrivateFraction": statistics.median(int(row["webviewPrivateBytes"]) / int(row["totalPrivateBytes"]) for row in measured),
        "groupSha256": digest(path),
        "processesSha256": digest(process_path),
        "provenanceSha256": digest(provenance_path),
        "verifierSha256": digest(Path(__file__)),
        "limitations": "One existing-window capture. View, foreground/minimized and loaded-index states were not inspected. Background load was uncontrolled. Summed working sets are not unique physical RAM. No controlled idle/loaded, startup, FPS, speedup or leak claim.",
    }
    output = path.with_suffix(".summary.json")
    if output.exists():
        assert json.loads(output.read_text(encoding="utf-8")) == result, "Existing summary differs"
    else:
        with output.open("x", encoding="utf-8") as destination:
            json.dump(result, destination, indent=2)
            destination.write("\n")
    return result


if __name__ == "__main__":
    if sys.flags.optimize:
        raise SystemExit("Run without Python -O; validity checks must stay enabled.")
    result = verify(Path(sys.argv[1]))
    print(json.dumps({"intervals": result["measuredIntervals"], "summary": result["summary"], "cpu": result["oneCoreCpuPercent"]}))
