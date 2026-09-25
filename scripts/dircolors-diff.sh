#!/usr/bin/env bash
# dircolors-diff.sh — compare our `dircolors` against GNU's, inside WSL.
#
# ## What this is checking
#
# `dircolors`' output is a function of its database and three environment
# variables, so every case sets `SHELL`, `TERM` and `COLORTERM` explicitly
# (or unsets them) rather than inheriting the caller's:
#
#   * **the built-in database** -- `-p` byte for byte, and the `LS_COLORS` it
#     yields for several terminals, in both shells' syntax and as
#     `--print-ls-colors`;
#   * **the shell guess** -- `csh`/`tcsh` by last component, Bourne otherwise,
#     and the refusal when `SHELL` is unset or empty;
#   * **a database FILE** -- sections opened by `TERM`/`COLORTERM` globs, the
#     entries before any of them, `.ext` and `*glob` keys, ignored keywords,
#     case, comments, CRLF, a missing final newline, a NUL inside a line, the
#     quoting of `'`, `:`, `=`, `\` and `^`, and both kinds of error (a line
#     with no argument anywhere; an unknown keyword only where it applies);
#   * **reading** -- `-` for stdin, a missing file, a directory, an unreadable
#     file;
#   * **the command line** -- the three conflicts, the operand counts,
#     prefixes, and a write error.
#
# ## Cases that differ on purpose
#
# `--help` omits the GNU ancillary block and `--version` names SlateOS.
set -u

DIFF_PROG='dircolors'
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0
TO_FULL=
# The environment one case runs with; reset after each.
CASE_ENV=(SHELL=/bin/bash TERM=xterm-256color)
INPUT=/dev/null

fx=$DIFF_TMP/fx
mkdir -p "$fx" || exit 1
( cd "$fx" &&
  printf 'DIR 01;34\n.tar 01;31\n*~ 00;90\n' > global &&
  printf 'TERM xterm*\nTERM linux\nDIR 01;34\nTERM dumb\nFILE 00\nCOLORTERM ?*\nLINK 01;36\n' > sections &&
  printf 'TERM *\nBOGUS 1\nDIR 01\n' > bogus-applies &&
  printf 'BOGUS 1\nTERM nomatch\nBOGUS 2\nTERM *\nDIR 01\n' > bogus-silent &&
  printf '# comment\n\nDIR\nFILE 00\n' > missing-arg &&
  printf 'dir 1\nSetuid 2\nCOLOR all\nOPTIONS -F\nEIGHTBIT 1\nnorm 3\nclrtoeol 4\n' > case &&
  printf 'DIR 01;34\r\n.tar 01;31  # tar\r\n' > crlf &&
  printf 'DIR 01;34\n.tar 01;31' > no-final-newline &&
  printf 'DIR 01\000;34\n.gz 01;31\n' > nul &&
  printf "*a'b 01\n*c:d 02\n*e=f 03\n*g\\\\:h 04\n*i^=j 05\n" > quoting &&
  printf '  \t DIR   01;34   # trailing\n\t.x\t02\n' > spacing &&
  printf 'x\n' > unreadable && chmod 000 unreadable &&
  mkdir dir ) || { echo "dircolors-diff: could not build the fixtures" >&2; exit 1; }

compare() {
  local side out err rc o_out g_out o_err g_err o_rc g_rc
  for side in ours gnu; do
    out=$DIFF_TMP/out-$side; err=$DIFF_TMP/err-$side
    if [ -n "$TO_FULL" ]; then
      ( cd "$fx" && timeout -k 2 30 env -i LC_ALL=C.UTF-8 "${CASE_ENV[@]}" PATH="$bindir/$side" \
          dircolors "$@" ) <"$INPUT" >/dev/full 2>"$err"
    else
      ( cd "$fx" && timeout -k 2 30 env -i LC_ALL=C.UTF-8 "${CASE_ENV[@]}" PATH="$bindir/$side" \
          dircolors "$@" ) <"$INPUT" >"$out" 2>"$err"
    fi
    rc=$?
    : >>"$out"
    if [ "$side" = ours ]; then
      o_rc=$rc; o_out=$(od -An -c <"$out"); o_err=$(cat "$err")
    else
      g_rc=$rc; g_out=$(od -An -c <"$out"); g_err=$(cat "$err")
    fi
    : >"$out"
  done
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%.300s} err{%s}\n  gnu  (rc=%s): out{%.300s} err{%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')")
}

label_of() {
  printf '[%s] dircolors %s%s' "${CASE_ENV[*]}" "$*" "${TO_FULL:+  [>/dev/full]}"
}

reset_case() {
  TO_FULL=; CASE_ENV=(SHELL=/bin/bash TERM=xterm-256color); INPUT=/dev/null
}

run_case() {
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$label" "$REPORT"
  fi
  reset_case
  return 0
}

xfail_case() {
  local why=$1; shift
  local label; label=$(label_of "$@")
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$label" "$why"
  fi
  reset_case
  return 0
}

# --- the built-in database ------------------------------------------------------------
run_case -p
run_case --print-database
run_case
run_case -b
run_case -c
run_case --sh
run_case --bourne-shell
run_case --csh
run_case --c-shell
run_case --print-ls-colors
for t in linux dumb vt100 screen-256color xterm-kitty rxvt-unicode ansi putty tmux-256color con132x25 eterm-color; do
  CASE_ENV=(SHELL=/bin/bash "TERM=$t"); run_case
done
CASE_ENV=(SHELL=/bin/bash TERM=dumb COLORTERM=truecolor); run_case
CASE_ENV=(SHELL=/bin/bash TERM=dumb COLORTERM=); run_case
CASE_ENV=(SHELL=/bin/bash); run_case
CASE_ENV=(SHELL=/bin/bash TERM=); run_case

# --- the shell guess --------------------------------------------------------------------
CASE_ENV=(SHELL=/bin/tcsh TERM=linux); run_case
CASE_ENV=(SHELL=csh TERM=linux); run_case
CASE_ENV=(SHELL=/usr/bin/csh/ TERM=linux); run_case
CASE_ENV=(SHELL=/bin/cshx TERM=linux); run_case
CASE_ENV=(SHELL=/bin/zsh TERM=linux); run_case
CASE_ENV=(SHELL= TERM=linux); run_case
CASE_ENV=(TERM=linux); run_case
CASE_ENV=(TERM=linux); run_case -b
CASE_ENV=(TERM=linux); run_case --print-ls-colors
CASE_ENV=(SHELL=/bin/tcsh TERM=linux); run_case -b

# --- a database FILE ----------------------------------------------------------------------
run_case global
CASE_ENV=(SHELL=/bin/bash TERM=xterm-256color); run_case sections
CASE_ENV=(SHELL=/bin/bash TERM=linux); run_case sections
CASE_ENV=(SHELL=/bin/bash TERM=dumb); run_case sections
CASE_ENV=(SHELL=/bin/bash TERM=vt100); run_case sections
CASE_ENV=(SHELL=/bin/bash TERM=vt100 COLORTERM=truecolor); run_case sections
run_case bogus-applies
run_case bogus-silent
run_case missing-arg
run_case case
run_case crlf
run_case no-final-newline
run_case nul
run_case quoting
run_case -c quoting
run_case --print-ls-colors quoting
run_case spacing
run_case --print-ls-colors sections
INPUT=$fx/global; run_case -
INPUT=$fx/missing-arg; run_case -

# --- reading ------------------------------------------------------------------------------
run_case missing
run_case dir
run_case unreadable
run_case ''

# --- the command line ---------------------------------------------------------------------
run_case -p -b
run_case -b -p
run_case --print-ls-colors -c
run_case -p --print-ls-colors
run_case -p global
run_case global sections
run_case -b global sections
run_case -x
run_case --nope
run_case --p
run_case --print
run_case --c
run_case --s
run_case --b
run_case --bourne=1
run_case -- global
xfail_case 'our --help omits the GNU ancillary block' --help
xfail_case 'our --version names SlateOS' --version
xfail_case 'our --help omits the GNU ancillary block' global --help

# --- write errors ---------------------------------------------------------------------------
TO_FULL=1; run_case
TO_FULL=1; run_case -p

chmod -R u+rwx "$fx" 2>/dev/null

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)\n' "$xpass"
  exit 1
fi
printf '\n'
[ "$fail" -eq 0 ]
