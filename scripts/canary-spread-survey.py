#!/usr/bin/env python3
"""Survey the canary's observed spread across every recorded run.

`CANARY_TOLERANCE_PCT` (25) was a placeholder: it was chosen before there was
any data on what an *uncontaminated* run's spread actually looks like, and
`known-issues.md` has carried a note to retune it from real measurements ever
since. This prints the distribution needed to do that.

The point it is easy to get wrong -- and the reason this script prints the
provenance columns rather than just a histogram -- is that a threshold
separating "clean" from "contaminated" can only be fitted to runs whose load
state is *known*. The history file records the measurement, not whether anyone
was compiling on the host at the time. A high spread in a run of unknown
provenance is not evidence that the tolerance is too tight; it is equally
consistent with the run having been genuinely contaminated, which is the
detector working. Fitting a threshold to that data would widen the tolerance
until the detector stopped firing, and call the silence a clean bill of health.

So this classifies each record into three provenance classes and refuses to
pool them:

* **known-loaded** -- produced by `scripts/canary-load-test.sh`, i.e. six CPU
  spinners were running by construction. These are the positive controls.
* **known-idle** -- runs the operator confirmed idle at the time.
* **unknown** -- everything else. The majority, and unusable for fitting a
  threshold in either direction.

Usage: python scripts/canary-spread-survey.py [path/to/history.jsonl]
"""

import json
import sys
from pathlib import Path

# Runs demonstrated to be under load: the three `canary-load-test.sh` runs of
# 2026-08-14, identified by their measured spread and commit. Listed explicitly
# rather than detected, because nothing in the record marks a run as loaded --
# which is itself worth fixing (see the note printed at the end).
KNOWN_LOADED_SPREADS = {53, 117, 128}


def classify(record):
    canary = record.get("canary") or {}
    spread = canary.get("spread")
    if spread is None:
        return "no-canary"
    if spread in KNOWN_LOADED_SPREADS:
        return "known-loaded"
    return "unknown"


def main():
    path = Path(sys.argv[1] if len(sys.argv) > 1 else "bench/history.jsonl")
    if not path.exists():
        print(f"no history at {path}", file=sys.stderr)
        return 1

    buckets = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        record = json.loads(line)
        if record.get("profile") != "release":
            # Debug-profile runs measure a different binary entirely: both arms
            # of the canary's A/B are mostly loop scaffolding there, so its
            # spread is not the same quantity. Excluded, not pooled.
            continue
        buckets.setdefault(classify(record), []).append(record)

    for name in ("known-loaded", "unknown", "no-canary"):
        records = buckets.get(name, [])
        print(f"\n=== {name}: {len(records)} release run(s) ===")
        if not records:
            continue
        spreads = []
        for r in records:
            c = r.get("canary") or {}
            spread = c.get("spread")
            if spread is not None:
                spreads.append(spread)
            print(f"  {r.get('timestamp','?')}  {r.get('commit','?')[:9]}  "
                  f"spread={spread!s:>5}  samples={c.get('samples')!s:>3}  "
                  f"invalid={c.get('invalid')!s:>3}  "
                  f"min_centi={c.get('min_centi')!s:>5} "
                  f"max_centi={c.get('max_centi')!s:>5}")
        if spreads:
            spreads.sort()
            print(f"  -> min={spreads[0]} median={spreads[len(spreads)//2]} "
                  f"max={spreads[-1]}  over-25%={sum(1 for s in spreads if s > 25)}"
                  f"/{len(spreads)}")

    print("\nNote: only the 'known-loaded' class has established provenance. A "
          "threshold cannot honestly be fitted until 'known-idle' runs are "
          "distinguishable from 'unknown' ones -- which needs the harness to "
          "record the host's load state alongside the measurement, not a "
          "judgement call made afterwards from the number itself.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
