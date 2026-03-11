#!/usr/bin/env python3

import re
import sys
from pathlib import Path


THRESHOLDS = {
    "src/kernel/run/persister.rs": 85.0,
    "src/kernel/skills/remote/repository.rs": 85.0,
    "src/kernel/skills/remote/sync.rs": 70.0,
    "src/kernel/skills/store.rs": 70.0,
    "src/kernel/skills/runner.rs": 70.0,
    "src/service/v1/skills/http.rs": 80.0,
    "src/service/v1/skills/service.rs": 80.0,
    "src/service/v1/variables/http.rs": 90.0,
    "src/service/v1/variables/service.rs": 90.0,
    "src/service/v1/runs/http.rs": 60.0,
    "src/service/v1/runs/service.rs": 65.0,
    "src/storage/dal/variable/repo.rs": 75.0,
    "src/storage/dal/run/repo.rs": 75.0,
    "src/storage/dal/run_event/repo.rs": 80.0,
}


LINE_RE = re.compile(
    r"^(.+?)\s+(\d+)\s+(\d+)\s+([0-9.]+)%\s+(\d+)\s+(\d+)\s+([0-9.]+)%\s+(\d+)\s+(\d+)\s+([0-9.]+)%"
)


def parse_summary(path: Path) -> dict[str, float]:
    lines = path.read_text().splitlines()
    result: dict[str, float] = {}
    for line in lines:
        match = LINE_RE.match(line.strip("\n"))
        if not match:
            continue
        filename = match.group(1).strip()
        line_cover = float(match.group(10))
        result[filename] = line_cover
    return result


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check_core_coverage.py <coverage-summary.txt>", file=sys.stderr)
        return 2

    summary_path = Path(sys.argv[1])
    actual = parse_summary(summary_path)

    missing: list[str] = []
    failures: list[tuple[str, float, float]] = []

    print("Core Coverage Gate")
    for filename, minimum in THRESHOLDS.items():
        key = filename.removeprefix("src/")
        if key not in actual:
            missing.append(filename)
            print(f"MISSING {filename}")
            continue
        value = actual[key]
        status = "OK" if value >= minimum else "FAIL"
        print(f"{status} {filename}: {value:.2f}% (min {minimum:.2f}%)")
        if value < minimum:
            failures.append((filename, value, minimum))

    if missing or failures:
        if missing:
            print("\nMissing files in coverage summary:", file=sys.stderr)
            for filename in missing:
                print(f"- {filename}", file=sys.stderr)
        if failures:
            print("\nCore coverage below threshold:", file=sys.stderr)
            for filename, value, minimum in failures:
                print(
                    f"- {filename}: {value:.2f}% < {minimum:.2f}%",
                    file=sys.stderr,
                )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
