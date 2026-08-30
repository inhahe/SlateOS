#!/usr/bin/env bash
# Differential test: our ed against GNU ed.
#
# ## What is peculiar about certifying an editor
#
# Every other harness here compares three things: stdout, stderr, and the exit
# status. For `ed` that is not enough, because `ed`'s whole purpose is the
# fourth thing — **the bytes left on disk**. A `w` that writes the buffer it
# should have written and a `w` that writes an *empty* buffer both print a
# plausible number and both exit 0; the only place they differ is the file. So
# every case here runs each side in its own private copy of the fixture
# directory and, after the run, compares those two directories file for file as
# hex dumps. That is not belt-and-braces: the bug this ed was rewritten to fix
# was exactly of that shape. The old implementation read the file with
# `fs::read_to_string`, could not tell "not valid UTF-8" from "does not exist",
# reported an empty buffer, and a subsequent `w` truncated the user's file to
# nothing while printing `0` and exiting 0. Three of the four observables agreed
# with GNU. Only the disk disagreed.
#
# ## The second peculiarity: what stdin *is* changes what ed does
#
# GNU ed asks `is_regular_file(stdin)` — not `isatty` — and branches on the
# answer twice:
#
#   * a script read from a **regular file** stops at the first error, and its
#     `-v` explanation carries a `script, line N: ` prefix;
#   * the same bytes arriving down a **pipe** carry on past the error with a
#     bare sentence.
#
# and a third time at startup: `ed nosuch.txt` with a file-driven stdin prints
# the OS's complaint and exits 2 without editing anything, while over a pipe it
# prints the same complaint and carries on with an empty buffer at status 0.
# Testing only one of the two kinds of stdin therefore certifies half of ed. So
# `run_pipe` and `run_file` feed byte-identical scripts by the two routes, and
# a good many cases appear once in each form on purpose.
#
# Run `OURS=/usr/bin/ed ./scripts/ed-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

# Into WSL, build ours for Linux, find GNU's, and put both behind the one name
# `ed` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG='ed'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; kbug=0; kfixed=0

# --- the fixture template -----------------------------------------------------
#
# Copied fresh for each side of each case, so that a case whose `w` rewrites
# `f.txt` cannot change what the next case reads.
template=$DIFF_TMP/template
mkdir -p "$template"
printf 'alpha\nbeta\ngamma\n'          > "$template/f.txt"
printf 'one\ntwo\nthree\nfour\nfive\n' > "$template/five.txt"
printf 'abc'                           > "$template/nonl.txt"
printf 'a\r\nb\r\n'                    > "$template/crlf.txt"
printf 'x\n\n\ny\n'                    > "$template/blanks.txt"
# Bytes no `String` can hold. Reading this file is the case the rewrite exists
# for: the previous implementation turned it into an empty buffer.
printf 'A\xff\xfe\nB\x80\n'            > "$template/bytes.txt"
: > "$template/empty.txt"

# --- one invocation of one side ----------------------------------------------
#
# `$1` is `ours` or `gnu`; `$2` is the case's private directory, already
# populated; `$3` names how the script reaches stdin — `pipe`, `file`, or
# `none` for `/dev/null`. `$4` is the script, still in its `\n` form.
#
# The `%b` expansion happens *here*, at the point of use, and never through a
# `$(...)`. Command substitution strips trailing newlines, and for ed that is
# not a cosmetic loss: a script whose last line has no terminator is a
# different script. Measured — `printf '1d\nq\n' | ed f` exits 1, and the same
# bytes without the final newline exit 2. This harness did it the wrong way
# round for one run and reported three quit-status differences that were
# entirely its own.
#
# The invocation goes through `$bindir/$side/ed`, a symlink whose *name* is
# `ed` on both sides, so a diagnostic's `ed: ` prefix comes from the same
# `argv[0]` and a difference in it is a difference in ed.
run_side() {
  local side=$1 dir=$2 kind=$3 script=$4 out=$5 err=$6; shift 6
  (
    cd "$dir" || exit 125
    case $kind in
      file)
        printf '%b' "$script" > .script
        env PATH="$bindir/$side" ed "$@" < .script > "$out" 2> "$err"
        ;;
      none)
        env PATH="$bindir/$side" ed "$@" < /dev/null > "$out" 2> "$err"
        ;;
      *)
        # A real pipe, which is what makes `is_regular_file(stdin)` false.
        printf '%b' "$script" | env PATH="$bindir/$side" ed "$@" > "$out" 2> "$err"
        ;;
    esac
  )
}

# A directory rendered as one comparable string: every file's name, then its
# bytes as hex. `.script` is excluded — it is the harness's own, and only one
# of the three stdin kinds creates it.
dir_state() {
  local dir=$1 f
  ( cd "$dir" || exit 1
    find . -type f ! -name .script | LC_ALL=C sort | while IFS= read -r f; do
      printf '%s:' "$f"; od -An -tx1 < "$f" | tr -s ' \n' ' '; printf '\n'
    done )
}

# Sets `AGREED` and `REPORT`. `$1` is the stdin kind, `$2` the script, and the
# rest is argv.
compare() {
  local kind=$1 script=$2; shift 2
  local o_dir g_dir o_out g_out o_err g_err o_bin g_bin o_rc g_rc o_st g_st
  o_dir=$DIFF_TMP/o; g_dir=$DIFF_TMP/g
  rm -rf "$o_dir" "$g_dir"
  cp -r "$template" "$o_dir"; cp -r "$template" "$g_dir"
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout goes to a file, never into a pipe: in `x=$(ed … | od)` the status
  # recorded belongs to `od`, so every failing case would be scored a pass.
  run_side ours "$o_dir" "$kind" "$script" "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side gnu  "$g_dir" "$kind" "$script" "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(od -An -tx1 <"$o_bin"); g_out=$(od -An -tx1 <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  # stderr as text, not merely for presence. Sound only because the reference
  # is glibc's: our `errmsg` prints POSIX's strerror strings, which agree with
  # glibc and did not agree with the Cygwin host these harnesses used to run on.
  local o_msg g_msg
  if [ "${ERR_MODE:-full}" = first-line ]; then
    o_msg=$(head -1 "$o_err"); g_msg=$(head -1 "$g_err")
  else
    o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  fi
  rm -f "$o_err" "$g_err"

  o_st=$(dir_state "$o_dir"); g_st=$(dir_state "$g_dir")

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] &&
     [ "$o_msg" = "$g_msg" ] && [ "$o_st" = "$g_st" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  if [ "$o_st" != "$g_st" ]; then
    REPORT=$(printf '%s\n  DISK ours: %s\n  DISK gnu : %s' "$REPORT" \
      "$(printf '%s' "$o_st" | tr '\n' '|')" "$(printf '%s' "$g_st" | tr '\n' '|')")
  fi
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

# A script's newlines are written `\n` in the case list and stay that way all
# the way down to `run_side`, which is the only place that expands them. See
# the note there: expanding earlier would mean a `$(...)`, and a `$(...)` eats
# the trailing newline that decides ed's exit status.
#
# `run_pipe SCRIPT ARGS...` — the script arrives down a pipe, so ed carries on
# past an error and its `-v` sentences are bare.
run_pipe() {
  local raw=$1; shift
  compare pipe "$raw" "$@"
  report "pipe: ed $* <<< '$raw'"
}

# `run_file SCRIPT ARGS...` — the same bytes in a regular file, so ed stops at
# the first error and prefixes its `-v` sentences with `script, line N: `.
run_file() {
  local raw=$1; shift
  compare file "$raw" "$@"
  report "file: ed $* < script('$raw')"
}

# `run_null ARGS...` — nothing on stdin at all. `/dev/null` is a character
# device, so this is the not-a-regular-file branch with no commands in it: it
# isolates what ed does at *startup* from what it does with a script.
run_null() {
  compare none '' "$@"
  report "null: ed $*"
}

# `usage_case ARGS...` — a command line ed should refuse, compared on stdout,
# status and the *first line* of stderr. The block beneath that first line is
# ours on purpose: it names SlateOS and omits the GNU project's URLs.
usage_case() {
  # Set and reset rather than prefixing the call: in bash an assignment
  # prefixed to a *function* call stays in effect after it returns, which would
  # silently put every later case into first-line mode.
  ERR_MODE=first-line
  compare none '' "$@"
  ERR_MODE=full
  report "usage: ed $* (first stderr line)"
}

xfail_pipe() {
  local reason=$1 raw=$2; shift 2
  compare pipe "$raw" "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf "XFAIL pipe ed %s <<< '%s'  (%s)\\n" "$*" "$raw" "$reason"
  else
    xpass=$((xpass+1))
    printf "XPASS pipe ed %s <<< '%s'\\n  now agrees with GNU, so this reason is stale: %s\\n" \
      "$*" "$raw" "$reason"
  fi
  return 0
}

xfail_case() {
  local reason=$1; shift
  compare none '' "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL ed %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS ed %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

# --- known bugs ---------------------------------------------------------------
#
# A divergence that is *not* deliberate but is not fixed yet. It is loud on
# every run and names its `known-issues.md` entry, but it does not fail the
# run — because a harness that is permanently red is a harness nobody reads,
# and that is how bc's `quit` stayed broken for months.
kbug_pipe() {
  local entry=$1 raw=$2; shift 2
  compare pipe "$raw" "$@"
  if [ "$AGREED" = no ]; then
    kbug=$((kbug+1))
    printf "KBUG pipe ed %s <<< '%s'  (known-issues.md -> %s)\\n%s\\n" \
      "$*" "$raw" "$entry" "$REPORT"
  else
    kfixed=$((kfixed+1))
    printf "KFIXED pipe ed %s <<< '%s'\\n  agrees with GNU now; close known-issues.md -> %s\\n" \
      "$*" "$raw" "$entry"
  fi
  return 0
}

# === reading a file ===========================================================

run_pipe 'q\n'                       f.txt
run_pipe 'q\n'                       empty.txt
run_pipe 'q\n'                       nonl.txt     # `Newline appended`, then 4
run_pipe 'q\n'                       crlf.txt     # CRs are kept by default
run_pipe 'q\n' --strip-trailing-cr   crlf.txt
run_pipe 'q\n'                       bytes.txt    # the reason this ed exists
run_pipe 'q\n'                       blanks.txt
run_pipe 'q\n' -s                    f.txt        # -s drops the byte count
run_pipe 'q\n'                                    # no operand at all
# A directory opens and then will not read, which GNU grades as a *command*
# failure — two lines on stderr and status 1 — where a file that never opened
# is neither. Both kinds of stdin, because the file-driven side exits 2 after
# the first line and never reaches the second.
run_pipe 'q\n'                       .
run_file 'q\n'                       .
run_pipe 'q\n'                       nosuch.txt
run_file 'q\n'                       nosuch.txt   # file-driven: exits 2, no edit
run_null                             nosuch.txt
# The startup failure must not *create* the file. Checked on disk, not stdout.
run_file 'a\nX\n.\nw\nq\n'           nosuch.txt
run_pipe 'q\n' -q                    nosuch.txt   # -q silences the stderr side

# === addressing ===============================================================

run_pipe ',p\nq\n'                   f.txt
run_pipe '%p\nq\n'                   f.txt
run_pipe ';p\nq\n'                   five.txt
run_pipe '1,3p\nq\n'                 five.txt
run_pipe '2\np\nq\n'                 five.txt
run_pipe '$p\nq\n'                   five.txt
run_pipe '$-2p\nq\n'                 five.txt
run_pipe '1\n+p\nq\n'                five.txt
run_pipe '3\n-p\nq\n'                five.txt
run_pipe '1\n+2p\nq\n'               five.txt
run_pipe '2\n-1p\nq\n'               five.txt
run_pipe '1+1+1p\nq\n'               five.txt
run_pipe '0p\nq\n'                   f.txt        # 0 is not printable
run_pipe '9p\nq\n'                   f.txt        # past the end
run_pipe '3,1p\nq\n'                 f.txt        # reversed
run_pipe '99999999999999999999p\nq\n' f.txt       # too large to hold
run_pipe 'p\nq\n'                    empty.txt    # nothing to print
run_pipe '=\nq\n'                    f.txt        # `=` defaults to $
run_pipe '1,2=\nq\n'                 f.txt        # and prints the address
run_pipe '0=\nq\n'                   f.txt
run_pipe '=\nq\n'                    empty.txt
run_pipe '1n\nq\n'                   f.txt
run_pipe ',n\nq\n'                   f.txt

# === command suffixes =========================================================

run_pipe '1 p\nq\n'                  f.txt        # a space before is fine
run_pipe '1p \nq\n'                  f.txt        # one after is not
run_pipe '1pp\nq\n'                  f.txt        # prints once, not twice
run_pipe '1pn\nq\n'                  f.txt
run_pipe '1pl\nq\n'                  f.txt
run_pipe '1px\nq\n'                  f.txt        # not a print suffix
run_pipe '1dp\nq\n'                  f.txt
run_pipe '1dn\nq\n'                  f.txt
run_pipe '1dl\nq\n'                  f.txt
# `p`, `n` and `l` are flags that add up, not styles that replace each other,
# and both letters of a pair contribute. All six orderings are checked because
# an implementation that overwrites instead of merging passes half of them.
run_pipe '1np\nq\n'                  f.txt
run_pipe '1nl\nq\n'                  f.txt
run_pipe '1ln\nq\n'                  f.txt
run_pipe '1lp\nq\n'                  f.txt
run_pipe '1nn\nq\n'                  f.txt
run_pipe '1ll\nq\n'                  f.txt
run_pipe 'Z\nq\n'                    f.txt        # unknown command
run_pipe 'z\nq\n'                    five.txt     # GNU has no `z` either

# === list =====================================================================

run_pipe '1l\nq\n' -s                bytes.txt
run_pipe ',l\nq\n' -s                bytes.txt
run_pipe '1l\nq\n' -s                blanks.txt
# The 72-column fold, and that it never falls inside an escape.
run_pipe 'a\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n.\n$l\nq\n' -s empty.txt
run_pipe 'a\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\200\n.\n$l\nq\n' -s empty.txt
run_pipe 'a\n\t\a\b\f\r\v\\$\n.\n$l\nq\n' -s      empty.txt
# The margin is 72 *printed columns*, decided before each escape and never
# after the last one. 71 a's + `\200` is 75 columns and does not fold; one more
# `a` and it does. 36 tabs are 36 bytes and 72 columns, so a 37th starts a line.
run_pipe 'a\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n.\n$l\nq\n' -s empty.txt
run_pipe 'a\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n.\n$l\nq\n' -s empty.txt
run_pipe 'a\naaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\200\n.\n$l\nq\n' -s empty.txt
run_pipe 'a\n\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t\n.\n$l\nq\n' -s empty.txt

# === editing ==================================================================

run_pipe '2d\n,p\nw\nq\n'            f.txt
run_pipe '1,2d\n,p\nw\nq\n'          five.txt
run_pipe 'a\nX\nY\n.\n,p\nw\nq\n'    f.txt
run_pipe '0a\nX\n.\n,p\nw\nq\n'      f.txt
run_pipe '1i\nX\n.\n,p\nw\nq\n'      f.txt
run_pipe '0i\nX\n.\n,p\nw\nq\n'      f.txt
run_pipe '2c\nX\n.\n,p\nw\nq\n'      f.txt
run_pipe ',c\nX\n.\n,p\nw\nq\n'      f.txt
run_pipe 'a\nX\n.\nw\nq\n'           empty.txt
# Appending after the last line, then writing: the buffer and the file must
# agree about the final newline.
run_pipe 'a\nX\n.\nw\nq\n'           nonl.txt
run_pipe '$d\nw\nq\n'                nonl.txt
run_pipe 'a\n\xff\xfe\n.\nw\nq\n'    bytes.txt    # bytes survive the round trip
run_pipe 'd\nw\nq\n'                 bytes.txt
run_pipe '9d\nq\n'                   f.txt

# === moving, copying and joining ==============================================
#
# `m` and `t` take a *third* address after the command letter, which no other
# command does, and "where did the lines land" is not the same for the two:
# `m` removes before it inserts, so a destination past the range shifts down by
# the number of lines moved. Every case here reads `.` with `.=` *before* any
# `,p`, because `,p` sets `.` to the last line it printed and would otherwise
# measure the harness rather than the command.

run_pipe '1m$\n.=\n,p\nw\nq\n'       f.txt
run_pipe '2m0\n.=\n,p\nq\n'          f.txt
run_pipe '1m1\n.=\n,p\nq\n'          f.txt        # a no-op that still modifies
run_pipe '1,2m$\n.=\n,p\nq\n'        five.txt
run_pipe '1,2m0\n.=\n,p\nq\n'        five.txt
run_pipe '2,4m1\n.=\n,p\nq\n'        five.txt
run_pipe '1,2m1\nq\n'                f.txt        # a destination inside itself
run_pipe '1,2m2\n.=\n,p\nq\n'        f.txt        # the far edge is not inside
run_pipe 'm$\n.=\n,p\nq\n'           f.txt        # default range is `.,.`
run_pipe '1m\n.=\n,p\nq\n'           f.txt        # default destination is `.`
run_pipe '1m-1\n.=\n,p\nq\n'         f.txt
run_pipe '1m/gamma/\n.=\n,p\nq\n'    f.txt        # the destination may be a search
run_pipe '1m$p\nq\n'                 f.txt        # ...and may carry a suffix
run_pipe '1m5\nq\n'                  f.txt        # past the end
run_pipe '1m$x\nq\n'                 f.txt        # a suffix that is not one
run_pipe '3,1m0\nq\n'                f.txt        # a reversed range
run_pipe '1mz\nq\n'                  f.txt        # a destination that is not one

run_pipe '1t$\n.=\n,p\nw\nq\n'       f.txt
run_pipe '1t0\n.=\n,p\nq\n'          f.txt
run_pipe '1,2t1\n.=\n,p\nq\n'        f.txt        # copying a range into itself
run_pipe '1,3t2\n.=\n,p\nq\n'        five.txt
run_pipe '1,2t2\n.=\n,p\nq\n'        f.txt
run_pipe 't\n.=\n,p\nq\n'            f.txt
run_pipe '1t9\nq\n'                  f.txt
run_pipe '1t$p\nq\n'                 f.txt

run_pipe '1,2j\n.=\n,p\nw\nq\n'      f.txt
run_pipe ',j\n.=\n,p\nw\nq\n'        five.txt
run_pipe '2j\n.=\n,p\nq\n'           f.txt        # one address: both ends of it
run_pipe '$j\n.=\n,p\nq\n'           f.txt
run_pipe 'j\n.=\n,p\nq\n'            f.txt        # `.,.+1`, and `.` is `$` here
run_pipe '1\nj\n.=\n,p\nq\n'         f.txt        # ...so from line 1 it joins
run_pipe '1,1j\n.=\n,p\nq\n'         f.txt        # one line is already joined
run_pipe '1,1jp\n.=\nq\n'            f.txt        # ...and the suffix still runs
run_pipe '1,2jp\nq\n'                f.txt
run_pipe '1,2jn\nq\n'                f.txt
run_pipe '1,2j\nw\nq\n'              nonl.txt     # joining the only line
run_pipe '1,2j\nq\n'                 empty.txt

# === marks ====================================================================
#
# `k` names a line and `'x` addresses it. The name follows the *line*, not the
# line number, so a mark set before an insertion above it still points at the
# same text afterwards — which is the whole reason for having marks.

run_pipe "1ka\n'ap\nq\n"             f.txt
run_pipe "2kb\n0a\nX\n.\n'bp\nq\n"   f.txt        # the line moved; the mark went too
run_pipe "1ka\n1m\$\n'ap\n.=\nq\n"   f.txt        # ...and `m` carries it as well
run_pipe "1ka\n1t\$\n'a=\nq\n"       f.txt        # ...but a copy is not marked
run_pipe "1ka\n1d\n'ap\nq\n"         f.txt        # the line died and the mark with it
run_pipe "1ka\n2ka\n'a=\nq\n"        f.txt        # one line per letter
run_pipe "1ka\n2kb\n'a,'bp\nq\n"     f.txt
run_pipe "1ka\n.=\nq\n"              f.txt        # `k` does not move `.`
run_pipe "1ka\nq\n"                  f.txt        # ...nor modify the buffer
run_pipe "'zp\nq\n"                  f.txt        # never set
run_pipe "'1p\nq\n"                  f.txt        # not a mark name
run_pipe "'Ap\nq\n"                  f.txt
run_pipe "'p\nq\n"                   f.txt
run_pipe '1k\nq\n'                   f.txt        # no name at all
run_pipe '1kA\nq\n'                  f.txt
run_pipe '1k1\nq\n'                  f.txt
run_pipe '1kap\nq\n'                 f.txt        # `p` here is a second name
run_pipe '0ka\nq\n'                  f.txt
run_pipe "1,2ka\n'a=\nq\n"           f.txt        # a range marks its last line

# === comments =================================================================
#
# `#` swallows the rest of the line. It may be addressed — and the address is
# still resolved, so `.` moves and a search that fails is still an error.

run_pipe '#comment\n1p\nq\n'         f.txt
run_pipe '#\nq\n'                    f.txt
run_pipe '2#comment\n.=\nq\n'        f.txt
run_pipe '#p\nq\n'                   f.txt        # the `p` is inside the comment
run_pipe '/zzz/#\nq\n'               f.txt
run_pipe '/beta/#\n.=\nq\n'          f.txt
run_pipe 'g/a/#x\nq\n'               f.txt

# === substitution =============================================================
#
# The mechanics of `s`: delimiters, flags, the unterminated form. What the
# *pattern* means is a separate section further down.

run_pipe '2s/beta/BETA/\n,p\nq\n'    f.txt
run_pipe ',s/a/A/\n,p\nq\n'          f.txt
run_pipe ',s/a/A/g\n,p\nq\n'         f.txt
run_pipe '1s/a/A/p\nq\n'             f.txt
run_pipe '1s/a/A/n\nq\n'             f.txt
run_pipe '1s|a|A|\n,p\nq\n'          f.txt        # any delimiter
run_pipe '1s/zzz/A/\nq\n'            f.txt        # no match
run_pipe ',s/zzz/A/\nq\n'            f.txt
run_pipe '1s/alpha//\n,p\nq\n'       f.txt        # empty replacement
run_pipe '1s/a/A/l\nq\n'             f.txt
run_pipe '1s/a/A/gnl\nq\n'           f.txt
# An unterminated replacement means "print the last line changed" — POSIX's
# "a <newline> may be used instead of the final delimiter".
run_pipe '1s/a/A\n,p\nq\n'           f.txt
run_pipe ',s/a/A\nq\n'               f.txt
run_pipe '1s/zz/A\nq\n'              f.txt        # ...unless nothing matched
run_pipe '1s\nq\n'                   f.txt        # no pattern at all
run_pipe ',s/\xff/Z/\n,p\nq\n'       bytes.txt    # a byte no String can hold

# === regular expressions ======================================================
#
# A pattern is a POSIX *basic* RE through `ere::bre`, or an *extended* one under
# `-E`. Everything in this section was a KBUG under
# `TD-B-ED-HAS-NO-REGULAR-EXPRESSIONS` until the engine was wired in.
#
# Note on writing these: the script goes through `printf %b`, which turns `\1`
# into byte 0x01. A backslash meant for ed must be doubled.

run_pipe '1s/./X/\n,p\nq\n'          f.txt
run_pipe ',s/^/> /\n,p\nq\n'         f.txt
run_pipe ',s/$/./\n,p\nq\n'          f.txt
run_pipe ',s/a*/X/\n,p\nq\n'         f.txt        # an empty match at the front
run_pipe ',s/[aeiou]/./g\n,p\nq\n'   f.txt
run_pipe ',s/[^aeiou]/./g\n,p\nq\n'  f.txt
run_pipe ',s/[/]/X/\n,p\nq\n'        f.txt        # the delimiter inside a bracket
run_pipe ',s/m\\{2\\}/X/\n,p\nq\n'     f.txt        # a BRE interval
run_pipe ',s/\\(a\\)\\(l\\)/\\2\\1/\n,p\nq\n' f.txt  # \1..\9 in the replacement
run_pipe ',s/a/[&]/\n,p\nq\n'        f.txt        # & is the whole match
run_pipe ',s/a/[\\&]/\n,p\nq\n'      f.txt        # ...and \& is a literal one
run_pipe 'a\nbanana\n.\n$s/\\(an\\)\\1/X/\n$p\nq\n' empty.txt   # a backreference
run_pipe '1s/[/X/\nq\n'              f.txt        # an unbalanced bracket
run_pipe '1s/\\(/X/\nq\n'            f.txt        # an unbalanced group
run_pipe '1s/a\\+/X/\n,p\nq\n'       f.txt        # GNU's BRE extension
run_pipe ',s/a+/X/\n,p\nq\n'         f.txt        # ...and a bare + is literal
run_pipe ',s/\\./X/\n,p\nq\n'        f.txt        # an escaped metacharacter
run_pipe ',s/[[:alpha:]]/X/\n,p\nq\n' f.txt       # a character class

# The empty-match rule for `s///g`, which is ed's and not sed's: a match that
# consumes nothing, on any pass after the first, ends the command with
# `Infinite substitution loop` rather than skipping a character. The dividing
# line is whether the *second* search can still match empty where the first one
# did — which is what `^` cannot do, and `a*` can. Both halves are here because
# getting one right by breaking the other is the easy mistake.
run_pipe ',s/a*/X/g\n,p\nq\n'        f.txt        # empty after consuming: refused
run_pipe ',s/b*/X/g\n,p\nq\n'        f.txt        # empty at 0 twice: refused
run_pipe ',s/a*b*/X/g\n,p\nq\n'      f.txt        # ditto, two stars
run_pipe ',s/\\(a\\)*/X/g\n,p\nq\n'    f.txt        # ditto, through a group
run_pipe ',s/^x*/X/g\n,p\nq\n'       f.txt        # ^ cannot match twice: allowed
run_pipe ',s/^a*/X/g\n,p\nq\n'       f.txt        # ...even having consumed
run_pipe ',s/x*$/X/g\n,p\nq\n'       f.txt        # $ likewise
run_pipe ',s/al*/X/g\n,p\nq\n'       f.txt        # always consumes: allowed
run_pipe ',s/a*/X/\n,p\nq\n'         f.txt        # no `g`, so one pass, allowed
run_pipe 'a\n\n.\n$s/x*/X/g\n$p\nq\n' empty.txt   # an empty line substitutes once
# A line the walk already changed keeps its change when a later line raises the
# error — which is also why `q` then refuses.
run_pipe 'a\naaa\nbbb\n.\n,s/a*/X/g\n,p\nq\n' empty.txt
run_pipe 'a\nbbb\naaa\n.\n,s/a*/X/g\n,p\nq\n' empty.txt

# A search address, forwards and backwards, and the empty pattern that repeats
# the last one. The search wraps, so three `/a/` on a three-line file come back
# to where they started.
run_pipe '/beta/p\nq\n'              f.txt
run_pipe '/BETA/p\nq\n'              f.txt        # no match
run_pipe '?beta?p\nq\n'              f.txt
run_pipe '/a/p\n//p\nq\n'            f.txt
run_pipe '/a/p\n??p\nq\n'            f.txt
run_pipe '//p\nq\n'                  f.txt        # nothing searched for yet
run_pipe 's//X/\nq\n'                f.txt
run_pipe '/beta/,/gamma/p\nq\n'      f.txt
run_pipe '/a/\n/a/\n/a/\np\nq\n'     f.txt        # the search wraps
run_pipe '/gamma/+p\nq\n'            f.txt        # past the end after a search
run_pipe '/be/d\n,p\nq\n'            f.txt
run_pipe '/beta\nq\n'                f.txt        # unterminated, still a search

# `-E` selects the extended dialect, where `+`, `?`, `|` and unbackslashed
# parentheses are the metacharacters instead of their backslashed forms.
run_pipe ',s/a+/X/\n,p\nq\n' -E      f.txt
run_pipe ',s/(a)(l)/\\2\\1/\n,p\nq\n' -E f.txt
run_pipe ',s/a|b/X/g\n,p\nq\n' -E    f.txt
run_pipe ',s/al?/X/\n,p\nq\n' -E     f.txt
run_pipe ',s/\\(a\\)/X/\n,p\nq\n' -E   f.txt      # backslashed parens are literal
run_pipe 'g/a|g/p\nq\n' --extended-regexp f.txt
run_null -E
run_null --extended-regexp

# `-G` is GNU's compatibility mode. Of the commands this ed has, the one thing
# it changes is that `l` ends the line without the `$` marker.
run_pipe '1l\nq\n' -G                f.txt
run_pipe ',l\nq\n' -G -s             bytes.txt
run_pipe ',l\nq\n' --traditional -s  blanks.txt
run_pipe '1ll\nq\n' -G               f.txt
run_null -G
run_null --traditional

# === global commands ==========================================================
#
# `g`/`v` run a command list over every (non-)matching line; `G`/`V` ask at the
# terminal for the list instead, and read the answers from the same stdin the
# script arrived on.

run_pipe 'g/a/p\nq\n'                f.txt
run_pipe 'g/a/\nq\n'                 f.txt        # an empty list means p
run_pipe 'g/a/\\\n\nq\n'             f.txt        # ...twice, for two empty ones
run_pipe 'v/a/p\nq\n'                f.txt        # nothing lacks an a
run_pipe 'v/beta/p\nq\n'             f.txt
run_pipe 'g/a/d\n,p\nq\n'            f.txt
run_pipe 'v/beta/d\n,p\nq\n'         f.txt
run_pipe 'g/a/s/a/X/\n,p\nq\n'       f.txt
run_pipe 'g/a/s/a/X/g\n,p\nq\n'      f.txt
run_pipe '2,3g/a/p\nq\n'             f.txt
run_pipe '1,2v/a/p\nq\n'             f.txt
run_pipe 'g/zzz/p\nq\n'              f.txt        # nothing matches
run_pipe 'g/a/=\nq\n'                f.txt
run_pipe 'g/e/n\nq\n'                five.txt
run_pipe 'g/o/l\nq\n'                five.txt
run_pipe 'g/a/g/b/p\nq\n'            f.txt        # nesting is refused
run_pipe 'g\nq\n'                    f.txt        # no delimiter at all
run_pipe 'g/a p\nq\n'                f.txt        # a space is not a delimiter
run_pipe 'g,a,p\nq\n'                f.txt        # any other byte is
run_pipe 'g/a/w other.txt\nq\n'      f.txt        # the list may write
# The list may move a line the global has selected but not yet reached, and the
# selection has to move with it — the loop looks up the next selected line after
# every step, so a selection left behind on a number would run the list on
# whatever line slid into that place. Discriminating: without it this ends with
# the buffer in its original order.
# A line the list *moves* stops being selected, so a `g` whose list moves a
# still-pending line runs fewer times than it has matches. GNU gets this from
# `move_lines` calling `unset_active_nodes`; the first case here runs its list
# twice, not three times, and the third runs it once, not twice. Copies are
# never selected either, but the original keeps its mark — so the second case
# runs twice.
run_pipe 'g/o/3,4m0\n,p\nq\n'        five.txt
run_pipe 'g/o/1,2t$\n,p\nq\n'        five.txt
run_pipe 'g/^\\(one\\|four\\)$/4m0p\n,p\nq\n'  five.txt
run_pipe 'g/^\\(one\\|four\\)$/4t0p\n,p\nq\n'  five.txt
run_pipe 'g/e/j\n,p\nq\n'            five.txt
# The list continues while a line ends in an odd number of backslashes, and the
# newline the backslash hid separates the two commands.
run_pipe 'g/beta/s/e/E/\\\ns/t/T/\n,p\nq\n'   f.txt
# `a` inside a list takes its text from the list and ends when the list does —
# there is no `.` to close it.
run_pipe 'g/beta/a\\\ninserted\n,p\nq\n'      f.txt
run_pipe 'g/a/s/a/X/\\\np\nq\n'               f.txt
# `G` prints each selected line and takes its list from stdin: a reply is a new
# list, an empty reply skips the line, and `&` repeats the last list.
run_pipe 'G/a/\ns/a/X/\n\n&\n,p\nq\n'         f.txt
run_pipe 'V/beta/\np\n,p\nq\n'                f.txt
run_pipe 'G/a/\n\n\n\n,p\nq\n'                f.txt
run_pipe 'G/zzz/\nq\n'                        f.txt

# === file names ===============================================================

run_pipe 'f\nq\n'                    f.txt
run_pipe 'f other.txt\nf\nw\nq\n'    f.txt
run_pipe '1d\nw other.txt\nq\n'      f.txt
run_pipe '1,2w other.txt\nq\n'       five.txt
run_pipe 'w\nq\n'                                 # no name to write to
run_pipe 'w /nonexistent-dir/x\nq\n' f.txt
run_pipe 'w nosuchdir/x\nq\n'        f.txt        # an unwritable path
run_pipe 'f\nq\n' -r                 f.txt        # restricted, plain name
run_pipe 'w sub/x\nq\n' -r           f.txt        # restricted, a separator
# All six file-naming commands demand whitespace after the command letter, and
# say `Unexpected command suffix` — a different sentence from the `Invalid
# command suffix` that a bad *print* suffix gets. So a name can never be run
# straight onto the letter the way `p` runs onto `1`.
run_pipe 'fother.txt\nq\n'           f.txt
run_pipe '1,2wother.txt\nq\n'        f.txt
run_pipe 'f\tother.txt\nf\nq\n'      f.txt        # ...a tab is whitespace
run_pipe 'f  other.txt\nf\nq\n'      f.txt        # ...and so are two blanks

# === reading another file in ==================================================
#
# `r` is the read half of `w`, and the two are not symmetric: `w` defaults to
# the whole buffer and `r` defaults to appending after `$`. It reports the byte
# count the same way a startup read does, appends a missing final newline the
# same way, and — unlike `e` — leaves the remembered file name alone.

run_pipe '$r f.txt\n.=\n,p\nq\n'     five.txt
run_pipe 'r f.txt\n.=\n,p\nq\n'      five.txt     # the default address is `$`
run_pipe '0r f.txt\n.=\n,p\nq\n'     five.txt
run_pipe '2r f.txt\n.=\n,p\nw\nq\n'  five.txt
run_pipe 'r\n.=\n,p\nq\n'            f.txt        # no name: the one we opened
run_pipe 'r nonl.txt\n,p\nw\nq\n'    f.txt        # `Newline appended`
run_pipe 'r empty.txt\n.=\nq\n'      f.txt        # nothing read, nothing changed
run_pipe 'r bytes.txt\nw\nq\n'       f.txt
run_pipe 'r nosuch.txt\nq\n'
run_pipe 'r nosuch.txt\nq\n'         f.txt
run_file 'r nosuch.txt\n,p\nq\n'     f.txt        # file-driven: stops there
run_pipe 'r .\nq\n'                  f.txt        # a directory opens, then will not read
run_pipe 'r f.txt\nf\nq\n'           five.txt     # `r` does not rename the buffer
run_pipe 'r f.txt\nw\nq\n' -s        five.txt     # -s drops the count
run_pipe 'r f.txt\nq\n' -r           five.txt     # restricted, a plain name
run_pipe 'r sub/x\nq\n' -r           five.txt
run_pipe '$rf.txt\n,p\nq\n'          five.txt     # ...the space is *not* optional
run_pipe '$r\tf.txt\n,p\nq\n'        five.txt     # ...but a tab will do
run_pipe 'r\nq\n'                                 # no name anywhere

# === editing another file =====================================================
#
# `e` is `r` over the top of everything: it empties the buffer *first*, so a
# read that fails leaves nothing behind. It refuses a modified buffer once —
# and it shares that one refusal with `q`, so a `q` that has already warned
# lets the next `e` straight through.

run_pipe 'e five.txt\n.=\n,p\nq\n'   f.txt
run_pipe 'e five.txt\nf\nq\n'        f.txt        # ...and this one does rename
run_pipe 'e five.txt\nw\nq\n'        f.txt        # so `w` writes five.txt
run_pipe 'e nosuch.txt\n,p\n.=\nq\n' f.txt        # emptied, then nothing to read
run_pipe 'e .\n,p\nq\n'              f.txt
run_pipe 'e empty.txt\n,p\nq\n'      f.txt
run_pipe 'e nonl.txt\n,p\nw\nq\n'    f.txt
run_pipe 'e bytes.txt\nw\nq\n'       f.txt
run_pipe 'e\n,p\nq\n'                f.txt        # no name: reread this one
run_pipe '1d\ne\n,p\nq\n'            f.txt        # ...which is how you revert
run_pipe '1d\ne five.txt\nq\n'       f.txt        # modified: refused once
run_pipe '1d\ne five.txt\ne five.txt\n,p\nq\n' f.txt
run_pipe '1d\nq\ne five.txt\n,p\nq\n' f.txt       # `q` warned, so `e` goes
run_pipe '1d\nq\n1p\ne five.txt\nq\n' f.txt       # ...but only straight after
run_pipe '1d\nq\ne nosuch.txt\nq\n'  f.txt        # a failure for another reason
run_pipe '1d\ne nosuch.txt\ne nosuch.txt\nq\n' f.txt
run_pipe '1d\nE five.txt\n,p\nq\n'   f.txt        # E never warns
run_pipe 'E nosuch.txt\n,p\nq\n'     f.txt
run_pipe 'e five.txt\nq\n' -s        f.txt
run_pipe 'e five.txt\nq\n' -r        f.txt        # restricted refuses the name
run_pipe 'e sub/x\nq\n' -r           f.txt
run_pipe 'efive.txt\n,p\nq\n'        f.txt
run_pipe 'e five.txt p\nq\n'         f.txt        # a name, not a suffix
run_file 'e nosuch.txt\n,p\nq\n'     f.txt

# === undo =====================================================================
#
# One level deep, and `u` after `u` redoes — so the record is a swap, not a
# stack. What clears it is as much of the behaviour as what fills it: a command
# that starts modifying the buffer and then changes nothing leaves *nothing* to
# undo rather than leaving the previous command's record in place.

run_pipe '1d\nu\n.=\n,p\nq\n'        f.txt
run_pipe '1d\nu\nu\n,p\nq\n'         f.txt        # a second `u` redoes
run_pipe '1d\nu\nu\nu\n,p\nq\n'      f.txt
run_pipe 'u\nq\n'                    f.txt        # nothing has changed yet
run_pipe '1p\nu\nq\n'                f.txt        # ...and `p` did not change it
run_pipe 'a\nX\n.\nu\n,p\nq\n'       f.txt
run_pipe '0i\nX\n.\nu\n,p\nq\n'      f.txt
run_pipe '1c\nX\n.\nu\n,p\nq\n'      f.txt
run_pipe ',d\nu\n,p\nq\n'            f.txt
run_pipe '1s/a/X/\nu\n,p\nq\n'       f.txt
run_pipe '1,2j\nu\n,p\nq\n'          f.txt
run_pipe '1m$\nu\n.=\n,p\nq\n'       f.txt
run_pipe '1t$\nu\n,p\nq\n'           f.txt
run_pipe '$r f.txt\nu\n,p\nq\n'      five.txt
run_pipe 'g/a/d\nu\n,p\nq\n'         f.txt        # a whole global is one undo
run_pipe 'g/a/s/a/X/\nu\n,p\nq\n'    f.txt
# A global clears the record the moment it starts, so a global that changes
# nothing leaves `u` with *nothing to do* rather than a no-op to do — even a
# `v` that selects no line at all.
run_pipe '1d\ng/beta/p\nu\n,p\nq\n'  f.txt
run_pipe '1d\nv/zzz/p\nu\n,p\nq\n'   f.txt
run_pipe '2\ng/a/d\nu\n.=\n,p\nq\n'  f.txt        # `u` puts `.` back too...
run_pipe '1\ng/a/d\nu\n.=\n,p\nq\n'  f.txt        # ...to before the `g`, not
run_pipe '2\n4d\nu\n.=\nu\n.=\nq\n'  five.txt     # ...the first selected line
run_pipe '1d\nr empty.txt\nu\n,p\nq\n' f.txt      # nor did that read
run_pipe '1d\n1s/zzz/X/\nu\n,p\nq\n' f.txt        # nor that failed substitution
run_pipe '1d\ne five.txt\nu\n,p\nq\n' f.txt       # `e` is not undoable
run_pipe "1ka\n1d\nu\n'ap\nq\n"      f.txt        # the marks come back too
run_pipe '1d\nw\nu\nq\n'             f.txt        # ...and so does `modified`
run_pipe '1d\nup\nq\n'               f.txt        # `u` takes a print suffix
run_pipe '1d\nun\nq\n'               f.txt
run_pipe '1d\n1u\nq\n'               f.txt        # ...but no address
run_pipe '1d\nux\nq\n'

# === quitting =================================================================

run_pipe 'q\n'                       f.txt
run_pipe '1d\nq\n'                   f.txt        # modified: warn, then...
run_pipe '1d\nq\nq\n'                f.txt        # ...the second q goes
run_pipe '1d\nQ\n'                   f.txt        # Q never warns
run_pipe '1d\n'                      f.txt        # EOF on a modified buffer: 2
run_pipe '1d\nw\nq\n'                f.txt        # written, so no warning
run_pipe '1d\nq\n' -l                f.txt        # -l forces status 0
run_pipe '9p\nq\n' -l                f.txt
# What retracts the warning, which is not what anyone would guess. A command
# that merely *looks* does not: `1p` leaves the refusal standing and the next
# `q` goes. A command that *changes* the buffer does, because the warning was
# about work that has since been added to. And so does a command that *fails*.
run_pipe '1d\nq\n1p\nq\nq\n'         f.txt
run_pipe '1d\nq\n=\nq\n'             f.txt
run_pipe '1d\nq\nf other\nq\n'       f.txt
run_pipe '1d\nq\nw other.txt\nq\n'   f.txt
run_pipe '1d\nq\n1ka\nq\n'           f.txt        # `k` changes nothing
run_pipe '1d\nq\n#c\nq\n'            f.txt
run_pipe '1d\nq\n1d\nq\n'            f.txt        # a change: warns again
run_pipe '1d\nq\n1s/beta/X/\nq\n'    f.txt
run_pipe '1d\nq\n1,2j\nq\n'          f.txt
run_pipe '1d\nq\n1m$\nq\n'           f.txt
run_pipe '1d\nq\nr f.txt\nq\n'       f.txt
run_pipe '1d\nq\nu\nu\nq\n'          f.txt        # ...and a redo is a change
run_pipe '1d\nq\nu\nq\n'             f.txt        # ...while an undo unmodifies
run_pipe '1d\nq\nzzz\nq\n'           f.txt        # a failure: warns again
run_pipe '1d\nq\n9p\nq\n'            f.txt
run_pipe '1d\nq\n/zzz/\nq\n'         f.txt
run_pipe '1d\nq\n1s/zzz/X/\nq\n'     f.txt
# The end-of-input warning asks a *different* question — was the command just
# before the end the refusal — so these three exit 1, 2 and 2.
run_pipe '1d\nq\n'                   f.txt
run_pipe '1d\nq\n1p\n'               f.txt
run_pipe '1d\nq\n1d\n'               f.txt
kbug_pipe TD-B-ED-IS-MISSING-SEVEN-MORE-COMMANDS '1d\nq\nH\n'  f.txt
run_pipe '1d\ne five.txt\n'          f.txt        # `e`'s refusal counts too
run_pipe '1d\nu\nq\n'                f.txt        # undone: nothing to warn about
run_pipe '1d\nw\nu\nq\n'             f.txt        # ...even back across a `w`

# === the two stdin kinds disagree, on purpose =================================

run_file '9p\n1p\nq\n'               f.txt        # stops at the first error
run_pipe '9p\n1p\nq\n'               f.txt        # carries on
run_file '9p\n1p\nq\n' -v            f.txt        # `script, line 1: `
run_pipe '9p\n1p\nq\n' -v            f.txt        # a bare sentence
run_file 'Z\nq\n' -v                 f.txt
run_pipe 'Z\nq\n' -v                 f.txt
run_file '1s/zzz/A/\nq\n' -v         f.txt
run_pipe '1s/zzz/A/\nq\n' -v         f.txt
# The status is sticky: three failures then a success still exits 1.
run_pipe '9p\n8p\n7p\n1p\nq\n'       f.txt

# === options ==================================================================

run_pipe 'q\n' -p '*'                f.txt
run_pipe '1d\nq\nq\n' -p '*'         f.txt
run_pipe 'q\n' --prompt='> '         f.txt
run_pipe 'q\n' -s -p '*'             f.txt
run_pipe 'q\n' --quiet               f.txt
run_pipe 'q\n' --silent              f.txt
run_pipe 'q\n' --script              f.txt
run_pipe 'q\n' --verbose             f.txt
run_pipe 'q\n' --loose-exit-status   f.txt
run_pipe 'q\n' -s f.txt extra.txt                 # first operand wins
run_pipe 'q\n' f.txt -s                           # options may follow it
usage_case -Z
usage_case --nosuch
usage_case -p                                     # -p wants a value
usage_case --prompt
usage_case --quie                                 # an unambiguous abbreviation
usage_case --help=x
usage_case --version=x

# --- deliberate differences ---------------------------------------------------
#
# `--help` and `--version` reach the option table like any other long option,
# so a value attached to one is a table question and is compared above. What
# they *print* is not: ours names SlateOS, omits the GNU project's
# `Report bugs to:` block, and does not open with GNU's four-paragraph essay on
# what a line editor is.
xfail_case 'help is ours: no GNU bug-report block, no prose preamble' --help
xfail_case 'version names SlateOS' --version
xfail_case 'help is ours' -h
xfail_case 'version names SlateOS' -V
# GNU ed hand-rolls its own `arg_parser` rather than using gnulib's
# `getopt_long`, so an ambiguous long option gets one sentence and no list.
# `coreutils::getopt` lists the candidates, which is the more useful answer and
# is what every other utility here prints; matching ed's would mean a second
# option parser in the tree for the sake of a worse message.
xfail_case 'getopt lists the ambiguity candidates and ed does not' --s
# `!CMD`, and a file name beginning with `!`, hand the line to a shell. That is
# a deliberate omission rather than a missing feature — see the module docs and
# `design-decisions.md` §713 — so all three answer `Shell access not implemented
# by this ed`. Taking `!cmd` as a literal file name would write to a file the
# user did not ask for, which is why refusing is the honest answer.
xfail_pipe 'a name starting with ! is a shell command we do not run' 'w !cmd\nq\n' f.txt
xfail_pipe 'a name starting with ! is a shell command we do not run' 'r !cmd\nq\n' f.txt
xfail_pipe '! runs a shell command and we do not have a shell' '!echo hi\nq\n' f.txt

# --- known bugs ---------------------------------------------------------------
#
# None. The eight commands that used to live here — `m`, `t`, `j`, `k`/`'x`,
# `r`, `e`/`E`, `u` and `#` — are implemented, and their cases have moved up
# into the sections above as ordinary `run_pipe`s.

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$kbug" -gt 0 ]; then
  printf ', %d known bug(s)' "$kbug"
fi
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER DIFFER (see above)' "$xpass"
fi
if [ "$kfixed" -gt 0 ]; then
  printf ', %d known bug(s) FIXED (see above)' "$kfixed"
fi
printf '\n'
# An xpass or a kfixed fails the run: an xfail that has started agreeing is a
# reason in this file that is no longer true, and a known bug that has started
# agreeing is an entry in `known-issues.md` describing a defect that is gone.
# A stale reason is worse than none. A live KBUG does *not* fail the run — it
# is loud on every run instead, because a harness that is permanently red is a
# harness nobody reads, which is how bc's `quit` stayed broken for months.
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ] && [ "$kfixed" -eq 0 ]
