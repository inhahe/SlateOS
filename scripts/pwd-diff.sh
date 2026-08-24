#!/bin/bash
# pwd-diff.sh — run our `pwd` and GNU coreutils' `pwd` side by side and report
# every case where they disagree.
#
# `pwd` is the utility with the largest gap between how simple it looks and how
# much of it is decided elsewhere. It takes no operands, reads no files, and
# prints one line — and yet the line it prints depends on an environment
# variable, on whether two `stat` calls agree, on `POSIXLY_CORRECT`, and, when
# `getcwd(3)` fails, on a hand-written walk up the tree comparing i-nodes. Our
# version had none of that until 2026-08-24: it was `println!("{}", cwd)`.
#
# ## How it runs
#
# Both sides are invoked through a symlink named `pwd` on a one-entry `PATH`,
# so `argv[0]` is the bare word on both — which matters because every
# diagnostic here is prefixed with it and `--help` prints it in the usage line.
# Our binary is built for Linux inside WSL rather than for Windows
# (`design-decisions.md` §374); the shared preamble does all of that.
#
# The interesting knob is `SETUP`: a fragment of shell run inside each side's
# subshell before `pwd`, which is how a case chooses the working directory it
# is asking about. It runs once per side, so a case may destroy what it made
# (see the deleted-directory cases) without spoiling the other side's turn.
#
# ## What each group is for
#
#   * **logical vs physical** — the whole point of `-L`/`-P`. A shell that
#     reached here through a symlink exports a `PWD` naming the symlink;
#     `getcwd` names the target. Which of the two is printed is the only thing
#     `pwd` really decides.
#   * **when `$PWD` is not usable** — gnulib's `logical_getcwd` rejects a `PWD`
#     that is relative, that contains a `.` or `..` component, or that names a
#     different directory than `.` does, and falls through to the physical
#     answer without a word. The delicate one is the dotfile: `/a/.config`
#     contains the two characters `/.` and is *fine*, because upstream looks at
#     the character after the dot. A `split('/')` implementation gets that
#     right by accident and gets `PWD=/a/b/` (accepted, trailing slash and all)
#     wrong.
#   * **`POSIXLY_CORRECT`** — the only thing that changes the default from
#     `-P` to `-L`.
#   * **operands** — ignored, but not silently: `pwd foo` warns once on stderr
#     and still exits 0.
#   * **getcwd failing** — the only route to `robust_getcwd`, and the reason it
#     is transcribed at all. A working directory that has been `rmdir`ed makes
#     `getcwd` fail with `ENOENT`, and GNU answers with
#     `couldn't find directory entry in '..' with matching i-node` rather than
#     the raw errno.
#   * **the write errors** — `>&-` and `>/dev/full`, the sweep this file was
#     written for. Rust reopens a closed descriptor on /dev/null before `main`
#     and then reports writes to a closed one as successes, so both of these
#     were a silent exit 0 before `coreutils::stdfd`.
#
# ## Cases that differ on purpose
#
# Two, both recorded as `xfail`: `--help` omits the GNU project's ancillary
# block, as every converted utility here does, and `--version` names SlateOS.
#
# Run `OURS=/usr/bin/pwd ./scripts/pwd-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

DIFF_PROG=pwd
# `command -v pwd` finds the shell builtin, which is not what is being compared.
DIFF_REF="/usr/bin/pwd /bin/pwd"
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# --- fixtures ----------------------------------------------------------------
# `real` with a symlink to it is the logical/physical case; `dot/.config` is the
# dotfile that must *not* be mistaken for a `.` component; the `\xff` name is
# there because a directory name is an arbitrary byte string and rendering the
# answer as text would corrupt it.
mkdir -p "$DIFF_TMP/plain" "$DIFF_TMP/real" "$DIFF_TMP/other" \
         "$DIFF_TMP/deep/a/b/c" "$DIFF_TMP/dot/.config"
ln -s "$DIFF_TMP/real" "$DIFF_TMP/link"
nut=$DIFF_TMP/$'na\xffme'
mkdir -p "$nut"

# --- knobs, reset before every case ------------------------------------------
# `SETUP` is shell run inside the per-side subshell, before `pwd`; it decides
# the working directory. `ENVV` is a word list handed to `env`, so it can hold
# both assignments (`PWD=...`) and `env`'s own flags (`-u PWD`). `TO_FULL` and
# `CLOSED` are the two ways to reach a write error — they are not
# interchangeable: /dev/full fails every write, whereas a closed descriptor
# only fails one that was actually attempted.
SETUP=; ENVV=; TO_FULL=; CLOSED=

reset_knobs() { SETUP=; ENVV=; TO_FULL=; CLOSED=; }

# Bytes, not text: a directory name may hold anything but `/` and NUL, and the
# case whose whole point is `\xff` must not be compared through something that
# would normalise it.
render() {
  local f=$1 sz
  sz=$(stat -c %s "$f" 2>/dev/null) || { printf '<unstattable>\n'; return 0; }
  printf '%s bytes\n' "$sz"
  od -An -c <"$f"
}

# --- run one case on both sides ----------------------------------------------
compare() {
  local o_bin g_bin o_err g_err o_rc g_rc
  o_bin=$(mktemp); g_bin=$(mktemp); o_err=$(mktemp); g_err=$(mktemp)

  local side out err rc setup
  setup=$SETUP
  [ -n "$setup" ] || setup="cd '$DIFF_TMP/plain'"
  for side in ours gnu; do
    if [ "$side" = ours ]; then out=$o_bin; err=$o_err
    else out=$g_bin; err=$g_err; fi
    # `$ENVV` is deliberately unquoted: it is a word list, and its values are
    # paths under a `mktemp -d` directory, which never contain spaces.
    if [ -n "$CLOSED" ]; then
      # shellcheck disable=SC2086
      ( eval "$setup" >/dev/null 2>&1 || exit 127
        timeout -k 2 60 env $ENVV PATH="$bindir/$side" pwd "$@" ) >&- 2>"$err"
    elif [ -n "$TO_FULL" ]; then
      # shellcheck disable=SC2086
      ( eval "$setup" >/dev/null 2>&1 || exit 127
        timeout -k 2 60 env $ENVV PATH="$bindir/$side" pwd "$@" ) >/dev/full 2>"$err"
    else
      # shellcheck disable=SC2086
      ( eval "$setup" >/dev/null 2>&1 || exit 127
        timeout -k 2 60 env $ENVV PATH="$bindir/$side" pwd "$@" ) >"$out" 2>"$err"
    fi
    # On the very next line, before anything else runs — including a `[ ]`
    # test, whose own status would silently replace it. See tee-diff.sh, where
    # getting this wrong cost a full run.
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done

  local o_out g_out o_msg g_msg
  o_out=$(render "$o_bin"); g_out=$(render "$g_bin")
  o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  rm -f "$o_bin" "$g_bin" "$o_err" "$g_err"

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}\n  gnu  (rc=%s): out{%s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
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

# The label is built before `compare` runs, because `compare` resets the knobs.
label_of() {
  printf 'pwd %s%s%s%s%s' "$*" \
    "${SETUP:+  [$SETUP]}" "${ENVV:+  [$ENVV]}" \
    "${TO_FULL:+  [>/dev/full]}" "${CLOSED:+  [>&-]}"
}
run_case() { local label; label=$(label_of "$@"); compare "$@"; report "$label"; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  return 0
}

# =============================================================================
# --- the basics --------------------------------------------------------------
run_case
run_case -L
run_case -P
run_case --logical
run_case --physical
SETUP="cd \"$DIFF_TMP/deep/a/b/c\""; run_case
SETUP='cd /'; run_case
SETUP='cd /'; run_case -L
SETUP='cd /tmp'; run_case

# A directory name is an arbitrary byte string; the answer must be those bytes.
SETUP="cd \"$nut\""; run_case
SETUP="cd \"$nut\""; run_case -L
SETUP="cd \"$nut\""; run_case -P

# --- logical vs physical -----------------------------------------------------
# `cd link` is bash's logical cd, so `$PWD` names the symlink and `getcwd`
# names its target. This is the difference the two options exist for.
SETUP="cd \"$DIFF_TMP/link\""; run_case
SETUP="cd \"$DIFF_TMP/link\""; run_case -L
SETUP="cd \"$DIFF_TMP/link\""; run_case -P
SETUP="cd \"$DIFF_TMP/link\""; run_case --logical
SETUP="cd \"$DIFF_TMP/link\""; run_case --physical
# `cd -P` exports the resolved name, so both options now agree.
SETUP="cd -P \"$DIFF_TMP/link\""; run_case -L
SETUP="cd -P \"$DIFF_TMP/link\""; run_case -P

# Neither flag wins by rank: the last one on the line wins.
SETUP="cd \"$DIFF_TMP/link\""; run_case -L -P
SETUP="cd \"$DIFF_TMP/link\""; run_case -P -L
SETUP="cd \"$DIFF_TMP/link\""; run_case -LP
SETUP="cd \"$DIFF_TMP/link\""; run_case -PL
SETUP="cd \"$DIFF_TMP/link\""; run_case --physical --logical
SETUP="cd \"$DIFF_TMP/link\""; run_case --logical --physical
SETUP="cd \"$DIFF_TMP/link\""; run_case -L -L
SETUP="cd \"$DIFF_TMP/link\""; run_case -P -P

# Abbreviated long options: `--l` and `--p` are each unambiguous.
SETUP="cd \"$DIFF_TMP/link\""; run_case --l
SETUP="cd \"$DIFF_TMP/link\""; run_case --p
SETUP="cd \"$DIFF_TMP/link\""; run_case --log
SETUP="cd \"$DIFF_TMP/link\""; run_case --phys

# --- when $PWD is not usable -------------------------------------------------
# Each of these names *this* directory, so the i-node test would pass; the
# textual test is the only thing that can reject them, and when it does the
# answer silently becomes the physical one.
ENVV="PWD=$DIFF_TMP/plain/."; run_case -L
ENVV="PWD=$DIFF_TMP/./plain"; run_case -L
ENVV="PWD=$DIFF_TMP/plain/../plain"; run_case -L
ENVV="PWD=$DIFF_TMP/plain/.."; SETUP="cd \"$DIFF_TMP\""; run_case -L
# Not absolute.
ENVV='PWD=plain'; run_case -L
ENVV='PWD='; run_case -L
ENVV='PWD=.'; run_case -L
# Absent altogether.
ENVV='-u PWD'; run_case -L
# Absolute and traversal-free, but naming something else — or nothing.
ENVV="PWD=$DIFF_TMP/other"; run_case -L
ENVV='PWD=/nonexistent-for-sure'; run_case -L
ENVV='PWD=/'; run_case -L
# Accepted: a trailing slash is not a component, so this is printed verbatim,
# trailing slash and all.
ENVV="PWD=$DIFF_TMP/plain/"; run_case -L
# Accepted: `/.config` contains `/.` and is a perfectly ordinary name. This is
# the case that separates a transcription of upstream from a `split('/')`.
SETUP="cd \"$DIFF_TMP/dot/.config\""; run_case -L
ENVV="PWD=$DIFF_TMP/dot/.config"; SETUP="cd \"$DIFF_TMP/dot/.config\""; run_case -L
# `..b` is a name too.
ENVV="PWD=$DIFF_TMP/plain/..b"; run_case -L
# And none of this applies to `-P`, which never reads the variable.
ENVV="PWD=$DIFF_TMP/other"; run_case -P
ENVV='-u PWD'; run_case -P

# --- POSIXLY_CORRECT ---------------------------------------------------------
# The one thing it decides: the default becomes `-L`.
ENVV="POSIXLY_CORRECT=1"; SETUP="cd \"$DIFF_TMP/link\""; run_case
ENVV="POSIXLY_CORRECT=1"; SETUP="cd \"$DIFF_TMP/link\""; run_case -P
ENVV="POSIXLY_CORRECT=1"; SETUP="cd \"$DIFF_TMP/link\""; run_case -L
ENVV="POSIXLY_CORRECT="; SETUP="cd \"$DIFF_TMP/link\""; run_case
# Set, but with a `$PWD` that is not usable: back to physical.
ENVV="POSIXLY_CORRECT=1 PWD=$DIFF_TMP/other"; SETUP="cd \"$DIFF_TMP/link\""; run_case

# --- operands ----------------------------------------------------------------
# Warned about once, on stderr, and then ignored; the status is still 0.
run_case foo
run_case a b
run_case a b c
run_case ''
run_case -
run_case -- -L
run_case -- --help
SETUP="cd \"$DIFF_TMP/link\""; run_case -L foo
# No leading `+` in upstream's getopt string, so an option after an operand is
# still an option.
SETUP="cd \"$DIFF_TMP/link\""; run_case foo -L
run_case foo --help

# --- the option errors -------------------------------------------------------
run_case -x
run_case -Lx
run_case --nope
run_case --logical=3
run_case --physical=yes
run_case --help=1
# Ambiguous between nothing: every long option here has a distinct first letter.
run_case ---logical

# --- help and version --------------------------------------------------------
xfail_case 'our --help omits the GNU project ancillary block' --help
xfail_case 'our --version names SlateOS' --version
# They win over an operand rather than being one, and over each other by order.
xfail_case 'our --help omits the GNU project ancillary block' -L --help
xfail_case 'our --version names SlateOS' --version --help

# --- getcwd failing ----------------------------------------------------------
# The only route into `robust_getcwd`. A working directory that has been
# removed makes `getcwd` fail with ENOENT, and the answer is the walk's own
# diagnostic rather than the errno.
SETUP="mkdir -p \"$DIFF_TMP/gone\" && cd \"$DIFF_TMP/gone\" && rmdir \"$DIFF_TMP/gone\""
run_case
SETUP="mkdir -p \"$DIFF_TMP/gone\" && cd \"$DIFF_TMP/gone\" && rmdir \"$DIFF_TMP/gone\""
run_case -P
# `-L` reaches it too: `$PWD` still names the removed directory, whose i-node
# test cannot pass, so it falls through to the physical answer and fails there.
SETUP="mkdir -p \"$DIFF_TMP/gone\" && cd \"$DIFF_TMP/gone\" && rmdir \"$DIFF_TMP/gone\""
run_case -L
# One level deeper, so the walk has a component to prepend before it fails.
SETUP="mkdir -p \"$DIFF_TMP/g2/inner\" && cd \"$DIFF_TMP/g2/inner\" && rmdir \"$DIFF_TMP/g2/inner\""
run_case
# An unsearchable parent, which is the walk's `cannot open directory '..'`
# branch. (As root there is no such thing, and then both sides simply succeed —
# the comparison still holds, it just stops testing this branch.)
SETUP="mkdir -p \"$DIFF_TMP/np/in\" && cd \"$DIFF_TMP/np/in\" && chmod 000 \"$DIFF_TMP/np\""
run_case
chmod -R u+rwx "$DIFF_TMP/np" 2>/dev/null

# --- standard output closed, and full ----------------------------------------
# Both were a silent exit 0 before `coreutils::stdfd`.
CLOSED=1; run_case
CLOSED=1; run_case -L
CLOSED=1; run_case -P
CLOSED=1; SETUP="cd \"$DIFF_TMP/link\""; run_case -L
CLOSED=1; run_case --help
CLOSED=1; run_case --version
# The operand warning goes to stderr, so it survives; the answer does not, and
# the write error outranks the exit 0 the warning would have left behind.
CLOSED=1; run_case foo
# A usage error never gets as far as writing anything, so a closed descriptor
# makes no difference to it.
CLOSED=1; run_case -x
CLOSED=1; run_case --logical=3
# ... and neither does a run that was going to fail before it printed.
CLOSED=1; SETUP="mkdir -p \"$DIFF_TMP/gone\" && cd \"$DIFF_TMP/gone\" && rmdir \"$DIFF_TMP/gone\""
run_case

TO_FULL=1; run_case
TO_FULL=1; run_case -L
TO_FULL=1; run_case --help
TO_FULL=1; run_case --version
TO_FULL=1; run_case foo
TO_FULL=1; run_case -x

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
exit 0
