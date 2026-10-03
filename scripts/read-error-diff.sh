#!/usr/bin/env bash
# read-error-diff.sh — one question, asked of every utility that answers it:
# what happens when standard input cannot be read?
#
# ## Why a cross-utility harness
#
# `write-error-diff.sh` asks the output half of this; this is the input half,
# and it fails in the same silent, per-binary way. Two layers stand between a
# Rust program and a closed descriptor 0:
#
# 1. The runtime opens `/dev/null` on a standard descriptor that is closed at
#    startup. `guard_std_fds!` and `stdfd::restore` undo that, per binary.
# 2. `std::io::stdin()` answers `EBADF` with end of input, by design. A program
#    that reads through it sees an empty file even once the guard is in.
#
# So `wc <&-` printed `0 0 0` and exited 0, where GNU says
# `wc: 'standard input': Bad file descriptor`, prints its row anyway and exits
# 1 -- and nothing in the per-utility harnesses asked. The fix is per binary
# too: read descriptor 0 itself (`stdfd::RawStdin`, `stdfd::read`,
# `stdio::StdioReader`), then say what upstream says, which differs between
# utilities: `-`, `'standard input'`, `standard input` or `closing standard
# input`, sometimes twice, because most of them also close standard input at
# the end and report that failing too. See `known-issues/`
# `B-COREUTILS-A-CLOSED-STANDARD-INPUT-READS-AS-EMPTY`.
#
# ## The two shapes
#
# * `closed` -- `<&-`: every read fails with `EBADF`, and so does the final
#   close.
# * `dir` -- standard input is a directory: it opens, every read fails with
#   `EISDIR`, and the final close succeeds -- which separates the read error's
#   wording from the close error's.
#
# Standard output, standard error and the status are compared, each on its
# own: unlike a write failure, a read failure usually still prints something
# (`wc`'s row of zeros), and that is part of the answer.
#
# Add a utility to `DIFF_BINS` and to the cases below in the commit that
# converts it.
DIFF_PROG='read-error'
# Not the installed binaries: see `write-error-diff.sh` and `diff-wsl.sh`'s
# "Why a built reference".
DIFF_GNU_SOURCE=9.4
DIFF_NO_REF=1
DIFF_NEED="timeout"
DIFF_BINS="b2sum base32 base64 cat cksum comm cut dircolors expand factor fold head join
           md5sum nl numfmt od paste sha1sum sha256sum sha512sum sum tee tr tsort
           unexpand uniq wc"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0

have() { [ -e "$bindir/ours/$1" ]; }

# --- fixtures -----------------------------------------------------------------
fix=$DIFF_TMP/fix
mkdir -p "$fix/adir"
printf 'alpha\nbravo\ncharlie\n' > "$fix/f"
printf 'a\tb\tc\n\tindented\n'   > "$fix/tabs"

# --- run one case on both sides -----------------------------------------------
MODE=closed
AGREED=no; REPORT=

compare() {
  local prog="$1"; shift
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  o_out=$(mktemp); g_out=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)
  for side in ours gnu; do
    if [ "$side" = ours ]; then out=$o_out; err=$o_err
    else out=$g_out; err=$g_err; fi
    # A spelling per shape: a redirection held in a variable would arrive as
    # an argument. Run from the fixture directory so names stay relative and
    # read the same on both sides.
    case $MODE in
      closed) ( cd "$fix" && timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) <&- >"$out" 2>"$err" ;;
      dir)    ( cd "$fix" && timeout -k 2 60 env PATH="$bindir/$side" "$prog" "$@" ) <"$fix/adir" >"$out" 2>"$err" ;;
    esac
    # On the very next line, before anything else can replace it.
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done
  local o_text g_text o_msg g_msg
  o_text=$(od -An -c <"$o_out"); g_text=$(od -An -c <"$g_out")
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  if [ "$o_rc" = "$g_rc" ] && [ "$o_text" = "$g_text" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(tr '\n' '|' <"$o_out")" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(tr '\n' '|' <"$g_out")" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  rm -f "$o_out" "$g_out" "$o_err" "$g_err"
}

report() {
  local label="$1"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK    %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF  %s\n%s\n' "$label" "$REPORT"
  fi
  return 0
}

# `sweep PROG ARGS...` -- the same invocation in both shapes.
sweep() {
  have "$1" || return 0
  local m
  for m in closed dir; do
    MODE=$m
    compare "$@"
    report "$(printf '[%-6s] %s' "$m" "$*")"
  done
}

# --- standard input as the only input, as `-`, and among named files ---------
# The position matters: a read error is reported where the input is read, and
# the close of standard input at the end, after every other operand.
sweep cut       -c1
sweep cut       -c1 -
sweep cut       -c1 f -
sweep cut       -c1 - f
sweep expand
sweep expand    tabs -
sweep fold
sweep fold      -w 3 f -
sweep nl
sweep nl        -
sweep nl        f -
sweep nl        - f
sweep paste     -
sweep paste     - -
sweep paste     -s -
sweep paste     f -
sweep unexpand
sweep unexpand  -a tabs -
sweep wc
sweep wc        -l
sweep wc        -c
sweep wc        -
sweep wc        - -
sweep wc        f -
sweep wc        --total=only -
sweep wc        --files0-from=-
# The digests: `PROG: -: ...` for the read, `PROG: standard input: ...` for
# the close. With `-c` standard input is the checksum list, not the data.
sweep md5sum
sweep md5sum    -
sweep md5sum    f -
sweep md5sum    -c
sweep md5sum    -c -
sweep sha1sum
sweep sha256sum
sweep sha256sum --tag -
sweep sha512sum
sweep b2sum
sweep b2sum     -l 128 -
sweep cksum
sweep cksum     -a md5 -
sweep cksum     f -
sweep sum
sweep sum       -s
sweep sum       f -
# `cat` finds a closed standard input with `fstat`, before any read, and says
# `closing standard input` at the end; `head` names it `'standard input'`.
sweep cat
sweep cat       -
sweep cat       f -
sweep cat       -n -
sweep head
sweep head      -c 3
sweep head      -n 1 -
sweep head      f -
# A read error that ends the run: no close is reached.
sweep join      - f
sweep join      f -
sweep comm      - f
sweep comm      f -
sweep tsort
sweep numfmt
sweep factor
sweep base64
sweep base64    -d
sweep base32
sweep tr        a b
sweep tr        -d a
# A read error reported, then the input closed and that reported too.
sweep dircolors -
sweep uniq
sweep uniq      -c -
sweep tee
sweep od
sweep od        -c -
sweep od        f -

# --- a file named on the command line is not standard input ------------------
# Neither shape may change anything here: standard input is never read, so it
# is never reported, not even by the close at the end.
sweep cut       -c1 f
sweep nl        f
sweep wc        f
sweep md5sum    f
sweep sum       f
sweep cat       f
sweep head      f
sweep od        f
sweep uniq      f

printf '\n'
[ -n "$DIFF_SKIPPED" ] && printf 'not compared (no reference or no build):%s\n' "$DIFF_SKIPPED"
printf '%d passed, %d differed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
