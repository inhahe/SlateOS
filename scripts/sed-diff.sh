#!/usr/bin/env bash
# Differential test: our sed against GNU sed.
#
# Each case gives both seds identical argv and identical input, and compares
# stdout, stderr and the exit status. stdout is compared as a hex dump rather
# than through `$(...)`, because command substitution strips trailing newlines
# and eats NUL bytes — and half of what sed has to get right lives exactly
# there: whether an unterminated last line gains a terminator, whether `N` on
# the final line prints the pattern space or discards it, whether `a\` after
# the last line emits its text with or without a newline of its own.
#
# ## What changed when this harness moved into WSL
#
# It used to build a *native Windows* sed and compare it against whatever `sed`
# MSYS2 put on `PATH`. Two consequences, both of which hid real differences:
#
#   * MSYS2's sed is a Cygwin build. Its `getopt` is not glibc's — `unknown
#     option -- x` against `invalid option -- 'x'` — so every option-error case
#     would have certified wording no GNU/Linux system prints. That is the same
#     defect `sort-diff.sh` carried green for eight cases; see `known-issues.md`
#     → `TD-COREUTILS-GETOPT-DIAGNOSTICS-USE-THE-WRONG-SHAPE`.
#   * stderr was compared only for *presence* — "did either side say
#     something?" — with a note that matching GNU's `char 7:` offsets would be
#     fitting to its parser's internals. That reasoning does not survive
#     contact with the code: ours already emits
#     `sed: -e expression #1, char N: …`, so the offsets are a claim this
#     implementation makes and a claim a harness should therefore check. They
#     are compared as text here, and any that cannot be matched is an `xfail`
#     with the reason written down rather than a whole category waved through.
#
# The move also made file operands testable. The old harness fed everything
# through stdin, so `-s`, `-i`, `$` addressing across a file boundary, and the
# `can't read` diagnostic were untested — and `-i` in particular is the one
# part of sed that can destroy data.
#
# Run `OURS=/usr/bin/sed ./scripts/sed-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

# Into WSL, build ours for Linux, find GNU's, and put both behind the one name
# `sed` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG=sed
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0; kbug=0; kfixed=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
cd "$fixtures" >/dev/null || exit 1

printf 'a\nb\nc\n'                      > abc.txt
printf '1\n2\n3\n4\n5\n'                > nums.txt
printf 'foo bar\nbaz foo\nqux\n'        > words.txt
printf 'a\n\n\n\nb\n\nc\n'              > blanks.txt
printf 'a\nb'                           > nonl.txt
printf 'x\nx\ny\ny\ny\nz\n'             > dup.txt
printf '/usr/bin\n/tmp/x\nrelative\n'   > paths.txt
printf 'Alpha1\nbeta22\nGAMMA333\n'     > mixed.txt
printf 'A\x01\x7f\x80\xff\xc3\xa9Z\n'   > bytes.txt
printf 'd\ne\nf\n'                      > def.txt
: > empty.txt

# --- one invocation of one side ----------------------------------------------
#
# `$1` is `ours` or `gnu`; `$2` is a file to feed on stdin, or `-` for none.
# The invocation goes through `$bindir/$side/sed`, a symlink whose *name* is
# `sed` on both sides, so the `sed: ` prefix on every diagnostic is produced
# from the same `argv[0]` and a difference in it is a difference in sed.
run_side() {
  local side=$1 stdin=$2 out=$3 err=$4; shift 4
  if [ "$stdin" = "-" ]; then
    env PATH="$bindir/$side" sed "$@" </dev/null >"$out" 2>"$err"
  else
    env PATH="$bindir/$side" sed "$@" <"$stdin" >"$out" 2>"$err"
  fi
}

# Sets `AGREED` and `REPORT`. `$1` is the stdin fixture or `-`; the rest is argv.
compare() {
  local o_out g_out o_err g_err o_bin g_bin o_rc g_rc stdin=$1; shift
  o_err=$(mktemp); g_err=$(mktemp); o_bin=$(mktemp); g_bin=$(mktemp)
  # stdout is redirected to a file rather than piped into `od`, so the status
  # recorded is sed's own. In `x=$(sed … | od)` the status belongs to `od`, and
  # `PIPESTATUS` is set inside the substitution's subshell where it cannot be
  # read — such a pipeline compares od's success against od's success and calls
  # every failing case a pass.
  run_side ours "$stdin" "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side gnu  "$stdin" "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(od -An -tx1 <"$o_bin"); g_out=$(od -An -tx1 <"$g_bin")
  rm -f "$o_bin" "$g_bin"

  # stderr as text, not merely for presence. Sound only because the reference
  # is glibc's: our `errmsg` prints POSIX's strerror strings, which agree with
  # glibc and did not agree with the Cygwin host this harness used to run on.
  #
  # `ERR_MODE=first-line` keeps only the first line of each side. It exists for
  # one shape of case: GNU sed answers *every* usage error by printing its whole
  # usage block under the sentence, and ours prints a different block on purpose
  # — shorter, and without the three GNU project URLs, which would be wrong here.
  # Compared in full, a dozen cases would all be xfails saying the same thing
  # about the block and nothing about the sentence, which is the part under test.
  # The block itself is still compared, once, by the `--help` xfail below.
  local o_msg g_msg
  if [ "${ERR_MODE:-full}" = first-line ]; then
    o_msg=$(head -1 "$o_err"); g_msg=$(head -1 "$g_err")
  else
    o_msg=$(cat "$o_err"); g_msg=$(cat "$g_err")
  fi
  rm -f "$o_err" "$g_err"

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  gnu  (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
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

# `run_stdin FIXTURE ARGS...` — the fixture on standard input.
run_stdin() {
  local input=$1; shift
  compare "$input" "$@"
  report "$input | sed $*"
}

# `run_case ARGS...` — file operands, nothing on standard input.
run_case() {
  compare - "$@"
  report "sed $*"
}

# `usage_case ARGS...` — a command line sed should refuse, compared on its
# stdout, its status and the *first line* of its stderr. See `ERR_MODE` above:
# the usage block beneath that first line is deliberately not GNU's.
usage_case() {
  # Set and reset rather than `ERR_MODE=first-line compare …`: an assignment
  # prefixed to a *function* call stays in effect after it returns in bash, so
  # the prefix form would silently put every later case into first-line mode.
  ERR_MODE=first-line
  compare - "$@"
  ERR_MODE=full
  report "sed $* (first stderr line)"
}

xfail_case() {
  local reason=$1; shift
  compare - "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL sed %s  (%s)\n' "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS sed %s\n  now agrees with GNU, so this reason is stale: %s\n' "$*" "$reason"
  fi
  return 0
}

xfail_stdin() {
  local reason=$1 input=$2; shift 2
  compare "$input" "$@"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s | sed %s  (%s)\n' "$input" "$*" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS %s | sed %s\n  now agrees with GNU, so this reason is stale: %s\n' \
      "$input" "$*" "$reason"
  fi
  return 0
}

# --- known bugs ---------------------------------------------------------------
#
# A divergence that is *not* deliberate but is not fixed yet. It is loud on
# every run and names its `known-issues.md` entry, but it does not fail the
# run — because a harness that is permanently red is a harness nobody reads,
# and that is exactly how `bc`'s `quit` stayed broken for months. A known bug
# that starts agreeing does fail, so the entry cannot outlive the defect.
kbug_case() {
  local id=$1; shift
  compare - "$@"
  if [ "$AGREED" = no ]; then
    kbug=$((kbug+1))
    printf 'KBUG sed %s  (%s)\n' "$*" "$id"
  else
    kfixed=$((kfixed+1))
    printf 'KFIXED sed %s\n  now agrees with GNU: close %s and drop this marker.\n' "$*" "$id"
  fi
  return 0
}

kbug_stdin() {
  local id=$1 input=$2; shift 2
  compare "$input" "$@"
  if [ "$AGREED" = no ]; then
    kbug=$((kbug+1))
    printf 'KBUG %s | sed %s  (%s)\n' "$input" "$*" "$id"
  else
    kfixed=$((kfixed+1))
    printf 'KFIXED %s | sed %s\n  now agrees with GNU: close %s and drop this marker.\n' \
      "$input" "$*" "$id"
  fi
  return 0
}

# --- in-place editing, which is the one part of sed that can destroy data -----
#
# Each side edits its *own* copy of the tree, and the comparison is of what the
# two directories hold afterwards — contents and names both, so that a `-i.bak`
# backup landing under the wrong name is caught. Running the two sides over one
# copy would have the second read what the first wrote.
run_inplace() {
  local label=$1; shift
  local o_err g_err o_rc g_rc o_state g_state
  rm -rf ours.d gnu.d
  mkdir -p ours.d gnu.d
  cp abc.txt def.txt nonl.txt empty.txt ours.d/
  cp abc.txt def.txt nonl.txt empty.txt gnu.d/
  # A directory operand, so that `-i` on something that is not a regular file
  # is covered: it opens happily and only fails on the first read, which is
  # exactly the case a naive implementation reports as a read error.
  mkdir ours.d/adir gnu.d/adir
  o_err=$(mktemp); g_err=$(mktemp)
  # stdin is `/dev/null` rather than inherited: `-i` with no operand is one of
  # the cases below, and a sed that failed to reject it would otherwise block
  # on the harness's own terminal and hang the run rather than fail it.
  ( cd ours.d && env PATH="$bindir/ours" sed "$@" ) </dev/null >/dev/null 2>"$o_err"; o_rc=$?
  ( cd gnu.d  && env PATH="$bindir/gnu"  sed "$@" ) </dev/null >/dev/null 2>"$g_err"; g_rc=$?
  # `find | sort` names every file, including one only one side created; `od`
  # of each in turn compares contents. A missing file shows up as an absent
  # block rather than as silence.
  o_state=$(cd ours.d && find . -type f | sort | while read -r f; do
              printf '== %s\n' "$f"; od -An -tx1 <"$f"; done)
  g_state=$(cd gnu.d && find . -type f | sort | while read -r f; do
              printf '== %s\n' "$f"; od -An -tx1 <"$f"; done)
  local o_msg g_msg
  # The directory name leaks into a diagnostic that quotes the path, so it is
  # normalised away before comparison — the difference under test is sed's
  # wording, not which scratch directory it ran in.
  o_msg=$(sed -e 's|ours\.d|D|g' "$o_err"); g_msg=$(sed -e 's|gnu\.d|D|g' "$g_err")
  rm -f "$o_err" "$g_err"

  if [ "$o_state" = "$g_state" ] && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   [inplace] %s\n' "$label"
  else
    fail=$((fail+1))
    printf 'DIFF [inplace] %s\n' "$label"
    printf '  ours (rc=%s) {%s}\n%s\n' "$o_rc" "$(printf '%s' "$o_msg" | tr '\n' '|')" "$o_state"
    printf '  gnu  (rc=%s) {%s}\n%s\n' "$g_rc" "$(printf '%s' "$g_msg" | tr '\n' '|')" "$g_state"
  fi
  rm -rf ours.d gnu.d
  return 0
}

echo "sed-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# --- substitution ------------------------------------------------------------
run_stdin words.txt 's/foo/FOO/'
run_stdin words.txt 's/foo/FOO/g'
run_stdin words.txt 's/o/0/2'
run_stdin words.txt 's/o/0/2g'
run_stdin words.txt 's/^/> /'
run_stdin words.txt 's/$/ </'
run_stdin words.txt 's/[abc]/./g'
run_stdin words.txt 's/[^abc]/./g'
run_stdin words.txt 's/\(b\)\(a\)/\2\1/'
run_stdin words.txt 's/.*/[&]/'
run_stdin words.txt 's/a/[\&]/'
run_stdin words.txt 's/a*/-/g'
run_stdin words.txt 's/x*/-/g'
run_stdin words.txt 's/.*/\U&/'
run_stdin mixed.txt 's/.*/\L&/'
run_stdin mixed.txt 's/\(.\)\(.*\)/\u\1\2/'
run_stdin mixed.txt 's/\(.*\)/\U\1\E!/'
run_stdin paths.txt 's|/usr|/opt|'
run_stdin paths.txt 's,/tmp,/var,'
run_stdin paths.txt 's/[/]/:/g'
run_stdin paths.txt 's/[^/]*$/LAST/'
run_stdin words.txt 's/[[:upper:]]/U/g'
run_stdin mixed.txt 's/[[:digit:]][[:digit:]]*//'
run_stdin words.txt 's/FOO/x/I'
run_stdin words.txt 's/o/0/p'
run_stdin words.txt -n 's/o/0/p'
run_stdin words.txt 's/a\+/X/'
run_stdin words.txt -E 's/a+/X/'
run_stdin words.txt -E 's/(foo|baz)/[\1]/'
run_stdin words.txt 's/foo\|baz/X/'
# A replacement carrying an escape that is not a group reference, and one
# carrying a literal newline.
run_stdin abc.txt 's/b/\n/'
run_stdin abc.txt 's/b/\t/'
run_stdin abc.txt $'s/b/x\\\ny/'
# Undecodable bytes: a substitution has to leave them alone, and a class has to
# match them as bytes rather than dropping them.
run_stdin bytes.txt 's/Z/z/'
run_stdin bytes.txt 's/[[:print:]]//g'
run_stdin bytes.txt 's/.*/[&]/'

# --- addresses ---------------------------------------------------------------
run_stdin nums.txt '2d'
run_stdin nums.txt '2,4d'
run_stdin nums.txt '$d'
run_stdin nums.txt '1,$d'
run_stdin nums.txt -n '2,+2p'
run_stdin nums.txt -n '2,~4p'
run_stdin nums.txt -n '4,~4p'
run_stdin nums.txt -n '1~2p'
run_stdin nums.txt -n '0~2p'
run_stdin abc.txt -n '/b/p'
run_stdin abc.txt -n '/b/,/c/p'
run_stdin abc.txt -n '/a/,/zz/p'
run_stdin abc.txt -n '0,/a/p'
run_stdin abc.txt -n '1,/a/p'
run_stdin abc.txt '2!d'
run_stdin abc.txt -n '/B/Ip'
run_stdin paths.txt -n '\%/usr%p'
run_stdin abc.txt -n '$='
run_stdin abc.txt '='

# --- the rest of the command set ---------------------------------------------
run_stdin abc.txt 'p'
run_stdin abc.txt -n 'p'
run_stdin abc.txt 'd'
run_stdin abc.txt '1!G;h;$!d'
run_stdin abc.txt '$!N;s/\n/ /'
run_stdin abc.txt 'N;s/\n/ /'
run_stdin abc.txt -n 'n;p'
run_stdin blanks.txt '/^$/{N;/^\n$/D}'
run_stdin dup.txt '$!N;/^\(.*\)\n\1$/!P;D'
run_stdin abc.txt ':a;s/a/b/;ta'
run_stdin words.txt ':a;s/  / /;ta'
run_stdin abc.txt 's/a/x/;T end;s/$/ (changed)/;:end'
run_stdin abc.txt '/b/{s/b/B/;p}'
run_stdin abc.txt 'y/abc/xyz/'
run_stdin abc.txt $'2i\\\nbefore'
run_stdin abc.txt '1a after'
run_stdin abc.txt $'2c\\\nnew'
run_stdin abc.txt $'1,2c\\\nnew'
run_stdin abc.txt '2q'
run_stdin abc.txt '2Q'
run_stdin abc.txt '2q5'
run_stdin abc.txt 'h;s/./X/;G'
run_stdin abc.txt 'H;$!d;x;s/\n/,/g'
run_stdin abc.txt -n 'x;p'
run_stdin abc.txt -n '$!{h;d};x;p'
run_stdin abc.txt $'#n\np'
run_stdin abc.txt $'# comment\np'
# `a`, `i` and `c` against the last line and against no line at all, which is
# where the trailing-newline question lives.
run_stdin nonl.txt '$a appended'
run_stdin nonl.txt '$c replaced'
run_stdin empty.txt '$a appended'
run_stdin abc.txt '1i inserted'

# --- the script as an operand, and -e / -f -----------------------------------
run_stdin abc.txt -e 's/a/A/' -e 's/b/B/'
run_stdin abc.txt --expression='s/a/A/' --expression='s/c/C/'
printf 's/a/A/\ns/b/B/\n' > script.sed
run_stdin abc.txt -f script.sed
run_stdin abc.txt --file=script.sed
run_stdin abc.txt -f script.sed -e 's/c/C/'
printf '' > empty.sed
run_stdin abc.txt -f empty.sed
run_case -f nosuch.sed abc.txt

# --- file operands ------------------------------------------------------------
run_case -n '$p' abc.txt
run_case -n '$p' abc.txt def.txt
run_case -n '$=' abc.txt def.txt
run_case -s -n '$p' abc.txt def.txt
run_case -s -n '$=' abc.txt def.txt
run_case 'p' abc.txt def.txt
run_case -n 'p' empty.txt abc.txt
run_case 's/a/A/' abc.txt nonl.txt
run_case 's/a/A/' nosuch.txt
run_case 's/a/A/' abc.txt nosuch.txt def.txt
run_case 's/a/A/' .
run_case -n 'p' -
run_case 's/a/A/' abc.txt -

# --- in-place -----------------------------------------------------------------
run_inplace 'plain -i'            -i 's/a/A/' abc.txt def.txt
run_inplace '-i with a suffix'    -i.bak 's/a/A/' abc.txt
run_inplace '-i with a * suffix'  -i'bak_*' 's/a/A/' abc.txt
run_inplace '--in-place=.bak'     --in-place=.bak 's/b/B/' abc.txt
run_inplace '-i on an empty file' -i 's/a/A/' empty.txt
run_inplace '-i, no terminator'   -i '$a end' nonl.txt
run_inplace '-i and q'            -i '1q' abc.txt def.txt
run_inplace '-i, missing file'    -i 's/a/A/' abc.txt nosuch.txt
run_inplace '-i with no operand'  -i 's/a/A/'
run_inplace '-n -i, and p'        -n -i 'p' abc.txt
# `-` is a file named `-` here, not standard input: a stream cannot be edited
# in place, so it gets no special case.
run_inplace '-i on a - operand'   -i 's/a/A/' -
run_inplace '-i on a directory'   -i 's/a/A/' adir
run_inplace '-i, dir then file'   -i 's/a/A/' adir abc.txt

# --- w, W and the `s///w` flag ------------------------------------------------
#
# The file a `w` writes is the whole output of these cases, so it has to be
# compared — and each side needs its own, or the second run overwrites the
# first and the comparison is of one side against itself. `@W@` in the script
# is replaced by a per-side path, and the two files are compared afterwards.
run_w() {
  local input=$1; shift
  local o_err g_err o_rc g_rc o_out g_out o_file g_file o_msg g_msg
  local o_script g_script arg
  rm -f w-ours w-gnu
  : > w-ours; : > w-gnu
  o_err=$(mktemp); g_err=$(mktemp); o_out=$(mktemp); g_out=$(mktemp)
  # Rebuild argv twice, substituting the per-side path. `${arg//@W@/…}` rather
  # than a `sed` pass, so a script containing regex metacharacters is untouched.
  local -a o_argv=() g_argv=()
  for arg in "$@"; do o_argv+=("${arg//@W@/w-ours}"); g_argv+=("${arg//@W@/w-gnu}"); done
  # `-` for "nothing on standard input", as elsewhere in this harness: the
  # cases that pass file operands must not also have a fixture on stdin.
  [ "$input" = "-" ] && input=/dev/null
  env PATH="$bindir/ours" sed "${o_argv[@]}" <"$input" >"$o_out" 2>"$o_err"; o_rc=$?
  env PATH="$bindir/gnu"  sed "${g_argv[@]}" <"$input" >"$g_out" 2>"$g_err"; g_rc=$?
  o_script=$(od -An -tx1 <"$o_out"); g_script=$(od -An -tx1 <"$g_out")
  o_file=$(od -An -tx1 <w-ours); g_file=$(od -An -tx1 <w-gnu)
  # The per-side name appears in a diagnostic about the file, so it is folded
  # back to one spelling before the wording is compared.
  o_msg=$(sed -e 's/w-ours/W/g' "$o_err"); g_msg=$(sed -e 's/w-gnu/W/g' "$g_err")
  rm -f "$o_err" "$g_err" "$o_out" "$g_out" w-ours w-gnu

  if [ "$o_script" = "$g_script" ] && [ "$o_file" = "$g_file" ] \
     && [ "$o_rc" = "$g_rc" ] && [ "$o_msg" = "$g_msg" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   [w] %s | sed %s\n' "$input" "$*"
  else
    fail=$((fail+1))
    printf 'DIFF [w] %s | sed %s\n' "$input" "$*"
    printf '  ours (rc=%s) out=%s file=%s {%s}\n' "$o_rc" \
      "$(printf '%s' "$o_script" | tr -s ' \n' ' ')" \
      "$(printf '%s' "$o_file" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')"
    printf '  gnu  (rc=%s) out=%s file=%s {%s}\n' "$g_rc" \
      "$(printf '%s' "$g_script" | tr -s ' \n' ' ')" \
      "$(printf '%s' "$g_file" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')"
  fi
  return 0
}

run_w abc.txt -n 'w @W@'
run_w abc.txt 'w @W@'
run_w abc.txt -n 's/a/A/w @W@'
run_w abc.txt -n '$!N;W @W@'
run_w nonl.txt -n 'w @W@'
run_w abc.txt -n '/b/w @W@'
# Two `w`s naming one file share a single handle in GNU, so the second does not
# truncate what the first wrote.
run_w abc.txt -n -e '/a/w @W@' -e '/c/w @W@'
# The same handle has to survive the *file* boundary under `-s`, where sed
# restarts its line numbering for each operand: a target reopened per file
# would truncate away everything the previous one wrote.
run_w - -s -n 'w @W@' abc.txt def.txt
run_w - -n 'w @W@' abc.txt def.txt
run_stdin abc.txt -n 'w /dev/stdout'
run_stdin abc.txt -n 'p;w /dev/stderr'
run_stdin abc.txt -n 'w /nosuch/dir/file'
# The `w` target is opened while the script is compiled, so an unopenable one
# fails even when no line ever reaches the command.
run_stdin - -n 'w /nosuch/dir/file'
run_stdin - -n '/nomatch/w /nosuch/dir/file'

# --- reading a file back: r and R --------------------------------------------
run_stdin abc.txt "1r def.txt"
run_stdin abc.txt "\$r def.txt"
run_stdin abc.txt "1r nosuch.txt"
run_stdin abc.txt "1R def.txt"
run_stdin abc.txt "R def.txt"

# --- odd inputs ---------------------------------------------------------------
run_stdin nonl.txt 'p'
run_stdin nonl.txt 's/b/B/'
run_stdin nonl.txt '$!N;s/\n/-/'
run_stdin empty.txt 'p'
run_stdin empty.txt 's/a/b/'
run_stdin abc.txt -s 'p'
run_stdin abc.txt -n '$p'
run_stdin abc.txt -z 's/a/A/'
run_stdin abc.txt --posix 's/a/A/'
run_stdin abc.txt -u 's/a/A/'
run_stdin abc.txt --debug 's/a/A/'
run_stdin abc.txt --sandbox 's/a/A/'
run_stdin abc.txt --sandbox 'w /tmp/x'
run_stdin abc.txt -l 3 's/.*/aaaaaaaa/;l'
run_stdin bytes.txt -n 'l'
run_stdin abc.txt -n 'l 0'

# --- script errors ------------------------------------------------------------
#
# These are the cases the old harness could only ask "did it complain?" of.
# Compared as text, they assert the `-e expression #N, char C:` prefix and the
# sentence after it.
run_stdin abc.txt 'Z'
run_stdin abc.txt 's/a'
run_stdin abc.txt '{p'
run_stdin abc.txt 'p}'
run_stdin abc.txt 'b nowhere'
run_stdin abc.txt 'y/ab/x/'
run_stdin abc.txt 's/[a/x/'
run_stdin abc.txt 's/a/b/q'
run_stdin abc.txt 's/a/b/2p2'
run_stdin abc.txt '1,2,3p'
run_stdin abc.txt '/a'
run_stdin abc.txt '1~'
run_stdin abc.txt 'q9999999999999999999'
run_stdin abc.txt ':'
run_stdin abc.txt ':a;:a;p'
run_stdin abc.txt '}'
run_stdin abc.txt 's/\(/x/'
run_stdin abc.txt 's/a/\9/'
run_stdin abc.txt '2,1p'
run_stdin abc.txt '0p'
run_stdin abc.txt '0,5p'
run_stdin abc.txt 'w'
run_stdin abc.txt 'r'
run_stdin abc.txt -e 's/a/A/' -e 'Z'

# --- usage errors -------------------------------------------------------------
#
# glibc's getopt on both sides now, so the quoting and the wording of these are
# a real comparison rather than a Cygwin artefact.  They go through
# `usage_case`, which compares only the first stderr line: the usage block GNU
# prints beneath it is deliberately not ours, and is compared once, in full, by
# the `--help` xfail at the bottom of this section.
usage_case
usage_case -x 'p' abc.txt
usage_case --nosuchopt 'p' abc.txt
usage_case -f
usage_case -e
usage_case -i.bak
# Not a usage error: a long option may be abbreviated to any unambiguous
# prefix, and `--expr` is one.  It sits here, beside the errors, because it is
# the boundary case for the ambiguity check they exercise.
run_case --expr 'p' abc.txt
# `--help` and `--version` reach the option table like any other long option,
# so a value attached to one is a table question and is compared.
usage_case --help=x
usage_case --version=x
# What they *print* is not: ours names SlateOS and omits the GNU project's
# `Report bugs to:` block, exactly as every other utility here does.
xfail_case 'help omits the GNU bug-report block' --help
xfail_case 'version names SlateOS' --version

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
