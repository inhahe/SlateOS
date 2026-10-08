#!/usr/bin/env bash
# Differential test: our `look` against util-linux 2.39.3's.
#
# The reference is Ubuntu's build, in `bsdextrautils`.
#
# What is compared: stdout, stderr and the exit status of each case.
#
#   * word lists sorted as a dictionary and as bytes, searched with and
#     without -d and -f -- and the defaults, which are -df with one operand
#     and nothing with two;
#   * strings that are empty, longer than every line, all punctuation, with
#     blanks, with bytes above 0x7f, and every one of -t's cuts;
#   * files that are not sorted, have no final newline, CRLF lines, NUL bytes,
#     long lines; an empty file and a directory, which mmap refuses; missing
#     and unreadable files;
#   * WORDLIST, readable or not, -a, and a second operand over both;
#   * the command line's refusals, and closed or full descriptors -- with
#     output long enough to fail mid-way.
#
# Cases that differ on purpose: --version and -V name this build.
set -u

DIFF_PROG='look'
DIFF_NEED='timeout python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; broken=0
fix=$DIFF_TMP/fix
mkdir -p "$fix"

python3 - "$fix" <<'PY'
import os, sys
d = sys.argv[1]
def put(name, data, mode=None):
    path = os.path.join(d, name)
    with open(path, "wb") as f:
        f.write(data)
    if mode is not None:
        os.chmod(path, mode)
words = [b"a", b"A", b"aardvark", b"Aaron", b"abaca", b"aback", b"abacus", b"abandon",
         b"abandoned", b"abase", b"ab-initio", b"ab initio", b"able", b"Able", b"able-bodied",
         b"abort", b"aboard", b"abode", b"about", b"above", b"abc's", b"abc", b"ABC",
         b"apple", b"Apple", b"apple's", b"applet", b"apply", b"apt", b"Apt", b"b", b"B",
         b"banana", b"band", b"bandana", b"c3po", b"c-3po", b"zebra", b"zebra\tcrossing",
         b"zed", b"zeta", b"zoo", b"0", b"007", b"1st", b"caf\xc3\xa9", b"cafe",
         b"\xc3\xa9clair", b"\xe9t\xe9", b"\xff"]
# A dictionary's order, as look -df compares: only the C locale's blanks and
# alphanumerics (ASCII -- a byte above 0x7f is neither), case folded.
def dict_key(w):
    return bytes(c for c in w.lower()
                 if 48 <= c <= 57 or 97 <= c <= 122 or c in (32, 9))
dictionary = sorted(set(words), key=lambda w: (dict_key(w), w))
put("words", b"\n".join(dictionary) + b"\n")
put("bytewise", b"\n".join(sorted(set(words))) + b"\n")
put("unsorted", b"\n".join(words) + b"\n")
put("nolast", b"apple\napply\napt")
put("crlf", b"apple\r\napply\r\napt\r\n")
put("nul", b"ab\x00cd\nab\x00ce\nabc\n")
put("long", b"abc" + b"x" * 5000 + b"\nabd" + b"y" * 3000 + b"\n")
put("many", b"".join(b"match%05d\n" % i for i in range(2000)))
put("empty", b"")
put("one", b"only\n")
put("unreadable", b"apple\n", 0)
os.mkdir(os.path.join(d, "dir"))
PY

# --- knobs ------------------------------------------------------------------
# REDIR: redirections after the command. ENVS: the environment's extra
# entries.
REDIR=
ENVS=()
reset_knobs() { REDIR=; ENVS=(); }

run_side() {
  local side=$1; shift
  local -a envs=(env -i "PATH=$bindir/$side:/usr/bin:/bin" "LC_ALL=C.UTF-8" "${ENVS[@]}")
  local -a cmd=(timeout -k 2 20 "${envs[@]}" sh -c 'cd "$1"; redir=$2; shift 2; eval "exec look \"\$@\" $redir"' _ "$fix" "$REDIR" "$@")
  diff_run "${cmd[@]}" </dev/null
}

compare() {
  run_side ours "$@" >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  run_side gnu "$@" >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  LABEL="look $*"
  [ "${#ENVS[@]}" -gt 0 ] && LABEL="$LABEL [${ENVS[*]##*/}]"
  [ -n "$REDIR" ] && LABEL="$LABEL $REDIR"
  reset_knobs
  if [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ] || [ "$o_rc" = 127 ] || [ "$g_rc" = 127 ]; then
    AGREED=broken
  elif cmp -s "$DIFF_TMP/o.out" "$DIFF_TMP/g.out" \
     && cmp -s "$DIFF_TMP/o.err" "$DIFF_TMP/g.err" && [ "$o_rc" = "$g_rc" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  --- ours rc=%s\n%s\n  ~~~ stderr\n%s\n  --- gnu rc=%s\n%s\n  ~~~ stderr\n%s' \
    "$o_rc" "$(cat -A "$DIFF_TMP/o.out" | head -20)" "$(cat -A "$DIFF_TMP/o.err" | head -10)" \
    "$g_rc" "$(cat -A "$DIFF_TMP/g.out" | head -20)" "$(cat -A "$DIFF_TMP/g.err" | head -10)")
}

report() {
  if [ "$AGREED" = broken ]; then
    broken=$((broken + 1))
    printf 'BROKEN %s -- never reached look on one or both sides\n%s\n' "$LABEL" "$REPORT"
  elif [ "$AGREED" = yes ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report; }

xfail_case() {
  local why=$1; shift
  [ "${1:-}" = -- ] && shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass + 1))
    printf 'XPASS %s -- expected to differ (%s)\n' "$LABEL" "$why"
  elif [ "$AGREED" = broken ]; then
    report
  else
    xfail=$((xfail + 1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

# A guard against vacuous agreement: the reference must have found a word.
run_case -df apple words
if ! grep -q '^apple$' "$DIFF_TMP/g.out"; then
  echo "look-diff: the reference found nothing in the fixture:" >&2
  cat "$DIFF_TMP/g.out" "$DIFF_TMP/g.err" >&2
  exit 1
fi

# --- strings against each list, each mode ---------------------------------------
for f in words bytewise unsorted nolast crlf nul long; do
  for s in '' a A ab Ab AB abc "abc's" 'ab i' 'ab-i' "a'" apple appl Apple APPLE \
           b ban z zebra 'zebra	c' zz 0 00 1 c-3 c3 'caf' "$(printf 'caf\303')" \
           "$(printf '\303')" "$(printf '\351')" "$(printf '\377')" xyzzy only; do
    for o in '' -d -f -df -fd; do
      # shellcheck disable=SC2086  # the options are words, on purpose
      run_case $o "$s" "$f"
    done
  done
done
run_case ab many
run_case match0199 many
run_case match many

# --- -t ----------------------------------------------------------------------------
for t in b c a z "'" - '' bc; do
  run_case -t "$t" abc words
  run_case -t "$t" -df abc words
  run_case "-t$t" apples words
done
run_case --terminate=p apple words
run_case -t

# --- the word list: WORDLIST, -a, a second operand --------------------------------
ENVS=("WORDLIST=$fix/words"); run_case ab
ENVS=("WORDLIST=$fix/words"); run_case -d ab
ENVS=("WORDLIST=$fix/words"); run_case ab bytewise
ENVS=("WORDLIST=$fix/unreadable"); run_case ab
ENVS=("WORDLIST=$fix/nosuch"); run_case ab
ENVS=("WORDLIST="); run_case ab
ENVS=("WORDLIST=$fix/words"); run_case -a ab
ENVS=("WORDLIST=$fix/words"); run_case -a ab words
run_case -a ab
run_case ab

# --- files mmap and open refuse ----------------------------------------------------
run_case ab empty
run_case ab dir
run_case ab nosuch
run_case ab unreadable
run_case ab /dev/null

# --- the command line ----------------------------------------------------------------
run_case
run_case a b c
run_case -Z ab words
run_case --nosuch ab words
run_case -h
run_case --help
run_case --help ab
run_case -- -d words
run_case -d -- ab words
xfail_case "our version string, not util-linux's" -- -V
xfail_case "our version string, not util-linux's" -- --version

# --- descriptors that cannot be written -------------------------------------------------
for redir in '>&-' '>/dev/full' '2>&-' '2>/dev/full'; do
  REDIR=$redir; run_case ab words
  REDIR=$redir; run_case match many
  REDIR=$redir; run_case zzz words
  REDIR=$redir; run_case ab nosuch
  REDIR=$redir; run_case --help
done

printf '\n%d passed, %d differed, %d broken, %d differ on purpose' \
  "$pass" "$fail" "$broken" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$broken" -eq 0 ] && [ "$xpass" -eq 0 ]
