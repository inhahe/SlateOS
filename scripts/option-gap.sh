#!/usr/bin/env bash
# Options the reference implementation has that OURS REJECTS AS UNKNOWN.
#
# ## The axis nothing else covers
#
# The tree has ~70 per-program differential harnesses and they are thorough
# about the invocations someone thought to write down. None of them asks the
# complementary question: *of every option the real program documents, which
# ones does ours refuse outright?*
#
# That is a different failure from a wrong answer and a louder one. `diff -C 1`
# did not print something GNU would not print -- it exited 2 with
# `invalid option -- 'C'`, so a script using it stopped. Found by hand on
# 2026-09-16; this file is the generalisation, and it found 74 more the same
# day across six other programs.
#
# ## Method, and why each step is the way it is
#
#   1. take the short options the REFERENCE's own `--help` lists, so the list
#      comes from the program rather than from a table here that would go
#      stale silently
#   2. run the reference with just that option; if IT calls the option unknown,
#      drop the row -- some help text mentions options the build does not have
#   3. run OURS with the same option, and report only `invalid option` and its
#      spellings
#
# Step 2 is what keeps the noise down. An option that needs an argument fails
# on both sides for the same reason and is never reported, because the filter
# is the unknown-option WORDING and not the exit status.
#
# ## Three ways this probe was wrong before it was right
#
# Recorded because each produced a confident empty result, and an empty result
# from a broken probe is indistinguishable from a clean tree:
#
#   * It was pointed at `target/x86_64-slateos/release`, whose binaries
#     SEGFAULT under Linux -- they are built for another OS. Every comparison
#     was between a crash and the reference, the filter dropped them all, and
#     it printed nothing. Hence `--version` being run first as proof the
#     subject executes at all, and the counts printed at the end.
#   * The script was piped into `sh -s`, so stdin WAS the script, and the first
#     program whose `--help` reads standard input ate the rest of it. The sweep
#     stopped after 17 of 72 programs and reported as though it had finished.
#     Hence `</dev/null` on every invocation including the help one.
#   * Running a program with a VALID option makes it work rather than error:
#     `yes -x` prints for ever and `sleep` sleeps. Hence `timeout` on each.
#
# ## Usage
#
#     bash scripts/option-gap.sh              # compare against the baseline
#     bash scripts/option-gap.sh --update     # rewrite the baseline
#     bash scripts/option-gap.sh --self-test  # prove it can still fail
#
# A baseline rather than a hard zero, for the reason `argv-utf8` and
# `raced-globals` are baselined: 75 standing gaps make every run red, and a
# gate that is always red is one nobody reads. What is watched is CHANGE --
# a new gap, or a baselined one that has been fixed and not removed.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASELINE="$ROOT/scripts/option-gap-baseline.txt"
MANIFEST="$ROOT/scripts/rootfs-bin-manifest.txt"
OURS_DIR="$ROOT/target/x86_64-pc-windows-gnu/debug"
OURS_EXT=".exe"
# On a Linux host our binaries are the linux-gnu build and have no extension.
if [ "$(uname -s 2>/dev/null)" = "Linux" ]; then
  OURS_DIR="$ROOT/target/x86_64-unknown-linux-gnu/debug"
  OURS_EXT=""
fi

# How to reach the reference implementation. On Windows it lives in WSL.
if command -v wsl.exe >/dev/null 2>&1 && [ "$(uname -s 2>/dev/null)" != "Linux" ]; then
  REF_VIA_WSL=1
else
  REF_VIA_WSL=0
fi

unknown_option_wording() {
  case "$1" in
    *"invalid option"*|*"unrecognized option"*|*"unknown option"*|*"illegal option"*) return 0 ;;
  esac
  return 1
}

# The reference side, batched into ONE crossing of the WSL boundary: about 470
# option probes is 470 process starts, and starting them one at a time from
# Windows costs minutes rather than seconds.
gather_reference() {
  local names="$1"
  if [ "$REF_VIA_WSL" = 1 ]; then
    # `wslpath` wants a WINDOWS path. `$ROOT` here is MSYS-style (`/e/...`)
    # because this script is run by MSYS bash, and handing that to `wslpath`
    # yields nothing -- which presented as "the reference produced no output",
    # i.e. as a finding about WSL rather than about the path. `cygpath -w`
    # converts it; without cygpath, fall back to letting WSL resolve `/mnt/e`
    # itself.
    local winroot
    if command -v cygpath >/dev/null 2>&1; then
      winroot=$(cygpath -w "$ROOT")
    else
      winroot="$ROOT"
    fi
    wsl.exe -e sh -c "cd \"\$(wslpath '$winroot')\" && sh scripts/option-gap-ref.sh $names" 2>&1
  else
    sh "$ROOT/scripts/option-gap-ref.sh" $names 2>&1
  fi
}

self_test() {
    # The probe must be able to FAIL. A real gap is `diff -D`; assert it is
    # detected, and that an option we DO have is not.
    local d="$OURS_DIR/diff$OURS_EXT"
    if [ ! -x "$d" ]; then
      echo "option-gap self-test: no diff binary to grade with" >&2
      exit 2
    fi
    local bad good
    bad=$(timeout 5 "$d" -D </dev/null 2>&1 >/dev/null | head -2)
    good=$(timeout 5 "$d" -u </dev/null 2>&1 >/dev/null | head -2)
    local rc=0
    if unknown_option_wording "$bad"; then
      echo "ok   a missing option is detected (diff -D)"
    else
      echo "FAIL diff -D was not reported as unknown; the probe cannot see a gap"
      rc=1
    fi
    if unknown_option_wording "$good"; then
      echo "FAIL diff -u was reported as unknown; the probe sees gaps that are not there"
      rc=1
    else
      echo "ok   ...and an option we DO have is not reported"
    fi
    echo "option-gap self-test: $([ $rc = 0 ] && echo passed || echo FAILED)"
    return $rc
}

main() {
  local update=0 selftest=0
  for a in "$@"; do
    case "$a" in
      --update) update=1 ;;
      --self-test|--selftest) selftest=1 ;;
    esac
  done

  if [ ! -f "$MANIFEST" ]; then
    echo "option-gap: no $MANIFEST here" >&2
    exit 2
  fi
  local names
  names=$(grep -vE '^[[:space:]]*(#|$)' "$MANIFEST" | grep -v '=' | tr '\n' ' ')

  # The self-test grades the PROBE and needs no reference sweep, so it runs
  # before one is attempted: a probe whose own two arms cannot be checked
  # without a working WSL is a probe that goes unchecked on most hosts.
  if [ "$selftest" = 1 ]; then
    self_test
    exit $?
  fi

  local ref
  ref=$(gather_reference "$names")
  local ref_progs
  ref_progs=$(printf '%s\n' "$ref" | awk 'NF {print $1}' | sort -u | wc -l)
  if [ "$ref_progs" -lt 2 ]; then
    echo "option-gap: CANNOT GRADE -- the reference side produced nothing." >&2
    echo "    Is WSL present, and does it have coreutils on PATH?" >&2
    exit 2
  fi

  local examined=0 unrunnable=0 found=0
  local gaps=""
  while read -r name opt verdict; do
    [ "${verdict:-}" = ok ] || continue
    local bin="$OURS_DIR/$name$OURS_EXT"
    [ -x "$bin" ] || continue
    # Proof the subject runs here before anything it says is believed.
    timeout 5 "$bin" --version >/dev/null 2>&1
    if [ $? -ge 126 ]; then
      unrunnable=$((unrunnable + 1))
      continue
    fi
    examined=$((examined + 1))
    local err
    err=$(timeout 5 "$bin" "$opt" </dev/null 2>&1 >/dev/null | head -2)
    if unknown_option_wording "$err"; then
      found=$((found + 1))
      gaps="$gaps$name $opt"$'\n'
    fi
  done <<EOF
$ref
EOF

  if [ "$examined" -eq 0 ]; then
    echo "option-gap: CANNOT GRADE -- no subject could be executed." >&2
    echo "    $unrunnable refused to run; is the Windows build present?" >&2
    exit 2
  fi

  gaps=$(printf '%s' "$gaps" | grep -v '^$' | sort)

  if [ "$update" = 1 ]; then
    printf '%s\n' "$gaps" > "$BASELINE"
    echo "option-gap: baseline rewritten -- $found gap(s) over $examined option(s)"
    exit 0
  fi

  if [ ! -f "$BASELINE" ]; then
    echo "option-gap: no baseline; run with --update to create one" >&2
    exit 2
  fi

  local new fixed
  new=$(comm -23 <(printf '%s\n' "$gaps") <(sort "$BASELINE"))
  fixed=$(comm -13 <(printf '%s\n' "$gaps") <(sort "$BASELINE"))

  echo "option-gap: examined $examined option(s) over $ref_progs program(s); $found gap(s)"
  local rc=0
  if [ -n "$new" ]; then
    echo "NEW option(s) we refuse that the reference accepts:"
    printf '%s\n' "$new" | sed 's/^/    /'
    rc=1
  fi
  if [ -n "$fixed" ]; then
    echo "FIXED -- these no longer reproduce; drop them from the baseline:"
    printf '%s\n' "$fixed" | sed 's/^/    /'
    rc=1
  fi
  [ "$rc" = 0 ] && echo "option-gap: no change against the baseline"
  exit $rc
}

main "$@"
