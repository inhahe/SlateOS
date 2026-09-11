#!/usr/bin/env bash
# Does a `DIFF_*` knob set on the command line reach the far side of the WSL
# re-exec?
#
# ## Why this test exists
#
# `scripts/diff-wsl.sh` re-execs every harness inside WSL. Environment
# variables do not cross that boundary on their own -- `wsl.exe` passes only
# what `WSLENV` or an explicit `env VAR=...` prefix carries -- so the re-exec
# rebuilds the environment by hand. It used to rebuild it from a written-out
# list of four names, and `DIFF_PKG` was not one of them.
#
# `DIFF_PKG` is the knob that chooses WHICH HALF of a duplicate pair is the
# subject. Dropping it did not fail; it fell back to `coreutils` and the run
# came back green:
#
#     DIFF_PKG=no-such-package-at-all ./scripts/expand-diff.sh
#     216 passed, 0 differed, 2 differ on purpose
#
# 216 passing cases for a package that does not exist. Nothing in that output
# is a warning, and the number is exactly the number a correct run prints.
#
# What it cost is specific. The whole point of `DIFF_PKG` is to run a harness
# against each half of a duplicate pair and see which one GNU agrees with. With
# the knob dropped, both runs measure the same binary, so every pair scores a
# perfect tie -- and a tie reads as "the halves are equivalent, delete either",
# which is the one conclusion that can delete the better half. `dd`'s two
# halves disagree on 331 of 339 cases and would have tied here.
#
# ## The two probes
#
# A gate needs proof that it RUNS and proof that it can REFUSE; a test of a
# forwarded variable needs the same, because "the value crossed" and "the
# harness ignored the value and did its default thing" produce the same green
# output. So the value is set to something that MUST produce a refusal:
#
#   * a package that does not exist -- cargo cannot resolve the ID;
#   * a package that exists but has no binary of this name -- cargo resolves
#     the package and then finds no such `--bin`.
#
# Both must fail. Against the old code both passed, and passed with a case
# count. The run-side probes then show the refusals are discriminating rather
# than constant: with no override, and with the default named explicitly, the
# harness must reach its cases.
#
# ## Why `logname`
#
# The vehicle has to be a harness, and this one is the cheapest: `logname`
# takes no operands, so its case list is short and a run is seconds. Nothing
# here depends on `logname` being CORRECT -- the run-side probes assert only
# that the harness reached its cases and printed a count, not that the count
# was clean. A harness that started failing its own comparison would not drag
# this test red with it.
set -u

here=$(cd "$(dirname "$0")" && pwd)
harness="$here/logname-diff.sh"

if [ ! -f "$harness" ]; then
  echo "test-diff-forward: $harness is missing" >&2
  exit 1
fi

pass=0
fail=0

# Runs the harness with the given environment, and reports whether it reached
# its cases. Output is captured rather than shown: a refusal prints a cargo
# error that would otherwise read as this test failing.
run_harness() {
  out=$(env "$@" bash "$harness" 2>&1)
  rc=$?
}

# `N passed` is the harness's own summary line, printed only once the build
# succeeded and the cases ran.
reached_cases() {
  printf '%s\n' "$out" | grep -Eq '[0-9]+ passed'
}

check_refuses() {
  label=$1
  shift
  run_harness "$@"
  if [ "$rc" = 0 ]; then
    echo "FAIL: $label -- exited 0, expected a refusal"
    fail=$((fail + 1))
    return
  fi
  if reached_cases; then
    echo "FAIL: $label -- ran its cases anyway:"
    printf '%s\n' "$out" | grep -E '[0-9]+ passed' | sed 's/^/       /'
    fail=$((fail + 1))
    return
  fi
  echo "ok: $label -- refused"
  pass=$((pass + 1))
}

check_runs() {
  label=$1
  shift
  run_harness "$@"
  if ! reached_cases; then
    echo "FAIL: $label -- never reached its cases (rc=$rc):"
    printf '%s\n' "$out" | tail -5 | sed 's/^/       /'
    fail=$((fail + 1))
    return
  fi
  echo "ok: $label -- reached its cases"
  pass=$((pass + 1))
}

# -- the two refusals, which is what a dropped variable cannot produce --
check_refuses "DIFF_PKG names no package" \
  DIFF_PKG=no-such-package-at-all-9d3f

# `quoting` is a library crate under userspace/ with no binaries at all, so
# cargo resolves the package and then fails to find `--bin logname`. That is a
# different refusal from the one above, and it proves the value was used for
# the build rather than merely validated.
check_refuses "DIFF_PKG names a package with no such bin" \
  DIFF_PKG=quoting

# -- and the run side, so the refusals above are known to discriminate --
check_runs "no override"
check_runs "DIFF_PKG set to the default" DIFF_PKG=coreutils

echo
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
