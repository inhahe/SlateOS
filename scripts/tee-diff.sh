#!/usr/bin/env bash
# Differential test: our tee against GNU coreutils' tee.
#
# ## What this one has to compare that its siblings do not
#
# `cmp`, `du`, `find` and `ls` are all read-only: run each side, compare what
# came out. `tee`'s *point* is what it leaves behind, so agreeing on stdout,
# stderr and the exit status proves almost nothing on its own — a tee that
# silently wrote nothing would pass every such case. So each case here runs in
# a directory of its own per side, and the two directories are compared
# afterwards, file by file, bytes and all. That is the only thing that can see
# whether `--output-error=exit` really did abort before writing the later
# operands, or whether `tee dup dup` opened one file or two.
#
# It also means every case needs its fixtures built twice, once per side, which
# is what `SETUP` is for: a snippet of shell run inside the case directory
# before tee is. It cannot be hoisted out to a shared fixture directory the way
# `cmp-diff.sh` does, because tee writes to its fixtures.
#
# ## Why the subject is built for Linux, and why both sides run inside WSL
#
# The same reasons as `cmp-diff.sh`, whose header spells them out: our tee's
# body is `#[cfg(unix)]`, the fixtures include names that are not valid UTF-16
# and so cannot exist on Windows at all, and a Linux build sharing the
# repository's `target/` with the Windows one would make each invalidate the
# other. The build lands in `$HOME/.cache/slateos-diff-target` inside WSL,
# shared with the other four harnesses (`design-decisions.md` §374).
#
# ## Cases that differ on purpose
#
# Three kinds, each recorded as `xfail`:
#
#   * `--help` omits the GNU project's `Report bugs to:` block, and `--version`
#     names SlateOS. As everywhere here.
#   * A broken stdout under the *default* mode. GNU leaves SIGPIPE fatal and
#     dies of it, status 141; SlateOS has no signals, so our default mode is
#     upstream's own ignored-SIGPIPE path — drop the write, keep copying to
#     whatever outputs are left, exit with the status so far. That is also what
#     `cut`, `head`, `tail` and `uniq` in this tree already do. See the header
#     of `tee.rs`.
#   * `tee out <&-` — stdin closed. GNU diagnoses twice (`read error: Bad file
#     descriptor`, then gnulib's `close_stdin` atexit hook adding `standard
#     input: Bad file descriptor`) and exits 1. We say nothing and exit 0,
#     because by the time `main` runs there is no closed descriptor left to
#     notice: Rust's std reopens any missing standard descriptor onto
#     `/dev/null` before `main`, so our stdin is an empty file and the copy
#     genuinely succeeds. Measured, not assumed — `tee /proc/self/fd/0 <&-`
#     opens successfully for us and is `No such file or directory` for GNU,
#     which is the descriptor table answering the question directly.
#
# Note what is *not* on that list: how a file name is rendered into a
# diagnostic. `cmp-diff.sh` has a whole family of xfails for it, because
# diffutils interpolates names with a bare `%s` while we quote them (§373).
# coreutils' tee does not — all three of its `error` calls spell the name
# `quotef (files[i])`, which is the function our `quotef_os` is — so the
# quoting cases here are ordinary passes, and were xfails only until the first
# run reported them as XPASS.
#
# Run `OURS=/usr/bin/tee ./scripts/tee-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

export MSYS2_ARG_CONV_EXCL='*'

# --- get ourselves into WSL --------------------------------------------------
if ! command -v wslpath >/dev/null 2>&1; then
  if ! command -v wsl >/dev/null 2>&1; then
    echo "tee-diff: no WSL on this host; skipping (our tee is a unix-only binary)"
    exit 0
  fi
  here=$(cd "$(dirname "$0")" && pwd)
  if command -v cygpath >/dev/null 2>&1; then here=$(cygpath -m "$here"); fi
  inside=$(wsl wslpath -u "$here" 2>/dev/null) || {
    echo "tee-diff: could not map $here into WSL; skipping"
    exit 0
  }
  exec wsl -e env "OURS=${OURS:-}" "VERBOSE=${VERBOSE:-}" \
    bash "$inside/tee-diff.sh"
fi

root=$(cd "$(dirname "$0")/.." && pwd) || exit 1

# --- the reference -----------------------------------------------------------
if ! command -v tee >/dev/null 2>&1; then
  echo "tee-diff: no GNU tee inside WSL; skipping"
  exit 0
fi

# --- the subject -------------------------------------------------------------
OURS=${OURS:-}
if [ -z "$OURS" ]; then
  # shellcheck disable=SC1091
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  if ! command -v cargo >/dev/null 2>&1; then
    echo "tee-diff: no cargo inside WSL; skipping"
    echo "  install one with:  wsl -e sh -c 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain nightly --profile minimal'"
    exit 0
  fi
  target_dir=$HOME/.cache/slateos-diff-target
  ( cd "$root" && cargo build -p coreutils --bin tee \
      --target x86_64-unknown-linux-gnu --target-dir "$target_dir" ) >&2 || exit 1
  OURS=$target_dir/x86_64-unknown-linux-gnu/debug/tee
fi
if [ ! -x "$OURS" ]; then
  echo "tee-diff: $OURS is not executable" >&2
  exit 1
fi
case $OURS in
  /*) ;;
  *) OURS=$(cd "$(dirname "$OURS")" && pwd)/$(basename "$OURS") ;;
esac

# Diagnostics are referenced under a UTF-8 locale, as everywhere since §351.
# It is load-bearing for one family of cases here: `--output-error=bogus` is
# rejected by gnulib's `argmatch`, which renders the offending word with
# `quote()` — directional single quotes under a UTF-8 locale, ASCII apostrophes
# under `C`. Getting the locale wrong would make every argmatch case disagree
# for a reason that has nothing to do with tee.
export LC_ALL=C.UTF-8

pass=0; fail=0; xfail=0; xpass=0

# --- one name for both sides -------------------------------------------------
# Each binary is reached through a symlink called `tee`, in a directory that is
# the whole of `PATH` for that one invocation, so `argv[0]` is the bare word
# `tee` on both sides and the `tee: ` prefix on every diagnostic matches.
gnu_real=$(command -v tee)
bindir=$(mktemp -d)
mkdir -p "$bindir/ours" "$bindir/gnu"
ln -s "$OURS" "$bindir/ours/tee"
ln -s "$gnu_real" "$bindir/gnu/tee"

scratch=$(mktemp -d)
trap 'chmod -R u+rwx "$scratch" 2>/dev/null; rm -rf "$scratch" "$bindir"' EXIT

# --- knobs, reset before every case ------------------------------------------
# `SETUP` is shell run inside the case directory on each side before tee.
# `STDIN` is a `printf %b` string. `STDIN_FILE` names a file in the case
# directory to redirect from instead; `STDIN_CLOSED` shuts descriptor 0.
#
# `SKIP_STDOUT` drops stdout from the comparison, leaving the status, stderr
# and the directory. Exactly one family of cases needs it: `--h` and `--v` are
# probes of whether an abbreviation resolves, but what they resolve *to* is
# `--help` and `--version`, whose text differs from GNU's on purpose. Marking
# them `xfail` instead would throw away the thing they are there to test, since
# an xfail passes however the two differ — including if ours had rejected the
# abbreviation as ambiguous.
SETUP=; STDIN=; STDIN_FILE=; STDIN_CLOSED=; SKIP_STDOUT=

reset_knobs() { SETUP=; STDIN=; STDIN_FILE=; STDIN_CLOSED=; SKIP_STDOUT=; }

# --- what a directory looks like afterwards ----------------------------------
# Names, types, sizes and contents. Contents in full for anything small enough
# to read at a glance, as a digest above that: the large cases exist to prove a
# multi-buffer copy is complete, and 4 MiB of `od -An -c` in a failure report
# would bury the one line that mattered.
render() {
  local f=$1 sz
  # `stat`, not `wc -c <"$f"`: the `noperm` fixture is mode 000, and the
  # redirection in `wc -c <"$f"` is performed by the *shell*, which prints
  # `Permission denied` on its own stderr where no `2>/dev/null` on `wc` can
  # reach it. That leaked two lines into the harness's output on every run.
  # `stat` needs no read permission, so the size is still reported and only
  # the contents are withheld.
  sz=$(stat -c %s "$f" 2>/dev/null) || { printf '<unstattable>\n'; return 0; }
  printf '%s bytes\n' "$sz"
  if [ ! -r "$f" ]; then printf '  <unreadable>\n'
  elif [ "$sz" -le 512 ]; then od -An -c <"$f"
  else md5sum <"$f"
  fi
}

snapshot() {
  ( cd "$1" 2>/dev/null || exit 0
    find . -mindepth 1 | LC_ALL=C sort | while IFS= read -r f; do
      if [ -L "$f" ]; then printf 'L %s\n' "$f"
      elif [ -d "$f" ]; then printf 'D %s\n' "$f"
      else printf 'F %s ' "$f"; render "$f"
      fi
    done )
}

# --- run one case on both sides ----------------------------------------------
compare() {
  local od gd o_bin g_bin o_err g_err o_rc g_rc
  od=$scratch/o; gd=$scratch/g
  chmod -R u+rwx "$od" "$gd" 2>/dev/null
  rm -rf "$od" "$gd"; mkdir -p "$od" "$gd"
  o_bin=$(mktemp); g_bin=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)

  local side dir out err rc
  for side in ours gnu; do
    if [ "$side" = ours ]; then dir=$od; out=$o_bin; err=$o_err
    else dir=$gd; out=$g_bin; err=$g_err; fi
    ( cd "$dir" && eval "$SETUP" ) >/dev/null 2>&1
    if [ -n "$STDIN_CLOSED" ]; then
      ( cd "$dir" && timeout -k 2 60 env PATH="$bindir/$side" tee "$@" <&- >"$out" 2>"$err" )
    elif [ -n "$STDIN_FILE" ]; then
      ( cd "$dir" && timeout -k 2 60 env PATH="$bindir/$side" tee "$@" <"$STDIN_FILE" >"$out" 2>"$err" )
    else
      ( cd "$dir" && printf '%b' "$STDIN" | timeout -k 2 60 env PATH="$bindir/$side" tee "$@" >"$out" 2>"$err" )
    fi
    # Into `rc` on the very next line, before anything else runs. Writing
    # `if [ "$side" = ours ]; then o_rc=$?; ...` instead reads `$?` of the
    # *test*, which is 0 for `ours` and 1 for `gnu` every single time — an
    # exit status the harness manufactured itself, agreeing with the truth
    # exactly when the truth happened to be 0 and 1. It cost a full run.
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done

  # stdout via a file, not a pipe: in `x=$(tee | od)` the recorded status would
  # be od's. Same note as cat-diff.sh.
  #
  # Rendered through `render`, not `od` flat out. tee's stdout is a copy of its
  # input, so a case feeding it 300 KB would put 2 MB of octal into a shell
  # variable and then `tr` it — and a case that fed it more would put the shell
  # itself into a spin. Small outputs still get the full byte dump, which is
  # what almost every case here needs to see.
  local o_out g_out o_msg g_msg o_tree g_tree
  if [ -n "$SKIP_STDOUT" ]; then
    o_out='<not compared>'; g_out='<not compared>'
  else
    o_out=$(render "$o_bin"); g_out=$(render "$g_bin")
  fi
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  o_tree=$(snapshot "$od"); g_tree=$(snapshot "$gd")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] \
     && [ "$o_msg" = "$g_msg" ] && [ "$o_tree" = "$g_tree" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n    tree{%s}\n  gnu  (rc=%s): out{%s} err{%s}\n    tree{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$(printf '%s' "$o_tree" | tr -s ' \n' ' ')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')" \
    "$(printf '%s' "$g_tree" | tr -s ' \n' ' ')")
  reset_knobs
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report "tee $*"; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "tee $*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "tee $*" "$why"
  fi
  return 0
}

# --- a broken stdout ---------------------------------------------------------
# The whole reason `--output-error` exists, and not reachable through `compare`:
# it needs a reader on the far side of tee's stdout that closes early, which
# means a real pipeline, and tee's own status then has to be dug out of
# `PIPESTATUS` because `$?` in a pipeline is the last stage's.
#
# The input is large and finite. Large, because the reader must be gone before
# tee's next write, and tee writes in 8 KiB units; finite, because under a
# `nopipe` mode with a surviving file operand tee is *supposed* to keep copying
# after stdout dies, so an endless `yes` would hang here exactly as it did
# during the measurements this file is built from.
pipe_compare() {
  local od gd o_rc g_rc
  od=$scratch/o; gd=$scratch/g
  rm -rf "$od" "$gd"; mkdir -p "$od" "$gd"
  local o_err g_err
  o_err=$(mktemp); g_err=$(mktemp)

  local side dir err
  for side in ours gnu; do
    if [ "$side" = ours ]; then dir=$od; err=$o_err; else dir=$gd; err=$g_err; fi
    ( cd "$dir" && eval "$SETUP" ) >/dev/null 2>&1
    ( cd "$dir" \
      && head -c 4000000 /dev/zero \
         | timeout -k 2 60 env PATH="$bindir/$side" tee "$@" 2>"$err" \
         | head -c 1 >/dev/null
      printf '%s' "${PIPESTATUS[1]}" >"$dir.rc" )
    if [ "$side" = ours ]; then o_rc=$(cat "$od.rc"); else g_rc=$(cat "$gd.rc"); fi
  done
  rm -f "$od.rc" "$gd.rc"

  local o_msg g_msg o_tree g_tree
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  o_tree=$(snapshot "$od"); g_tree=$(snapshot "$gd")
  rm -f "$o_err" "$g_err"

  if [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ] && [ "$o_tree" = "$g_tree" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): err{%s} tree{%s}\n  gnu  (rc=%s): err{%s} tree{%s}' \
    "$o_rc" "$(printf '%s' "$o_msg" | tr '\n' '|')" "$(printf '%s' "$o_tree" | tr -s ' \n' ' ')" \
    "$g_rc" "$(printf '%s' "$g_msg" | tr '\n' '|')" "$(printf '%s' "$g_tree" | tr -s ' \n' ' ')")
  reset_knobs
}

pipe_case() { pipe_compare "$@"; report "yes | tee $* | head -c1"; }

xfail_pipe() {
  local why="$1"; shift
  pipe_compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "yes | tee $* | head -c1" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "yes | tee $* | head -c1" "$why"
  fi
  return 0
}

# =============================================================================
# --- the plain copy ----------------------------------------------------------
STDIN='hello\nworld\n'; run_case
STDIN='hello\nworld\n'; run_case out
STDIN='hello\nworld\n'; run_case one two three
# No trailing newline: the copy is of bytes, not lines, and a tee that read by
# lines would lose or invent the last one.
STDIN='no newline here'; run_case out
# Empty input still creates — and truncates — every operand. The single most
# commonly missed case, because nothing is copied and it looks like a no-op.
STDIN=''; run_case out
SETUP='printf "junk that must go\n" > out'; STDIN=''; run_case out
# Every byte survives, including NUL and the ones that are not UTF-8. Our tee
# reads into `[u8]`, but a `String` somewhere in the pipeline would show here.
STDIN='\0\377\200\n\r\t x'; run_case bin
# Bigger than one 8 KiB read, so the loop runs many times and any off-by-one in
# the buffer slicing corrupts the middle rather than the ends.
#
# Deterministic, and that is not fussiness: the fixture is built once per side,
# so `/dev/urandom` here would hand the two tees different inputs and the case
# could never agree. `seq` gives varying byte lengths per line — the copy is
# then not aligned to anything — and the tail supplies the bytes `seq` cannot,
# so the case also covers NUL and the high half crossing a buffer boundary.
STDIN_FILE=big
SETUP='{ seq 1 200000; printf "\0\377\200\n"; } | head -c 300000 > big'
run_case copy1 copy2

# --- append ------------------------------------------------------------------
SETUP='printf "existing\n" > out'; STDIN='added\n'; run_case -a out
SETUP='printf "existing\n" > out'; STDIN='added\n'; run_case --append out
# `-a` on a file that does not exist creates it, rather than failing.
STDIN='fresh\n'; run_case -a newfile
# Without `-a` the same fixture is truncated first, which is the contrast the
# case above is only meaningful against.
SETUP='printf "existing\n" > out'; STDIN='added\n'; run_case out

# --- the same name twice -----------------------------------------------------
# Two descriptors onto one file, both writing from offset 0 — so the file ends
# up one copy long, not two, and only because the writes are the same bytes at
# the same offsets. A tee that deduplicated its operands would also produce one
# copy and pass this; the `-a` variant below is what tells them apart, since
# two appending descriptors do not share an offset.
STDIN='ab\n'; run_case dup dup
SETUP='printf "seed\n" > dup'; STDIN='ab\n'; run_case -a dup dup

# --- reading a file that is also an operand ----------------------------------
# tee opens its operands before reading a byte, so `self` is truncated to
# nothing and then read as nothing. The result is an empty file and a silent
# success, which surprises people but is what both sides must do.
SETUP='printf "seed content\n" > self'; STDIN_FILE=self; run_case self
# There is deliberately no `-a self` case. Appending to the file being read is
# not a difference between the two tees, it is a fixed point of neither: every
# byte tee appends is a byte it then reads, so both sides copy until something
# stops them. Measured before it was removed from here, they reached 188 MB and
# 166 MB respectively in the 60 s `timeout` allowed, differing only in how fast
# the machine happened to run each. A case whose result is a race against the
# clock cannot be compared, so it is not attempted.

# --- `-` is a file name, not stdout ------------------------------------------
# Unlike almost every other utility. tee has no operand meaning "standard
# output" because standard output is not optional.
STDIN='dash\n'; run_case -
STDIN='dash\n'; run_case -- -
STDIN='dash\n'; run_case -- -a

# --- -i ----------------------------------------------------------------------
# Accepted, and with nothing to interrupt it must not change the copy.
STDIN='x\n'; run_case -i out
STDIN='x\n'; run_case --ignore-interrupts out
STDIN='x\n'; run_case -ai out

# --- outputs that cannot be opened -------------------------------------------
# The status is 1 and the diagnostic names the file, but the *other* operands
# are still written: tee does not abort a run because one output failed, unless
# an `exit` mode says to.
STDIN='x\n'; run_case good1 nodir/bad good2
SETUP='mkdir adir'; STDIN='x\n'; run_case good adir good2
SETUP=': > noperm; chmod 000 noperm'; STDIN='x\n'; run_case good noperm good2
# Every failing operand is diagnosed, not just the first.
STDIN='x\n'; run_case nodir/a nodir/b good

# --- --output-error, against an open failure ---------------------------------
# The `exit` modes stop at the first failure, so `good2` is never created; the
# `warn` modes carry on and it is. That difference is invisible in stdout and
# stderr and lives entirely in the directory snapshot.
for mode in warn warn-nopipe exit exit-nopipe; do
  STDIN='x\n'; run_case "--output-error=$mode" good1 nodir/bad good2
done
STDIN='x\n'; run_case -p good1 nodir/bad good2
# An open failure is not a broken pipe, so even the `nopipe` modes report it.
STDIN='x\n'; run_case --output-error=exit-nopipe nodir/bad good

# --- --output-error, against a broken stdout ---------------------------------
# Here the `nopipe` modes finally diverge from the others: same failure, same
# errno, reported by two of the four and dropped by the other two. With a file
# operand surviving, the copy continues either way and the file ends up holding
# the whole input.
pipe_case --output-error=warn out
pipe_case --output-error=warn-nopipe out
pipe_case -p out
pipe_case --output-error=exit out
pipe_case --output-error=exit-nopipe out
# With no file operand there is nothing left to write to, so every mode stops —
# but they still differ in whether they say why, and in the status.
pipe_case --output-error=warn
pipe_case --output-error=warn-nopipe
pipe_case --output-error=exit
pipe_case --output-error=exit-nopipe

# --- --output-error, the argument itself -------------------------------------
STDIN='x\n'; run_case --output-error=bogus out
# The empty word is *ambiguous*, not invalid: gnulib's argmatch treats it as a
# prefix, and the empty prefix matches all four modes.
STDIN='x\n'; run_case --output-error= out
# Unambiguous prefixes are accepted; `warn` is a prefix of `warn-nopipe` but is
# also an exact match, and an exact match wins.
STDIN='x\n'; run_case --output-error=e out
STDIN='x\n'; run_case --output-error=warn out
STDIN='x\n'; run_case --output-error=exit-n out
# Ambiguous ones are not: `w` matches both `warn` and `warn-nopipe`.
STDIN='x\n'; run_case --output-error=w out
# The value is optional, so a separate word is *not* consumed as the value — it
# stays an operand, and the mode is the `--output-error`-with-no-value default.
# This is the reason the table entry is `Optional` and not `Required`.
STDIN='x\n'; run_case --output-error warn
STDIN='x\n'; run_case --output-error out

# --- option parsing ----------------------------------------------------------
STDIN='x\n'; run_case -Z out
STDIN='x\n'; run_case --nosuch out
# Options are permuted: an option after an operand still applies.
SETUP='printf "existing\n" > out'; STDIN='added\n'; run_case out -a
STDIN='x\n'; run_case -- -a out
# Bundled shorts.
SETUP='printf "existing\n" > out'; STDIN='added\n'; run_case -ai out
STDIN='x\n'; run_case -pa out
# `-p` takes no value, so the next word is an operand.
STDIN='x\n'; run_case -p out

# --- the long-option table ---------------------------------------------------
# `--=x` is the empty prefix, which matches every entry, so glibc's "ambiguous"
# line is a direct readout of the whole table — names *and* declaration order,
# since it reports the first match and then the rest. It is the only probe that
# can see the tail of the table, and it is what caught `cmp`'s reversed
# `--help`/`--version` pair. `scripts/getopt-ambiguity-check.py` reads GNU's
# copy the same way.
STDIN='x\n'; run_case --=x out
STDIN='x\n'; run_case --a out
STDIN='x\n'; run_case --i out
STDIN='x\n'; run_case --o out
# Status and stderr only: both resolve, and what they resolve to is the one
# thing here that is ours rather than GNU's. See `SKIP_STDOUT` above.
STDIN='x\n'; SKIP_STDOUT=1; run_case --h out
STDIN='x\n'; SKIP_STDOUT=1; run_case --v out
STDIN='x\n'; run_case --app out
STDIN='x\n'; run_case --out=warn out
STDIN='x\n'; run_case --ignore out

# --- names that are not text -------------------------------------------------
# The defect this rewrite exists for: the version it replaces collected argv as
# `Vec<String>` and panicked outright on the first of these.
STDIN='x\n'; run_case $'\xff\xfe-bad'
STDIN='x\n'; run_case -a $'\xff\xfe-bad'
STDIN='x\n'; run_case good $'\xff\xfe-bad' good2

# --- names that reach a diagnostic -------------------------------------------
# Every one of these fails to open, so the name is interpolated into a
# `tee: NAME: ...` line and the *rendering* of the name is what is under test.
# coreutils' tee spells all three of its diagnostic sites `quotef (files[i])` —
# shell-escape style, quotes appearing only when the name needs them — and our
# `quotef_os` is that function. So these agree, and they are ordinary cases.
#
# They were written as `xfail` first, on the assumption carried over from
# `cmp-diff.sh` that a name reaching our output is quoted where GNU's is raw
# (`design-decisions.md` §373). That is true of diffutils, which interpolates
# with a bare `%s`; it is not true here, and the harness reported all three as
# XPASS until this comment replaced the assumption.
STDIN='x\n'; run_case 'sp ace/bad'
STDIN='x\n'; run_case $'nl\nname/bad'
STDIN='x\n'; run_case $'\xff\xfe/bad'
STDIN='x\n'; run_case 'sp ace/bad' good

# =============================================================================
# --- differences on purpose --------------------------------------------------
STDIN='x\n'; xfail_case 'our --help omits the GNU project ancillary block' --help
STDIN='x\n'; xfail_case 'our --version names SlateOS' --version

# The default mode. GNU leaves SIGPIPE fatal and is killed by it (status 141,
# no diagnostic); we have no signals, so the default takes upstream's own
# ignored-SIGPIPE path — drop the write, keep copying to the file, exit 0.
xfail_pipe 'GNU dies of SIGPIPE (141); SlateOS has no signals' out
xfail_pipe 'GNU dies of SIGPIPE (141); SlateOS has no signals'

# Closed stdin. GNU diagnoses twice — `tee: read error: Bad file descriptor`
# from the failed read, then `tee: standard input: Bad file descriptor` from
# gnulib's `close_stdin` atexit hook — and exits 1. We print nothing and exit
# 0, and the reason is neither of those diagnostics: Rust's std reopens any
# closed standard descriptor onto `/dev/null` before `main` is entered (a
# deliberate hardening — otherwise the first file a program opens becomes its
# stdout). So our `main` is handed an empty stdin, copies zero bytes, and is
# right to call that a success. Nothing in `tee.rs` can see the difference, and
# on SlateOS proper the behaviour is our std port's to decide, not tee's.
#
# `tee /proc/self/fd/0 <&-` is the direct readout: that symlink exists only
# while the descriptor is open, so it opens for us and is `No such file or
# directory` for GNU.
STDIN_CLOSED=1; xfail_case "Rust's std reopens closed stdin on /dev/null before main" out
STDIN_CLOSED=1; xfail_case "Rust's std reopens closed stdin on /dev/null before main" -a out

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
