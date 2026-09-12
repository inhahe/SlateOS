#!/usr/bin/env bash
# Differential test: our `free` against procps-ng `free`.
#
# ## Why this one needed a technique the other harnesses did not
#
# `free` reports the machine. Run it twice and the numbers differ, because the
# machine moved between the two runs -- so a naive harness comparing our output
# against the reference's reports a difference that is a property of the memory
# subsystem rather than of either program. That is the same hazard `date-diff.sh`
# names: "a gate that fails once an hour for a reason nobody can reproduce is a
# gate that gets switched off."
#
# `date` solved it by making every case a function of its ARGUMENTS (`-d @0`),
# and said plainly that what it therefore does not cover is reading the clock,
# which is one line. `free` has no `-d`: its input IS the machine, and the part
# worth comparing -- every unit conversion, every rounding decision, the
# buff/cache and available arithmetic -- is exactly the part that depends on it.
# Argument-only coverage would leave the whole subject untested.
#
# So the input is PINNED instead. Each case runs inside a private mount
# namespace with a fixture bind-mounted over `/proc/meminfo`, so both sides read
# byte-identical memory and every difference in the output is a difference in
# the program. `df-diff.sh` established the technique here (`unshare -mUr`,
# because plain `-m` wants privileges this host does not give); this uses it
# per-case rather than re-execing the whole script, which is why it needs no
# entry in `check-diff-preamble-order.py`'s BASELINE.
#
# ## What happens when the namespace cannot be had
#
# Every pinned case is MASKED and counted, and the summary says so. It does not
# fall back to comparing live memory: that is the failure mode this harness
# exists to avoid, and "ran against a moving target and got lucky" and "ran
# against a fixed one" must not print the same thing. `df-diff.sh` reaches the
# same conclusion through `DF_DIFF_NS=none`.
#
# ## The reference is procps-ng, not coreutils
#
# `free` is not a coreutils program, so `DIFF_GNU_SOURCE` -- which fetches and
# builds a coreutils release -- does not apply and is deliberately unset. The
# installed `/usr/bin/free` is procps-ng 4.0.4, which is the exact version
# `coreutils/src/bin/free.rs` says it transcribes, so the reference and the
# subject's stated source agree by name and by version.
#
# ## The fixture's numbers are chosen, not typed at random
#
# `MemTotal` is 16000000 kB rather than a round power of two so that `-h`,
# `--si` and the `--kilo`/`--kibi` pairs cannot agree by accident: 1000-based
# and 1024-based renderings of a round number often coincide, and a case that
# passes because both sides divided a convenient number is not evidence.
set -u

DIFF_PROG='free'
DIFF_NEED=timeout
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; masked=0; broken=0

# ---------------------------------------------------------------------------
# The pinned machine
# ---------------------------------------------------------------------------
meminfo=$DIFF_TMP/meminfo
cat > "$meminfo" <<'MEMINFO'
MemTotal:       16000000 kB
MemFree:         2100000 kB
MemAvailable:    8300000 kB
Buffers:          510000 kB
Cached:          4100000 kB
SwapCached:        12000 kB
Active:          6000000 kB
Inactive:        3000000 kB
SwapTotal:       4000000 kB
SwapFree:        3100000 kB
Dirty:              3000 kB
Writeback:             0 kB
Shmem:            130000 kB
SReclaimable:     260000 kB
Committed_AS:    5200000 kB
MEMINFO

# Can we pin it?  Probed once, and the answer decides whether the cases below
# are evidence or are masked.
ns=none
if command -v unshare >/dev/null 2>&1 \
   && unshare -mUr sh -c "mount --bind '$meminfo' /proc/meminfo 2>/dev/null \
        && grep -q 'MemTotal:       16000000' /proc/meminfo" 2>/dev/null; then
  ns=yes
fi

run_side() {
  local side=$1; shift
  # `env -i`-style pinning of the locale only: PATH must keep the side's bindir
  # first, which is how the preamble selects which binary is under test.
  # `shift 2` then `"$@"`, NOT `"${@:2}"`. That slice is a BASH expansion and this
  # body runs under `sh` -- dash on this host -- where it does not mean what
  # it says, and `free` ended up invoked with NO ARGUMENTS. Every case then
  # ran the same bare `free` against the same pinned file, and the harness
  # reported 47 passed / 0 differed: forty-seven copies of one comparison,
  # not one of them the case it was named after.
  #
  # IT WAS CAUGHT BY THE TWO `xfail_case` PROBES AT THE BOTTOM, and would
  # not have been caught by anything else. `--help` and `--version` are
  # declared expected-to-DIFFER, so the harness printing XPASS for them is
  # what said the comparison was not happening -- our `free --version` says
  # "free from SlateOS coreutils 0.1.0" against procps-ng's "free from
  # procps-ng 4.0.4", and those cannot agree. A harness with no case
  # that is REQUIRED to fail cannot tell a clean sweep from an empty one.
  diff_run timeout -k 2 20 unshare -mUr sh -c \
    'mount --bind "$1" /proc/meminfo || exit 125
     # PREPENDED, not replaced. `PATH=$2` on its own removed every
     # directory holding `env`, so the exec died with `env: not found`
     # and rc=127 -- on BOTH sides, identically, which the comparison
     # below then read as agreement. Setting the variables here instead
     # of through `env` removes the dependency altogether.
     PATH=$2:$PATH; export PATH
     LC_ALL=C.UTF-8; export LC_ALL
     TZ=UTC; export TZ
     shift 2
     exec free "$@"' \
    _ "$meminfo" "$bindir/$side" "$@"
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc
  o_out=$(run_side ours "$@" 2>/dev/null); o_rc=$?
  o_err=$(run_side ours "$@" 2>&1 >/dev/null)
  g_out=$(run_side gnu  "$@" 2>/dev/null); g_rc=$?
  g_err=$(run_side gnu  "$@" 2>&1 >/dev/null)
  # NEITHER SIDE RUNNING IS NOT AGREEMENT, and this harness learned that
  # the expensive way: a PATH bug made both invocations die with
  # `env: not found` and rc=127, byte-identically, and 47 cases reported
  # a clean sweep. Two witnesses that fail for the same environmental
  # reason are one witness, and it is not testifying about the programs.
  #
  # 127 is `command not found` and 125 is the bind-mount failing above.
  # Both mean the case never reached `free`, so both are a harness fault
  # rather than a finding -- reported as BROKEN and counted apart, where
  # a reader cannot mistake them for either a pass or a difference.
  if [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ] \
     || [ "$o_rc" = 125 ] || [ "$g_rc" = 125 ]; then
    AGREED=broken
  elif [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr '\n' '|')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr '\n' '|')" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

report() {
  local label="$1"
  if [ "$AGREED" = broken ]; then
    broken=$((broken+1))
    printf 'BROKEN %s -- the case never reached `free` on one or both sides\n%s\n' "$label" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case() {
  if [ "$ns" != yes ]; then
    masked=$((masked+1))
    return 0
  fi
  compare "$@"
  report "free $*"
}

xfail_case() {
  local why=$1; shift
  if [ "$ns" != yes ]; then
    masked=$((masked+1))
    return 0
  fi
  compare "$@"
  if [ "$AGREED" = broken ]; then
    broken=$((broken+1))
    printf 'BROKEN free %s -- never reached `free`\n' "$*"
  elif [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS free %s -- expected to differ (%s) and did not\n' "$*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail free %s (%s)\n' "$*" "$why"
  fi
  return 0
}

# --- the default report --------------------------------------------------------
run_case

# --- every unit, one at a time --------------------------------------------------
# The 1000-based and 1024-based families are both walked in full: they are the
# pair most likely to be implemented once and aliased, which reads as correct
# for a round number and wrong for this fixture's.
for u in -b -k -m -g --bytes --kilo --mega --giga --tera --peta \
         --kibi --mebi --gibi --tebi --pebi; do
  run_case "$u"
done

# --- human-readable, which is the rounding-sensitive one ---------------------------
run_case -h
run_case --human
run_case --si
run_case -h --si
run_case --human --si

# --- the extra rows and columns -----------------------------------------------------
run_case -l
run_case --lohi
run_case -t
run_case --total
run_case -w
run_case --wide
run_case -v
run_case --committed
run_case -t -l
run_case -w -h
run_case --total --lohi --wide

# --- combinations that pick a last-wins unit -------------------------------------------
run_case -b -k
run_case -k -b
run_case -h -b
run_case -m --giga

# --- refusals, which is where a stub and a real implementation part company -------------
run_case -Z
run_case --nosuchoption
run_case -s
run_case -c
run_case --seconds
run_case --count
run_case -s abc
run_case -c abc
run_case -s -1
run_case --seconds=abc
run_case extra-operand

# --- the two whose text is ours ----------------------------------------------------------
# `--help` is NOT a declared divergence, and the harness is what told me. I
# wrote both of these as xfails by symmetry with every other harness in the
# tree, where a utility's help text is its own. This one is a transcription of
# procps-ng 4.0.4 and its help output is BYTE-IDENTICAL to the reference's, so
# the declaration was simply false -- and the XPASS was the harness refusing a
# declaration that had never been true rather than one that had stopped being.
#
# Two options declared together, one of them wrong, is the shape that survives
# because nobody re-reads a pair they wrote in one line.
run_case --help

# `--version` really does differ: ours says "free from SlateOS coreutils 0.1.0"
# against "free from procps-ng 4.0.4". Measured, not assumed, after the one
# above turned out not to be.
xfail_case "our version string, not procps-ng's" --version

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
if [ "$masked" -gt 0 ]; then
  printf ', %d MASKED' "$masked"
fi
if [ "$broken" -gt 0 ]; then
  printf ', %d BROKEN (never reached the subject)' "$broken"
fi
printf '\n'
if [ "$masked" -gt 0 ]; then
  cat <<'MASKEDNOTE'

Every case was masked: this host could not give the harness a private mount
namespace, so `/proc/meminfo` could not be pinned and both sides would have
been reading a machine that moves between them.  That is not a comparison, so
nothing was compared and nothing is claimed.  `unshare -mUr` is what is
missing; `df-diff.sh` documents the same requirement.
MASKEDNOTE
fi
[ -n "${DIFF_SKIPPED:-}" ] && printf 'skipped:%s\n' "$DIFF_SKIPPED"
[ "$fail" = 0 ] && [ "$xpass" = 0 ] && [ "$masked" = 0 ] && [ "$broken" = 0 ]
