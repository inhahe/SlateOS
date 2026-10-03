#!/bin/python3
"""Run every Oils spec file on this machine and compare with Linux.

This is what runs on SlateOS. It drives `sh_spec.py` (the Python 3 port of
upstream's harness) over each `spec/*.test.sh` in the installed tree, in this
process -- one Python start-up for the whole run rather than one per file,
which matters under emulation -- and compares every cell with
`expected/<name>.tsv`: the same run, recorded on Linux against the same Oils
release. A case that behaves differently here is a SlateOS bug -- in the C
library, the kernel, or a program a case runs -- or a platform difference to
explain.

THE EXPECTATIONS ARE RECORDED BY THIS SCRIPT, `--record`, ON LINUX, and not
taken from upstream's own CI or from `validate.sh`'s runs. Those ran with a
Linux `PATH` (a Python 2 on it, which some cases call), the source tree as
`$REPO_ROOT`, other temporary paths: on the same machine they disagree with
this script in 85 cells, every one of them the environment. Recording here
means the only thing that differs between the recorded run and the run on
SlateOS is the system. `--record` runs everything three times and writes the
cells the Linux runs did not all agree on -- timing-sensitive cases -- to
`expected/unstable.tsv`; those are reported apart and never counted as
differences.

The working tree is `/tmp/oils-spec` in both, deliberately the same path (see
`--out`): cases run in directories under it, and some of them see that.

usage:
    python3 /usr/share/oils-spec/run_all.py [--oils-for-unix PATH] [--out DIR]
                                            [--timeout SECS] [NAME...]
    python3 run_all.py --record --oils-for-unix NATIVE_BUILD   # on Linux

Prints one line per spec file, then `OILS_SPEC_SUMMARY ...`, and exits 0 when
no stable cell differs from Linux.
"""

from __future__ import annotations

import argparse
import contextlib
import io
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import sh_spec  # noqa: E402 -- beside this file, by design

SPEC_DIR = HERE / "spec"
EXPECTED_DIR = HERE / "expected"
UNSTABLE = EXPECTED_DIR / "unstable.tsv"


def read_tsv(path: Path) -> dict[tuple[str, str], str]:
    """`(case, shell) -> result` from a results table."""
    cells = {}
    for line in path.read_text(encoding="utf-8").splitlines()[1:]:
        if line:
            case, shell, result = line.split("\t")
            cells[(case, shell)] = result
    return cells


def link_shells(oils_for_unix: Path, bin_dir: Path) -> None:
    """`osh` and `ysh`, both the one program, which chooses its language
    from the name it is run by."""
    bin_dir.mkdir(parents=True, exist_ok=True)
    for name in ("osh", "ysh"):
        link = bin_dir / name
        if link.is_symlink() or link.exists():
            link.unlink()
        link.symlink_to(oils_for_unix)


def run_suite(names: list[str], out: Path, bin_dir: Path, timeout: str,
              progress: bool) -> dict[str, Path | None]:
    """Run each spec file; `name -> its results table`, or None when the
    harness wrote none (its log says why)."""
    tables = out / "tables"
    logs = out / "logs"
    for d in (tables, logs):
        d.mkdir(parents=True, exist_ok=True)
    results: dict[str, Path | None] = {}
    for name in names:
        table = tables / f"{name}.tsv"
        if table.exists():
            table.unlink()
        argv_one = [
            "sh_spec.py",
            "--tmp-env", str(out / "tmp"),
            "--path-env", f"{SPEC_DIR / 'bin'}:/bin",
            "--env-pair", "LC_ALL=C.UTF-8",
            "--env-pair", f"REPO_ROOT={HERE}",
            "--timeout", timeout,
            "--oils-bin-dir", str(bin_dir),
            "--tsv-output", str(table),
            str(SPEC_DIR / f"{name}.test.sh"),
        ]
        log = io.StringIO()
        try:
            with contextlib.redirect_stdout(log), contextlib.redirect_stderr(log):
                sh_spec.main(argv_one)
        except Exception as err:  # noqa: BLE001 -- one bad file must not end the run
            log.write(f"\nharness error: {type(err).__name__}: {err}\n")
        (logs / f"{name}.log").write_text(log.getvalue(), encoding="utf-8", newline="\n")
        results[name] = table if table.exists() else None
        if progress:
            print(f"  ran {name}", file=sys.stderr, flush=True)
    return results


def record(names: list[str], args: argparse.Namespace, bin_dir: Path) -> int:
    """Run everything `--runs` times; write expected/ from the first run, and
    every cell that was not the same in all of them to expected/unstable.tsv.

    Three by default, because two missed one: `background` case 7 races
    three jobs on 10-30 ms sleeps, agreed in two runs and flipped in a third
    on a loaded machine."""
    runs = [run_suite(names, args.out / f"record-{n}", bin_dir, args.timeout, True)
            for n in range(1, args.runs + 1)]
    if EXPECTED_DIR.exists():
        shutil.rmtree(EXPECTED_DIR)
    EXPECTED_DIR.mkdir(parents=True)
    unstable = []
    missing = []
    for name in names:
        tables = [run[name] for run in runs]
        if any(t is None for t in tables):
            missing.append(name)
            continue
        shutil.copyfile(tables[0], EXPECTED_DIR / f"{name}.tsv")
        cells = [read_tsv(t) for t in tables]
        for key in sorted(set().union(*cells)):
            seen = [c.get(key, "-") for c in cells]
            if len(set(seen)) > 1:
                unstable.append((name, key[0], key[1], ",".join(seen)))
    with open(UNSTABLE, "w", encoding="utf-8", newline="\n") as f:
        f.write("file\tcase\tshell\tresults\n")
        for row in unstable:
            f.write("\t".join(row) + "\n")
    print(f"OILS_SPEC_RECORDED tables={len(names) - len(missing)} "
          f"unstable_cells={len(unstable)} missing={len(missing)}")
    for name in missing:
        print(f"  no results for {name}; see its logs under {args.out}")
    return 1 if missing else 0


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(prog="run_all.py")
    ap.add_argument("--oils-for-unix", type=Path, default=Path("/bin/oils-for-unix"))
    # Fixed, not $TMPDIR: each case runs in a directory under it, and some
    # cases see that path -- one checks whether it is under $HOME, others
    # print $TMP -- so the recorded run and the run here must use the same.
    # The recording first ran under ~/.cache and ysh-prompt case 2 passed
    # there and failed in /tmp.
    ap.add_argument("--out", type=Path, default=Path("/tmp/oils-spec"))
    ap.add_argument("--timeout", default="20")
    ap.add_argument("--record", action="store_true",
                    help="on Linux: run everything --runs times and write expected/ from it")
    ap.add_argument("--runs", type=int, default=3,
                    help="how many runs --record compares to find unstable cells (default 3)")
    ap.add_argument("names", nargs="*", help="spec files to run (default: all)")
    args = ap.parse_args(argv[1:])

    bin_dir = args.out / "bin"
    link_shells(args.oils_for_unix, bin_dir)
    names = args.names or sorted(p.name[:-len(".test.sh")] for p in SPEC_DIR.glob("*.test.sh"))

    if args.record:
        return record(names, args, bin_dir)

    unstable = set()
    if UNSTABLE.exists():
        for line in UNSTABLE.read_text(encoding="utf-8").splitlines()[1:]:
            if line:
                name, case, shell = line.split("\t")[:3]
                unstable.add((name, case, shell))

    results = run_suite(names, args.out, bin_dir, args.timeout, False)
    differ_total = failed_total = cases_total = unstable_seen = 0
    files_with_differences = 0
    for name in names:
        table = results[name]
        if table is None:
            print(f"{name}: no results; see {args.out / 'logs' / (name + '.log')}")
            files_with_differences += 1
            continue
        got = read_tsv(table)
        cases_total += len(got)
        failed_total += sum(1 for r in got.values() if r in ("FAIL", "TIME"))
        expected_file = EXPECTED_DIR / f"{name}.tsv"
        if not expected_file.exists():
            print(f"{name}: {len(got)} cases (no Linux results to compare)")
            continue
        want = read_tsv(expected_file)
        keys = set(want) | set(got)
        shaky = {k for k in keys if (name, k[0], k[1]) in unstable}
        unstable_seen += len(shaky)
        differ = sorted(k for k in keys - shaky if want.get(k) != got.get(k))
        differ_total += len(differ)
        if differ:
            files_with_differences += 1
            cells = ", ".join(
                f"case {c} {s}: linux {want.get((c, s), '-')} here {got.get((c, s), '-')}"
                for c, s in differ[:6])
            more = f" (+{len(differ) - 6} more)" if len(differ) > 6 else ""
            print(f"{name}: {len(differ)} differ from Linux -- {cells}{more}")
        else:
            print(f"{name}: {len(got)} cases, same as Linux")
        sys.stdout.flush()

    print(f"OILS_SPEC_SUMMARY files={len(names)} cases={cases_total} "
          f"failed={failed_total} differ_from_linux={differ_total} "
          f"files_with_differences={files_with_differences} "
          f"unstable_skipped={unstable_seen}")
    return 0 if differ_total == 0 and files_with_differences == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
