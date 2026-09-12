#!/usr/bin/env bash
# Does any test binary hang when its stdin is an OPEN PIPE that never delivers?
#
# WHY THIS EXISTS. `ed`'s tests built an `Editor`, and `Editor::new` did
# `stdin().lock()`. A test that executed `0a` -- a command that reads its text
# from input -- therefore read the TEST PROCESS's standard input. Under the
# harness that is /dev/null, which returns EOF at once, so the suite was green.
# Attach a pipe with nothing on it, which is what a git hook or a CI runner
# hands you, and the read never returns while the lock is never released, so
# every other test in the binary blocks behind it. Measured on the built
# binary: /dev/null ran 73 tests in 0.01s, an open pipe hung until killed.
# It cost two fifteen-minute stalls of `cargo test -p coreutils` in one day
# and was recorded as machine contention both times, because "has been running
# for over 60 seconds" is what starvation looks like as well.
#
# A HANG IS A DIFFERENCE BETWEEN TWO CONDITIONS, NOT A TIMEOUT IN ONE. Each
# binary is run twice -- once with stdin at /dev/null, once with a pipe held
# open -- and only a binary that finishes the first and not the second counts.
# The first attempt at this sweep used a single 6s timeout and called
# `apps/automator` a hang; it legitimately runs 160 tests in 12.8s. Slow is
# not stuck, and a sweep that cannot tell them apart reports the difference
# between its own timeout and the machine's load.
#
# THREE COST AND CORRECTNESS ERRORS MADE WHILE BUILDING IT, ALL MEASURED:
#   * `deps/` holds 1677 binaries. Sweeping all of them at up to 45s each,
#     twice, is forty hours. Scope it to the names you actually care about.
#   * `( sleep 40 | timeout 35 "$f" )` makes the subshell wait for `sleep`
#     too, so every binary cost a flat 40s even when it exited instantly.
#     `< <(sleep 40)` holds the pipe open without being waited on: measured
#     0s for a non-reader and a refusal at the deadline for a reader.
#   * `deps/` holds BOTH the test harness and the plain binary under the same
#     `name-hash.exe` pattern, so "newest wins" picked the wrong one for about
#     half the names -- including `ed`, the binary this sweep exists for,
#     which it then reported as having no tests. Walk candidates newest-first
#     and take the first that actually lists some. That one fix took the
#     coverage from 48 names to 84.
#
# Usage:
#   scripts/stdin-hang-sweep.sh --selftest      prove it can both pass and refuse
#   scripts/stdin-hang-sweep.sh [name ...]      default: coreutils' bins + posix
set -u
cd "$(dirname "$0")/.." || exit 2

DEPS=${DEPS:-target/x86_64-pc-windows-gnu/debug/deps}
OUT=${OUT:-build/stdin-hang.txt}
BASE_CAP=${BASE_CAP:-60}   # longest baseline run we will wait out
PROBE_CAP=${PROBE_CAP:-30} # a binary still alive at this point is stuck
HOLD=${HOLD:-40}           # must exceed PROBE_CAP, or the pipe shuts first

sweep() {
  local hung=0 slow=0 tested=0 skipped=0 name cand c f n start base rc
  mkdir -p "$(dirname "$OUT")"
  : > "$OUT"
  for name in "$@"; do
    f=""; n=0
    for cand in $(ls -t "$DEPS"/"$name"-*.exe 2>/dev/null); do
      c=$("$cand" --list 2>/dev/null </dev/null | grep -c ": test")
      case "$c" in ''|0) continue;; esac
      f=$cand; n=$c; break
    done
    [ -n "$f" ] || { skipped=$((skipped+1)); continue; }
    tested=$((tested+1))
    start=$(date +%s)
    timeout "$BASE_CAP" "$f" >/dev/null 2>&1 </dev/null
    rc=$?
    base=$(( $(date +%s) - start ))
    # Anything whose ordinary run is already near the probe deadline cannot be
    # judged: a refusal there would mean "slow", not "stuck".
    if [ "$rc" = 124 ] || [ "$base" -ge $(( PROBE_CAP - 5 )) ]; then
      slow=$((slow+1)); echo "too slow to judge (${base}s baseline): $name" >> "$OUT"; continue
    fi
    timeout "$PROBE_CAP" "$f" >/dev/null 2>&1 < <(sleep "$HOLD")
    if [ "$?" = 124 ]; then
      hung=$((hung+1))
      echo "STDIN HANG ($n tests, ${base}s baseline): $name  $f" >> "$OUT"
    fi
  done
  echo "binaries: $tested   stdin-hangs: $hung   too slow to judge: $slow   no tests: $skipped"
  cat "$OUT"
  [ "$hung" = 0 ]
}

# A detector nothing has ever watched REFUSE is not a detector. The self-test
# builds two binaries that look like libtest harnesses -- one that reads stdin
# and one that does not -- and requires exactly one of them to be caught.
selftest() {
  local d rc out
  command -v rustc >/dev/null || { echo "selftest needs rustc"; return 2; }
  d=$(mktemp -d) || return 2
  cat > "$d/hangfix.rs" <<'RS'
fn main() {
    if std::env::args().any(|a| a == "--list") {
        println!("reads_stdin: test");
        return;
    }
    let mut s = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut s);
}
RS
  cat > "$d/goodfix.rs" <<'RS'
fn main() {
    if std::env::args().any(|a| a == "--list") {
        println!("no_stdin: test");
    }
}
RS
  rustc -O "$d/hangfix.rs" -o "$d/hangfix-aaaaaaaa.exe" 2>/dev/null || { rm -rf "$d"; return 2; }
  rustc -O "$d/goodfix.rs" -o "$d/goodfix-bbbbbbbb.exe" 2>/dev/null || { rm -rf "$d"; return 2; }
  out=$(DEPS=$d OUT=$d/out.txt sweep hangfix goodfix nosuchname); rc=$?
  rm -rf "$d"
  echo "$out"
  case "$out" in
    *"binaries: 2   stdin-hangs: 1   too slow to judge: 0   no tests: 1"*) ;;
    *) echo "SELFTEST FAILED: expected 2 binaries, 1 hang, 1 without tests"; return 1;;
  esac
  case "$out" in *"STDIN HANG"*"hangfix"*) ;; *) echo "SELFTEST FAILED: the reader was not named"; return 1;; esac
  [ "$rc" = 1 ] || { echo "SELFTEST FAILED: a sweep that found a hang must exit non-zero"; return 1; }
  echo "SELFTEST OK: caught the reader, passed the non-reader, skipped the absent name"
}

if [ "${1:-}" = "--selftest" ]; then
  selftest
  exit $?
fi

if [ "$#" -gt 0 ]; then
  sweep "$@"
else
  sweep $(ls userspace/coreutils/src/bin/*.rs | sed 's|.*/||; s|\.rs$||') posix
fi
