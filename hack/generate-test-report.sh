#!/usr/bin/env bash
# generate-test-report.sh — Parse nextest JUnit XML into a human-readable report.
#
# Usage:
#   bash hack/generate-test-report.sh [junit.xml] [report.txt]
#
# Defaults:
#   junit.xml  = target/nextest/default/junit.xml (or ci/junit.xml if exists)
#   report.txt = test-report.txt
#
# The report includes FULL assertion details (left/right values, stderr, stdout)
# so you never need to re-run a test just to see what failed.

set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

# Find JUnit XML: use explicit arg, or pick the most recently modified profile
if [[ -n "${1:-}" ]]; then
  JUNIT="$1"
else
  JUNIT=""
  for candidate in "${PROJECT_DIR}"/target/nextest/*/junit.xml; do
    [[ -f "$candidate" ]] || continue
    if [[ -z "$JUNIT" ]] || [[ "$candidate" -nt "$JUNIT" ]]; then
      JUNIT="$candidate"
    fi
  done
  if [[ -z "$JUNIT" ]]; then
    echo "No JUnit XML found. Run tests first." >&2
    exit 1
  fi
fi

REPORT="${2:-${PROJECT_DIR}/test-report.txt}"

if [[ ! -f "$JUNIT" ]]; then
  echo "JUnit XML not found: ${JUNIT}" > "$REPORT"
  echo "JUnit XML not found: ${JUNIT}" >&2
  exit 1
fi

python3 - "$JUNIT" "$REPORT" <<'PYEOF'
import sys, xml.etree.ElementTree as ET

junit_path, report_path = sys.argv[1], sys.argv[2]
tree = ET.parse(junit_path)
root = tree.getroot()

passed, failed, errored, skipped = 0, 0, 0, 0
failures = []

def dedup_lines(text):
    """Remove duplicate lines while preserving order."""
    seen = set()
    result = []
    for line in text.splitlines():
        stripped = line.strip()
        if stripped and stripped not in seen:
            seen.add(stripped)
            result.append(line)
        elif not stripped:
            result.append(line)  # keep blank lines for readability
    return "\n".join(result)

def get_text(elem):
    """Safely get element text."""
    if elem is not None and elem.text is not None:
        return elem.text.strip()
    return ""

for suite in root.iter("testsuite"):
    for case in suite.findall("testcase"):
        name = case.get("name", "?")
        classname = case.get("classname", "")
        time_s = case.get("time", "")
        failure = case.find("failure")
        error = case.find("error")
        skip = case.find("skipped")
        rerun = case.find("flakyFailure")
        if rerun is None:
            rerun = case.find("rerunFailure")

        if failure is not None:
            failed += 1
            msg = failure.get("message", "")
            # Collect output from all sources, then deduplicate
            parts = []
            ft = get_text(failure)
            if ft:
                parts.append(ft)
            st = get_text(case.find("system-err"))
            if st:
                parts.append(st)
            ot = get_text(case.find("system-out"))
            if ot:
                parts.append(ot)
            if rerun is not None:
                rm = rerun.get("message", "")
                if rm:
                    parts.append(f"[retry] {rm}")
                rt = get_text(rerun)
                if rt:
                    parts.append(rt)

            detail = dedup_lines("\n".join(parts))
            # Don't repeat message if it's already in detail
            if msg and msg in detail:
                msg = ""
            failures.append((classname, name, time_s, msg, detail))
        elif error is not None:
            errored += 1
            msg = error.get("message", "error")
            detail = get_text(error)
            st = get_text(case.find("system-err"))
            if st:
                detail = detail + "\n" + st if detail else st
            failures.append((classname, name, time_s, msg, detail))
        elif skip is not None:
            skipped += 1
        else:
            passed += 1

total = passed + failed + errored
status = "PASS" if failed == 0 and errored == 0 else "FAIL"

with open(report_path, "w") as f:
    f.write(f"Test Report: {status}\n")
    f.write(f"{'=' * 72}\n")
    f.write(f"Total: {total}  Passed: {passed}  Failed: {failed}  Errors: {errored}")
    if skipped:
        f.write(f"  Skipped: {skipped}")
    f.write(f"\n\n")

    if failures:
        f.write(f"Failed Tests:\n")
        f.write(f"{'=' * 72}\n")
        for classname, name, time_s, msg, detail in failures:
            f.write(f"\n{'─' * 72}\n")
            f.write(f"FAIL: {name}")
            if time_s:
                f.write(f"  ({time_s}s)")
            f.write(f"\n")
            if classname:
                f.write(f"  in: {classname}\n")
            if msg:
                f.write(f"  message: {msg}\n")
            if detail:
                f.write(f"\n")
                for line in detail.splitlines():
                    f.write(f"  {line}\n")
            f.write(f"\n")
    else:
        f.write("All tests passed.\n")

print(f"Test report: {report_path} ({status}: {passed}/{total} passed)")
PYEOF
