#!/usr/bin/env bash
# Differential test: our sh against dash.
#
# ## Why dash and not bash
#
# `userspace/coreutils/src/bin/sh.rs` is the *POSIX baseline* shell — the one
# the image boots with and the one `#!/bin/sh` scripts get. bash's superset
# lives in `userspace/oils` (`osh`), and `osh-diff.sh` compares that against
# bash. Comparing this shell against bash would therefore report a hundred
# differences that are all the same difference, and none of them a defect.
# dash is Debian's `/bin/sh`, is POSIX with very little added, and is the
# reference this shell's semantics were measured against while it was written.
#
# ## What is compared, and the two things that are normalised
#
# stdout, the exit status, stderr, and the files the case left behind. Two
# fields of a diagnostic cannot be compared and are normalised away by
# `norm_diag`, which runs over all three byte streams — stderr, stdout, and each
# file on disk — because `2>&1` and `2>err` and `exec 2>log` move a diagnostic
# between them freely:
#
#   * dash prefixes a diagnostic with the line number it was reading —
#     `sh: 1: nosuch: not found` — and this shell does not track line numbers,
#     so every diagnostic case would differ by that field alone;
#   * dash abbreviates `strerror`, saying `No such file` where every other
#     program on the system says `No such file or directory`; dash's side is
#     expanded to the POSIX text rather than ours cut down to dash's.
#
# Nothing else about a message is touched, so a difference in the words is still
# a difference.
#
# Two smaller consequences of the same fact:
#
#   * the *wording* of a syntax error is ours by design (dash says
#     `Syntax error: "then" unexpected`, we name the token we wanted), so
#     `syntax_case` compares stdout and status and only asks that both sides
#     agreed on *whether* to complain;
#   * anything with a pid in it — `$$`, `$!`, `$PPID` — cannot be compared at
#     all and is not attempted.
#
# ## PATH
#
# `$bindir/$side` first, so `sh` is the side under test even when a script
# runs `sh` recursively, then `/usr/bin:/bin` so that `cat`, `head`, `tr` and
# friends exist. It must *not* be the harness's inherited PATH: under WSL that
# contains the Windows directories, where a lookup for a name that does not
# exist fails with `EACCES` rather than `ENOENT`, and both shells then report
# `Permission denied` for a missing command. Measured — that is exactly what
# an early run of this harness reported, for every one of its cases.
#
# Run `OURS=/bin/dash ./scripts/sh-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

# Into WSL, build ours for Linux, find dash, and put both behind the one name
# `sh` so `argv[0]` matches. See `scripts/diff-wsl.sh`.
DIFF_PROG='sh'
# `command -v sh` would find WSL's own `/bin/sh`, which is dash — but by way of
# a symlink whose name is `sh`, so dash would print `sh:` on one side and this
# name is what the harness relies on matching. Naming dash directly is the
# same program with the argv[0] question settled by the symlink `diff-wsl.sh`
# makes.
DIFF_REF='/bin/dash /usr/bin/dash'
DIFF_NEED='cat head tr wc sort sed seq od yes ls perl'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

# Which sides `norm_diag` has dash's quirks to undo on. Both rules exist because
# a side *is* dash, not because it is the reference — and the self-check
# (`OURS=/bin/dash`) puts dash on both, where treating only one of them as dash
# would report every diagnostic case as a difference and make the self-check
# useless for its one job.
case ${OURS:-} in
  *dash) NORM_OURS=gnu ;;
  *)     NORM_OURS=ours ;;
esac

pass=0; fail=0; xfail=0; xpass=0; kbug=0; kfixed=0

# --- the fixture template -----------------------------------------------------
#
# Copied fresh for each side of each case, so a case whose `>` rewrites a file
# cannot change what the next case reads.
template=$DIFF_TMP/template
mkdir -p "$template/sub"
printf 'alpha\nbeta\ngamma\n' > "$template/f.txt"
printf 'one\ntwo\n'           > "$template/g.txt"
printf 'x'                    > "$template/nonl.txt"
: > "$template/empty.txt"
printf 'deep\n'               > "$template/sub/h.txt"
# A name no `String` can hold. A shell that converts a path to `str` anywhere
# mangles this one, and the point of the rewrite was that none does.
printf 'raw\n'                > "$template/$(printf 'b\xff')ad.txt"

# --- one invocation of one side ----------------------------------------------
#
# `$1` is `ours` or `gnu`; `$2` is the case's private directory; `$3` says how
# the script reaches the shell — `arg` for `sh -c`, `stdin` for a pipe, or
# `file` for `sh script`. `$4` is the script, still in its `\n` form.
#
# The `%b` expansion happens here, at the point of use, never through a
# `$(...)`: command substitution strips trailing newlines, and for a shell
# script the presence of the last one decides whether the final command runs
# at all when the file ends mid-word.
run_side() {
  local side=$1 dir=$2 kind=$3 script=$4 out=$5 err=$6; shift 6
  (
    cd "$dir" || exit 125
    PATH=$bindir/$side:/usr/bin:/bin
    export PATH
    case $kind in
      file)
        printf '%b' "$script" > .script
        diff_run sh .script "$@" > "$out" 2> "$err"
        ;;
      stdin)
        printf '%b' "$script" | diff_run sh "$@" > "$out" 2> "$err"
        ;;
      *)
        # `printf -v`, not `$(printf …)`: command substitution strips every
        # trailing newline, and `sh -c 'cat <<EOF\nx\nEOF\n'` is a different
        # program from the same text with the last newline removed.
        local body
        printf -v body '%b' "$script"
        diff_run sh -c "$body" "$@" > "$out" 2> "$err"
        ;;
    esac
  )
}

# dash's `sh: N: ` becomes `sh: `. See the header: this is the one field the
# two shells cannot agree on, and removing it is what lets every other word of
# every diagnostic still be compared.
#
# The second rule undoes an abbreviation, and is deliberately the *only* other
# one. dash carries a private table of shortened `strerror` strings — `ENOENT`
# is "No such file" there, not the "No such file or directory" that every other
# program on the system prints — to keep its own error path free of `strerror`.
# We print the POSIX text on purpose (`coreutils::errmsg::strerror`, so that a
# diagnostic reads the same whichever host the binary was built for), and being
# *more* informative than the reference is not a defect to fix. Expanding dash's
# side rather than truncating ours keeps the comparison honest about which one
# each shell actually emits: a case where we regressed to some third wording
# would still differ.
#
# ## Why this runs over stdout and over the files a case left behind, too
#
# A diagnostic is not confined to descriptor 2. `nosuchcmd 2>&1` puts it on
# stdout, `nosuchcmd 2>err` puts it in a file, and `exec 2>log` puts everything
# after it in a file — and the line number is exactly as unagreeable there.
# Normalising only stderr made four such cases differ by nothing but `1: `.
#
# ## Why it edits only dash's side
#
# Both rules undo something only dash does, so running them over our side can
# only destroy information. That is not hypothetical: this shell's own message
# for a bad descriptor *is* `sh: 3: Bad file descriptor`, and a side-blind
# `^sh: \d+: ` rule ate the `3` from ours while dash's `sh: 1: 3: …` kept it —
# turning agreement into a difference and, worse, capable of hiding one.
#
# ## Why perl and not sed
#
# `sed` appends a newline to input that lacks one, and the fixtures include a
# file that deliberately ends without one (`nonl.txt`); running the disk state
# through `sed` would add it on both sides and hide any genuine difference in
# the last byte. `perl -0777 -p` reads the whole stream and writes back exactly
# what it read, so the filter is byte-exact everywhere it does not match.
norm_diag() {
  if [ "$1" != gnu ]; then
    cat
  else
    LC_ALL=C perl -0777 -pe 's/^sh: \d+: /sh: /mg; s/: No such file$/: No such file or directory/mg'
  fi
}

# Kept as a name of its own because the three call sites mean different things:
# this one is the stream the diagnostic was aimed at.
norm_err() {
  norm_diag "$1"
}

# The files a case left behind, as one comparable string. A redirection that
# writes the wrong bytes prints nothing and exits 0; the disk is the only
# place it shows.
dir_state() {
  local dir=$1 side=$2 f
  ( cd "$dir" || exit 1
    find . -type f ! -name .script | LC_ALL=C sort | while IFS= read -r f; do
      printf '%s:' "$f"; norm_diag "$side" < "$f" | od -An -tx1 | tr -s ' \n' ' '; printf '\n'
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
  # stdout to a file, never into a pipe: in `x=$(sh … | od)` the status
  # recorded is `od`'s, so every failing case would be scored a pass.
  run_side ours "$o_dir" "$kind" "$script" "$o_bin" "$o_err" "$@"; o_rc=$?
  run_side gnu  "$g_dir" "$kind" "$script" "$g_bin" "$g_err" "$@"; g_rc=$?
  o_out=$(norm_diag "$NORM_OURS" <"$o_bin" | od -An -tx1)
  g_out=$(norm_diag gnu  <"$g_bin" | od -An -tx1)
  rm -f "$o_bin" "$g_bin"

  local o_msg g_msg
  case ${ERR_MODE:-full} in
    none)     o_msg=; g_msg= ;;
    presence) o_msg=$([ -s "$o_err" ] && echo yes); g_msg=$([ -s "$g_err" ] && echo yes) ;;
    *)        o_msg=$(norm_err "$NORM_OURS" <"$o_err"); g_msg=$(norm_err gnu <"$g_err") ;;
  esac
  rm -f "$o_err" "$g_err"

  o_st=$(dir_state "$o_dir" "$NORM_OURS"); g_st=$(dir_state "$g_dir" gnu)

  if [ "$o_out" = "$g_out" ] && [ "$o_rc" = "$g_rc" ] &&
     [ "$o_msg" = "$g_msg" ] && [ "$o_st" = "$g_st" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): %s  {%s}\n  dash (rc=%s): %s  {%s}' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_msg" | tr '\n' '|')" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_msg" | tr '\n' '|')")
  if [ "$o_st" != "$g_st" ]; then
    REPORT=$(printf '%s\n  DISK ours: %s\n  DISK dash: %s' "$REPORT" \
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

# `run SCRIPT` — the script as the argument of `-c`, which is how a shell is
# reached from another program and the only form in which a one-liner has no
# file behind it.
run() {
  compare arg "$1"
  report "sh -c '$1'"
}

# `run_stdin SCRIPT` — the same bytes down a pipe. Not a duplicate: a shell
# reading a *pipe* cannot seek, so a here-document, a `read` and the script
# text itself all come off one unseekable stream and can steal each other's
# bytes. That is the bug shape this form exists to catch.
run_stdin() {
  compare stdin "$1"
  report "sh <<< '$1'"
}

# `run_file SCRIPT` — the script in a regular file, named as an operand.
run_file() {
  compare file "$1"
  report "sh script('$1')"
}

# `syntax_case SCRIPT` — a script neither shell will run. The wording of the
# complaint is ours by design (see the header), so only stdout, the status and
# *whether* stderr was written are compared.
syntax_case() {
  ERR_MODE=presence
  compare arg "$1"
  ERR_MODE=full
  report "syntax: sh -c '$1' (stderr presence only)"
}

xfail_run() {
  local reason=$1
  compare arg "$2"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf "XFAIL sh -c '%s'  (%s)\\n" "$2" "$reason"
  else
    xpass=$((xpass+1))
    printf "XPASS sh -c '%s'\\n  now agrees with dash, so this reason is stale: %s\\n" \
      "$2" "$reason"
  fi
  return 0
}

# A divergence that is not deliberate and is not fixed yet. Loud on every run
# and naming its `known-issues.md` entry, but it does not fail the run — a
# harness that is permanently red is a harness nobody reads.
kbug_run() {
  local entry=$1
  compare arg "$2"
  if [ "$AGREED" = no ]; then
    kbug=$((kbug+1))
    printf "KBUG sh -c '%s'  (known-issues.md -> %s)\\n%s\\n" "$2" "$entry" "$REPORT"
  else
    kfixed=$((kfixed+1))
    printf "KFIXED sh -c '%s'\\n  agrees with dash now; close known-issues.md -> %s\\n" \
      "$2" "$entry"
  fi
  return 0
}

# === the four cases the old line-scanner could not do =========================
#
# Every one of these was measured against dash before the rewrite and every one
# of them differed. They are first so that a regression in the shape of the
# whole thing is the first thing printed.

run 'if true; then echo yes; fi'
run 'while false; do echo x; done; echo end'
run 'case a in a) echo m;; esac'
run 'echo $((2+3))'
run 'f() { echo in f; }; f'
run 'echo ${UNSET:-def}'
run 'set -- a b c; echo $#; echo $@'
run 'echo a`echo b`c'
run 'read x <<EOF\nhello\nEOF\necho $x'
run '( echo sub )'
run '{ echo grp; }'
run 'echo $(echo nested $(echo deep))'
run 'yes | head -2'

# === words, quoting and splitting =============================================

run 'echo a b   c'
run "echo 'a b'"
run 'echo "a b"'
run 'echo a\\ b'
run "echo ''; echo [\$#]"
run "set -- ''; echo \$#"
run 'echo "a\\tb"'
run 'echo a\\tb'
run 'X="a  b"; echo $X'
run 'X="a  b"; echo "$X"'
run 'X="a  b"; set -- $X; echo $#'
run 'echo "unterminated in a quote is fine here"'
run 'echo a"b"c'
run "echo a'b'c"
run 'echo "$(echo inner)"'
# An empty field is data when IFS is not whitespace, and the hole has to stay
# where it was: `a::b` is three fields, `:a` is two, `a:` is one. A shell that
# dropped empties would silently renumber every positional after the hole.
run 'IFS=:; X=a::b; set -- $X; echo $#; echo "[$1][$2][$3]"'
run 'IFS=:; X=:a; set -- $X; echo $#; echo "[$1][$2]"'
run 'IFS=:; X=a:; set -- $X; echo $#; echo "[$1][$2]"'
run 'IFS=:; X=:::; set -- $X; echo $#'
run 'IFS=:; X=a::b; for f in $X; do echo "[$f]"; done'
run 'IFS=:; X=a:; set -- $X; echo $#'
run 'IFS=; set -- a b; echo "$*"'
run 'set -- a b c; IFS=-; echo "$*"; echo "$@"'
run 'echo ~ | wc -c'
run 'HOME=/nowhere; echo ~'
run 'HOME=/nowhere; echo ~/x'
run 'echo x~'

# === parameters ===============================================================

run 'echo ${UNSET-def}'
run 'X=; echo [${X-def}] [${X:-def}]'
run 'echo ${U:=set}; echo $U'
run 'X=v; echo ${X:+yes}; echo ${U:+yes}'
run 'X=abcdef; echo ${#X}'
run 'echo ${#UNSET}'
run 'F=a.b.c; echo ${F#*.} ${F##*.}'
run 'F=a.b.c; echo ${F%.*} ${F%%.*}'
run 'F=a.b.c; echo ${F#nomatch}'
run 'echo ${UNSET?}'
run 'echo ${UNSET?custom words}'
run 'X=v; echo ${X?}'
run 'echo $?'
run 'false; echo $?'
run 'set -- a b; echo $1$2'
run 'set -- a b c d e f g h i j k; echo ${10}'
run 'echo "${UNSET:-a b}" | wc -w'
run 'set -- a b c; shift; echo $@'
run 'set -- a b c; shift 2; echo $@'
run 'set -- a; shift 2; echo $?'

# === expansion order ==========================================================

run 'X="*"; echo $X'
run 'X="*"; echo "$X"'
run 'echo *.txt'
run 'echo nomatch*.zzz'
run 'echo sub/*'
run 'echo [fg].txt'
run 'echo ?.txt'
run 'set -f; echo *.txt'
run 'echo {a,b}'

# === arithmetic ===============================================================

run 'echo $((1+2*3))'
run 'echo $(((1+2)*3))'
run 'echo $((7/2)) $((-7/2)) $((7%2)) $((-3%2))'
run 'echo $((1<<4)) $((-16>>2))'
run 'echo $((1&3)) $((1|2)) $((1^3)) $((~0))'
run 'echo $((1&&0)) $((0||3)) $((!0)) $((!5))'
run 'echo $((1<2)) $((2<=2)) $((3>4)) $((4>=4)) $((1==1)) $((1!=1))'
run 'echo $((1?5:6)) $((0?5:6))'
run 'echo $((0x10)) $((010)) $((10))'
run 'X=3; echo $((X+1))'
run 'echo $((X=4)); echo $X'
run 'X=5; echo $((X+=2)); echo $X'
xfail_run 'an empty arithmetic expansion is 0 here, as in bash and ksh; dash alone makes it a syntax error, and POSIX does not say' \
  'echo $(( ))'
run 'echo $((1/0))'
run 'echo $((2)); echo $?'

# === control flow =============================================================

run 'if false; then echo a; else echo b; fi'
run 'if false; then echo a; elif true; then echo b; else echo c; fi'
run 'i=0; while [ $i -lt 3 ]; do echo $i; i=$((i+1)); done'
run 'i=0; until [ $i -ge 2 ]; do echo $i; i=$((i+1)); done'
run 'for x in a b c; do echo $x; done'
run 'set -- p q; for x; do echo $x; done'
run 'for x in; do echo $x; done; echo end'
run 'for a in 1 2; do for b in x y; do echo $a$b; break 2; done; done'
run 'for a in 1 2 3; do [ $a = 2 ] && continue; echo $a; done'
run 'for a in 1 2; do for b in x y; do echo $a$b; continue 2; done; done'
run 'case abc in a*) echo one;; *) echo two;; esac'
run 'case zzz in a*) echo one;; *) echo two;; esac'
run 'case b in a|b) echo alt;; esac'
run "case '*' in '*') echo lit;; esac"
run 'case x in y) echo n;; esac; echo $?'
run 'true && echo yes || echo no'
run 'false && echo yes || echo no'
run '! true; echo $?'
run '! false; echo $?'
run 'f() { echo $1-$2; }; f a b'
run 'f() { return 3; }; f; echo $?'
run 'set -- x y z; f() { echo $#; }; f a; echo $#'
run 'f() { g() { echo inner; }; }; f; g'
run 'x=1; { x=2; }; echo $x'
run 'x=1; ( x=2 ); echo $x'
run '( exit 4 ); echo $?'
run 'exit 3; echo unreached'
run 'exit 300; echo unreached'

# === builtins =================================================================

run 'echo'
run 'echo -n x; echo'
run 'echo -e x'
run 'echo -n'
# Four backslashes, not two. These cases are written in double quotes — the
# script text has a single-quoted string inside it — and bash collapses `\\` to
# `\` there, so `\\tb` would reach `run` as `\tb` and `printf '%b'` would turn it
# into a real tab before the shell under test ever saw it. The `\c` case was the
# worst of the three: `%b` treats `\c` as "stop output", which truncated the
# script mid-quote and made both shells report a syntax error, so the case
# silently tested nothing. The single-quoted cases above (`run 'echo "a\\tb"'`)
# need only two, because single quotes pass the pair through untouched.
run "echo 'a\\\\tb'"
run "echo 'a\\\\cb'; echo after"
run "echo 'a\\\\0101b'"
run ':; echo $?'
run 'true; echo $?'
run 'false; echo $?'
run 'cd sub && pwd | sed "s|.*/||"'
xfail_run 'cd names the errno the way bash does, where dash says only that it could not cd and drops the reason. Status and stdout agree; knowing whether it was ENOENT or EACCES is worth more than matching dash word for word' \
  'cd /nonexistent-dir; echo $?'
run 'cd sub; cd ..; ls f.txt'
run 'export V=1; sh -c "echo [\$V]"'
run 'V=1; sh -c "echo [\$V]"'
run 'V=1 sh -c "echo [\$V]"'
run 'V=1 true; echo [$V]'
run 'V=1 :; echo [$V]'
run 'x=1; unset x; echo [$x]'
run 'f() { echo f; }; unset -f f; f; echo $?'
run 'set -- a b c; echo $#; set --; echo $#'
# An error in a *special* builtin ends a non-interactive shell — which is not a
# formality: `shift 2` with one parameter leaves the parameters untouched, so a
# script that carried on would work on the wrong argument. A non-zero status is
# a different thing entirely, and `false` still does not end anything.
run 'set -- a; shift 2; echo $?'
run 'set -- a b; shift 1; echo $#; echo unreached-not'
syntax_case 'eval "if"; echo unreached'
run '. /nonexistent/file; echo unreached'
run 'export 1bad=x; echo unreached'
run 'false; echo still-here'
run "eval 'a=1; echo \$a'"
run "X='echo hi'; eval \$X"
run 'read a b <<EOF\none two three\nEOF\necho [$a][$b]'
run 'read x <<EOF\n\nEOF\necho [$x]'
run 'read x; echo [$x]'
run 'wait; echo $?'
# `&` on a compound command. There is no job control here and no fork, so the
# group runs synchronously in the shell itself and `wait` then has nothing to
# wait for — which is indistinguishable from dash's behaviour for a group this
# short, and is why this is a plain case and not an xfail. Only a *simple*
# command backgrounds for real, by being spawned and not waited for.
run '{ echo grouped; } & wait'
run 'nosuchcommand; echo $?'
run './f.txt; echo $?'
run '/nonexistent/program; echo $?'

# === redirection and here-documents ===========================================

run 'echo one > out; cat out'
run 'echo one > out; echo two > out; cat out'
run 'echo one > out; echo two >> out; cat out'
run 'cat < f.txt'
run 'cat 0< f.txt'
run 'echo hi 1> out; cat out'
run 'nosuchcommand 2> err; cat err | wc -l'
run 'echo out; nosuchcommand 2>&1'
run 'exec 3< f.txt; cat <&3'
run 'cat < nosuchfile.txt; echo $?'
run 'echo x > sub; echo $?'
run 'set -C; echo one > out; echo two > out; echo $?; cat out'
run 'cat <<EOF\nplain\nEOF'
run 'X=v; cat <<EOF\n$X\nEOF'
run "X=v; cat <<'EOF'\n\$X\nEOF"
run 'cat <<-EOF\n\tindented\n\tEOF'
run 'cat <<EOF\na\nEOF\ncat <<EOF2\nb\nEOF2'
run 'cat <<EOF | tr a-z A-Z\nlower\nEOF'
run 'echo start; cat <<EOF; echo end\nmiddle\nEOF'
# `exec` with no command: its redirections become the shell's own, from there to
# the end of it. The first case is the whole point of the feature — a script
# that logs everything after one line — and the last is the boundary that makes
# it hard: a redirection written on the *enclosing* compound still wins for the
# descriptors it names, so `exec 2>inner` inside `{ … } 2>outer` changes nothing
# that the group can see.
run 'exec 2> err; nosuchcommand; cat err | wc -l'
run 'exec > out; echo captured'
run 'exec 3< f.txt; cat <&3; cat <&3'
run 'exec 3< f.txt; exec 3<&-; cat <&3; echo $?'
run 'cat <&9; echo $?'
# A dup target that is not a number: dash calls it a syntax error and ends the
# shell over it, and the wording of a syntax error is ours by design — hence
# `syntax_case`, which compares the status and the output and asks only that
# both sides agreed to complain.
syntax_case 'cat <&notanumber; echo unreached'
run '{ exec 2> inner; nosuchcommand; } 2> outer; wc -l < inner; wc -l < outer'
run '( exec 2> sub_err; nosuchcommand ); nosuchcommand2 2> after_err; wc -l < sub_err; wc -l < after_err'

# === pipelines ================================================================

run 'echo one two | tr a-z A-Z'
run 'echo a | cat | cat'
run 'yes | head -3 | wc -l'
run 'true | false; echo $?'
run 'false | true; echo $?'
run '! true | false; echo $?'
# Every stage of a pipeline is a subshell, builtins included. That is what makes
# the first two cases print nothing back: the assignment happened, in a shell
# that then went away. The `cd` case is the same rule seen from further off.
run 'echo hi | read x; echo [$x]'
run 'X=outer; echo inner | { read X; }; echo [$X]'
# `ls f.txt` rather than `pwd`, because `pwd` would print the fixture directory
# and the two sides get different ones. `f.txt` is at the top and `sub` has only
# `h.txt`, so it names the cwd without printing it.
run 'cd sub | cat; ls f.txt'
run 'echo a | { echo piped; exit 3; }; echo $?'
run 'seq 1 5 | sort -r | head -1'
run 'cat f.txt | wc -l'

# === options ==================================================================

run 'set -e; false; echo unreached'
run 'set -e; false || true; echo reached'
run 'set -e; if false; then :; fi; echo reached'
run 'set -e; while false; do :; done; echo reached'
run 'set -e; ! false; echo reached'
run 'set -u; echo $UNSET; echo unreached'
run 'set -u; echo ${UNSET-ok}'
# `-n` takes effect from the command *after* the one that set it, so a shell
# that only consulted it at parse time would run everything here.
run 'set -n; echo never'
run 'echo before; set -n; echo never; echo also-never'
run 'set -n; set +n; echo still-never'
run 'set -x; echo traced'
run 'set -f; echo *.txt; set +f; echo *.txt'
run 'case $- in *f*) echo noglob;; *) echo glob;; esac'
run 'set -f; case $- in *f*) echo noglob;; *) echo glob;; esac'
run 'set -o noclobber; echo one > out; echo two > out; echo $?'

# === $0 and script operands ===================================================

run_file 'echo $0 | sed "s|.*/||"'
run_file 'echo $#'
run_file 'set -- a b; echo $@'
run_file 'exit 7'
run_file 'echo one\necho two'
run_file 'echo tail-without-newline'

run_stdin 'echo from stdin'
run_stdin 'echo a\necho b'
# The script and the commands in it share descriptor 0, so a shell that read a
# line at a time would hand `value-line` to `read` as data instead of running
# it. Slurping the whole script first is what both dash and bash do, and these
# two cases are the ones that can tell the difference.
run_stdin 'read x\necho [$x]\nvalue-line'
run_stdin 'echo start\ncat\nrest-of-script-as-data'
run_stdin 'cat <<EOF\nbody\nEOF\necho after'

# === bytes that are not text ==================================================
#
# The whole reason the rewrite works in `&[u8]`. A shell that converts a path,
# an argument or an environment value to `str` anywhere loses these.

run 'echo b?ad.txt'
run 'cat b*ad.txt'
run 'for f in b*ad.txt; do echo "[$f]"; done'
run 'X=$(printf "a\\377b"); echo "$X" | od -An -tx1'
run 'printf "a\\377b\\n" > out; read x < out; echo "$x" | od -An -tx1'

# === deliberate differences ===================================================
#
# Each of these is a feature this shell does not have, and does not have on
# purpose — see the "Deliberately absent" section of `sh.rs`'s module docs and
# `design-decisions.md`. An XPASS here means the reason has gone stale.

xfail_run 'no trap: a shell that accepted and ignored one would make a script look as though it had a cleanup handler' \
  'trap "echo bye" EXIT; echo body'
xfail_run 'no getopts' 'set -- -a; getopts a o; echo $o'
xfail_run 'no aliases: they are an interactive convenience and change how a *later* line parses' \
  'alias e=echo; e hi'
xfail_run 'no `times`' 'times | wc -l'
xfail_run 'no `readonly`' 'readonly X=1; X=2; echo $X'
xfail_run 'no `local`: dash has it as an extension and POSIX does not' \
  'f() { local x=1; }; f; echo $?'

# --- known bugs ---------------------------------------------------------------
#
# None yet. Anything this harness finds that is a defect rather than a
# deliberate omission belongs here, with an entry in `known-issues.md`.

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
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ] && [ "$kfixed" -eq 0 ]
