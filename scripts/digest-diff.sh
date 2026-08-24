#!/usr/bin/env bash
# Differential test: our md5sum/sha256sum against GNU coreutils'.
#
# ## One harness, two programs
#
# Upstream's `src/digest.c` is one file compiled eight times, differing only in
# which `HASH_ALGO_*` is defined; ours is one module parameterised by an
# `Algorithm` constant. So there is one harness, and it runs the whole case list
# once per program:
#
#     ./scripts/digest-diff.sh                 # md5sum, then sha256sum
#     PROG=sha256sum ./scripts/digest-diff.sh  # just that one
#
# Nothing here hard-codes a digest width. The two places that would have —
# a deliberately-wrong checksum, and a digest of the wrong length — derive
# theirs from the reference binary's own output at startup, so adding
# `sha1sum` later is one word in `PROGS` rather than a rewrite.
#
# ## Why the checksum files are built per side
#
# `--check` reads back output this same program wrote, so a checksum file is a
# format with two implementations inside one binary. Building the fixture with
# the side's *own* binary (`SETUP` runs under that side's `PATH`) tests the
# round trip; building it with `printf` tests cross-verification, since both
# sides then read byte-identical input. Both kinds appear below, and the
# distinction matters: a program whose writer and reader were wrong in the same
# direction would pass every round-trip case and fail every literal one.
#
# The case directories are also compared afterwards, as in `tee-diff.sh`.
# Neither side is supposed to write anything, and that is worth checking rather
# than assuming.
#
# ## Why both sides run inside WSL
#
# The same reasons as `cmp-diff.sh` and `tee-diff.sh`, whose headers spell them
# out: the fixtures include names that are not valid UTF-16 and so cannot exist
# on Windows at all, and a Linux build sharing the repository's `target/` with
# the Windows one would make each invalidate the other. The build lands in
# `$HOME/.cache/slateos-diff-target` inside WSL, shared with the other
# harnesses (`design-decisions.md` §374).
#
# ## Cases that differ on purpose
#
# Only one kind, recorded as `xfail`: `--help` omits the GNU project's `Report
# bugs to:` block and `--version` names SlateOS, as everywhere here.
#
# Note what is *not* on that list. `-b` and `-t` are genuine no-ops on both
# sides for these algorithms — gnulib's `O_BINARY` is 0 on POSIX, so the flags
# only choose the ` `/`*` indicator byte — so they are ordinary passes rather
# than a documented difference. And every diagnostic that names a file goes
# through `quotef`, which is what our `quotef_os` is, so the quoting cases pass
# outright.
#
# Run `PROG=md5sum OURS=/usr/bin/md5sum ./scripts/digest-diff.sh` to confirm the
# harness still discriminates: it should report every xfail as XPASS and nothing
# else. `OURS` replaces one binary, so it requires a `PROG` to say which.
set -u

export MSYS2_ARG_CONV_EXCL='*'

# With `PROG` unset, both programs run — `scripts/all-diff.sh` reaches this
# harness through a glob and cannot pass one, and a harness that silently
# covered half of what it names is worse than no harness. `OURS` is a subject
# override for a single binary, so setting it without a `PROG` to apply it to
# is an error rather than a coin flip.
PROGS=${PROG:-md5sum sha256sum}
if [ -n "${OURS:-}" ] && [ -z "${PROG:-}" ]; then
  echo "digest-diff: OURS names one binary, so PROG must say which" >&2
  exit 2
fi

# Into WSL and build the family for Linux. See `scripts/diff-wsl.sh`.
#
# `DIFF_NO_REF` and `DIFF_NO_BINDIR`: this harness compares a *family*, so the
# reference and the one-name-for-both-sides symlinks are per program and are
# made inside the loop below rather than once up front. The one `cargo build`
# covers every program in the run, rather than one per program: a second
# invocation would be a no-op, but it would still print its own `Finished` line
# and make the output read as though something were rebuilt between the two
# halves of the run.
#
# The fixed UTF-8 locale is load-bearing: getopt renders an unknown or
# ambiguous option with directional single quotes under a UTF-8 locale and
# ASCII apostrophes under `C`, so the whole option-error family would disagree
# for a reason that has nothing to do with this program.
DIFF_PROG=digest
DIFF_BINS=$PROGS
DIFF_FORWARD=PROG
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

scratch=$DIFF_TMP/scratch
bindir=$DIFF_TMP/bin
mkdir -p "$scratch" "$bindir"

total_pass=0; total_fail=0; total_xfail=0; total_xpass=0
ran=

# --- knobs, reset before every case ------------------------------------------
# `SETUP` is shell run inside the case directory on each side, under that side's
# `PATH`, before the program. `STDIN` is a `printf %b` string; `STDIN_FILE`
# names a file in the case directory to redirect from instead.
#
# `SKIP_STDOUT` drops stdout from the comparison, leaving the status, stderr and
# the directory. One family needs it: `--h` and `--v` are probes of whether an
# abbreviation resolves, but what they resolve *to* is `--help`/`--version`,
# whose text differs on purpose. Marking them `xfail` would throw away the thing
# they test, since an xfail passes however the two differ — including if ours
# had rejected the abbreviation as ambiguous.
SETUP=; STDIN=; STDIN_FILE=; SKIP_STDOUT=

reset_knobs() { SETUP=; STDIN=; STDIN_FILE=; SKIP_STDOUT=; }

# --- what a directory looks like afterwards ----------------------------------
render() {
  local f=$1 sz
  # `stat`, not `wc -c <"$f"`: a mode-000 fixture makes the *shell* print
  # `Permission denied` on its own stderr, where no redirection on the inner
  # command can reach it.
  sz=$(stat -c %s "$f" 2>/dev/null) || { printf '<unstattable>\n'; return 0; }
  printf '%s bytes\n' "$sz"
  if [ ! -r "$f" ]; then printf '  <unreadable>\n'
  elif [ "$sz" -le 4096 ]; then od -An -c <"$f"
  else cksum <"$f"
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
    # SETUP under the side's own PATH, so a fixture built by running the
    # program is built by *that* side's program. The assignment is a statement
    # rather than a prefix on `eval`: `eval` is a special builtin, and a prefix
    # assignment on one of those has different persistence rules between
    # shells — here it would decide nothing, but only by accident.
    ( cd "$dir" && PATH="$progbin/$side:$PATH" && eval "$SETUP" ) >/dev/null 2>&1
    if [ -n "$STDIN_FILE" ]; then
      ( cd "$dir" && timeout -k 2 60 env PATH="$progbin/$side" "$PROG" "$@" \
          <"$STDIN_FILE" >"$out" 2>"$err" )
    else
      ( cd "$dir" && printf '%b' "$STDIN" \
          | timeout -k 2 60 env PATH="$progbin/$side" "$PROG" "$@" >"$out" 2>"$err" )
    fi
    # Into `rc` on the very next line, before anything else runs. Writing
    # `if [ "$side" = ours ]; then o_rc=$?; ...` instead reads `$?` of the
    # *test*, which is 0 for `ours` and 1 for `gnu` every single time — an
    # exit status the harness manufactured itself, agreeing with the truth
    # exactly when the truth happened to be 0 and 1. It cost a full run of
    # tee-diff.sh, and the note is repeated here for the next author.
    rc=$?
    if [ "$side" = ours ]; then o_rc=$rc; else g_rc=$rc; fi
  done

  # stdout via a file, not a pipe: in `x=$(md5sum | od)` the recorded status
  # would be od's.
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

run_case() { compare "$@"; report "$PROG $*"; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$PROG $*" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$PROG $*" "$why"
  fi
  return 0
}

# =============================================================================
# The whole case list, once per program
# =============================================================================
# Everything below runs against whichever program is being examined; `PROG`,
# `ours`, `gnu_real` and the counters are set here rather than at the top of the
# file because the same list is run twice. The counters are deliberately globals
# and not `local`s: `report` and `xfail_case` increment them from a nested
# frame, which under bash's dynamic scoping would work either way, but under a
# shell without `local` at all would silently count nothing.
run_program() {
  PROG=$1
  pass=0; fail=0; xfail=0; xpass=0

  gnu_real=$(command -v "$PROG") || gnu_real=
  if [ -z "$gnu_real" ]; then
    echo "digest-diff: no GNU $PROG inside WSL; skipping it"
    return 0
  fi

  ours=${OURS:-$(diff_ours "$PROG")}
  if [ ! -x "$ours" ]; then
    echo "digest-diff: $ours is not executable" >&2
    return 1
  fi
  case $ours in
    /*) ;;
    *) ours=$(cd "$(dirname "$ours")" && pwd)/$(basename "$ours") ;;
  esac

  # --- one name for both sides -----------------------------------------------
  # Each binary is reached through a symlink named after the program, in a
  # directory that is the whole of `PATH` for that one invocation, so `argv[0]`
  # is the bare word on both sides and the `md5sum: ` prefix on every diagnostic
  # matches. Per program, because `bindir` outlives the loop.
  progbin=$bindir/$PROG
  mkdir -p "$progbin/ours" "$progbin/gnu"
  ln -s "$ours" "$progbin/ours/$PROG"
  ln -s "$gnu_real" "$progbin/gnu/$PROG"

  # --- widths, taken from the reference rather than assumed -------------------
  # A digest of all zeros is a checksum that is wrong for every input, and one
  # hex digit shorter is a line that cannot parse. Both are derived from what
  # the reference prints for empty input, so nothing here knows how wide a
  # digest is.
  empty_digest=$("$gnu_real" </dev/null | cut -d' ' -f1)
  zeros=$(printf '%s' "$empty_digest" | tr '0-9a-f' '0')
  shortd=${zeros%?}

  echo "digest-diff: $PROG"
  echo "  ours: $ours"
  echo "  gnu:  $gnu_real"

# =============================================================================
# 1. Plain output
# =============================================================================

STDIN='hello\n'; run_case
STDIN='hello\n'; run_case -
STDIN=''; run_case                      # empty stdin
STDIN='no trailing newline'; run_case

SETUP='printf "hello\n" > a'; run_case a
SETUP='printf "hello\n" > a'; run_case -b a
SETUP='printf "hello\n" > a'; run_case -t a
SETUP='printf "hello\n" > a'; run_case --binary a
SETUP='printf "hello\n" > a'; run_case --text a
SETUP='printf "hello\n" > a'; run_case --tag a
SETUP='printf "hello\n" > a'; run_case -z a
SETUP='printf "hello\n" > a'; run_case --tag -z a
SETUP='printf "hello\n" > a'; run_case --zero --tag a

# `-b` then `-t` and back: the last one wins, and `--tag` after `-t` is not the
# error that `-t` after `--tag` is.
SETUP='printf "hello\n" > a'; run_case -b -t a
SETUP='printf "hello\n" > a'; run_case -t -b a
SETUP='printf "hello\n" > a'; run_case --text --tag a

SETUP='printf "one\n" > a; printf "two\n" > b'; run_case a b
SETUP='printf "one\n" > a; printf "two\n" > b'; run_case --tag a b
SETUP='printf "one\n" > a; printf "two\n" > b'; run_case -z a b
SETUP='printf "one\n" > a'; STDIN='piped\n'; run_case a - a

# An empty file, and one larger than any single read.
SETUP=': > empty'; run_case empty
SETUP='head -c 300000 /dev/zero > big'; run_case big
SETUP='head -c 65536 /dev/zero > exact'; run_case exact   # exactly one read
SETUP='head -c 65537 /dev/zero > over'; run_case over

# =============================================================================
# 2. Files that cannot be read
# =============================================================================

run_case nosuchfile
run_case nosuchfile --tag
SETUP='mkdir d'; run_case d
SETUP='printf "x\n" > np; chmod 000 np'; run_case np
SETUP='printf "ok\n" > a; chmod 000 a; printf "b\n" > b'; run_case a b
run_case /dev/null

# =============================================================================
# 3. Names that need escaping
# =============================================================================
# `\n` -> `\n`, `\r` -> `\r`, `\` -> `\\`, with a single leading backslash on
# the record to announce that it happened. `-z` turns escaping off entirely.

SETUP='printf "x\n" > "$(printf "we\\nird")"'
run_case "$(printf 'we\nird')"
SETUP='printf "x\n" > "$(printf "we\\nird")"'
run_case -z "$(printf 'we\nird')"
SETUP='printf "x\n" > "$(printf "we\\nird")"'
run_case --tag "$(printf 'we\nird')"

SETUP='printf "y\n" > "back\\slash"'; run_case 'back\slash'
SETUP='printf "y\n" > "back\\slash"'; run_case -z 'back\slash'
SETUP='printf "y\n" > "back\\slash"'; run_case --tag 'back\slash'

SETUP='printf "z\n" > "$(printf "c\\rr")"'; run_case "$(printf 'c\rr')"

# A name that is already an escape sequence: `\n` as two literal characters
# must come back as `\\n`, not as a newline.
SETUP='printf "q\n" > "\\n"'; run_case '\n'

# A name that looks like an option, reached both ways.
SETUP='printf "e\n" > ./-b'; run_case -- -b
SETUP='printf "e\n" > ./-b'; run_case ./-b

# A name that is not valid UTF-8 at all — the reason this whole conversion
# happened. The previous version panicked here before printing anything.
SETUP='printf "u\n" > "$(printf "na\\377me")"'
run_case "$(printf 'na\377me')"
SETUP='printf "u\n" > "$(printf "na\\377me")"'
run_case --tag "$(printf 'na\377me')"
SETUP='printf "u\n" > "$(printf "na\\377me")"'
run_case -z "$(printf 'na\377me')"

# =============================================================================
# 4. Option errors and abbreviations
# =============================================================================

run_case -Z
run_case --nope
run_case --=x                 # names every long option, in table order
run_case --s                  # ambiguous: status, strict
run_case --st                 # ambiguous: status, strict
run_case --t                  # ambiguous: tag, text
run_case --che                # unique prefix of --check
run_case --tag=x              # takes no argument
run_case --binar              # unique prefix
SKIP_STDOUT=1; run_case --h
SKIP_STDOUT=1; run_case --v

# The nine validation rules, in upstream's order.
run_case --tag --text
run_case --tag -c
run_case -z -c
run_case -b -c
run_case --ignore-missing
run_case --status
run_case --warn
run_case --quiet
run_case --strict

xfail_case 'help omits GNU bug-report block' --help
xfail_case 'version names SlateOS' --version

# =============================================================================
# 5. --check: the round trip
# =============================================================================

sums='printf "one\n" > a; printf "two\n" > b; $PROG a b > SUMS'
tsums='printf "one\n" > a; printf "two\n" > b; $PROG --tag a b > TSUMS'

SETUP="$sums"; run_case -c SUMS
SETUP="$sums"; run_case --check SUMS
SETUP="$tsums"; run_case -c TSUMS
SETUP="$sums"; run_case -c --quiet SUMS
SETUP="$sums"; run_case -c --status SUMS
SETUP="$sums"; run_case -c --strict SUMS
SETUP="$sums"; run_case -c -w SUMS

# Through stdin, which is also the name the diagnostics use.
SETUP="$sums"; STDIN_FILE=SUMS; run_case -c
SETUP="$sums"; STDIN_FILE=SUMS; run_case -c -

# The escaped round trip: written escaped, read back unescaped, verified.
SETUP='printf "x\n" > "$(printf "we\\nird")"; printf "y\n" > "back\\slash"; $PROG -- * > SUMS 2>/dev/null'
run_case -c SUMS

# =============================================================================
# 6. --check: failures
# =============================================================================

SETUP="printf 'bad\n' > b; printf '%s  b\n' $zeros > BAD"
run_case -c BAD
SETUP="printf 'bad\n' > b; printf '%s  b\n' $zeros > BAD"
run_case -c --status BAD
SETUP="printf 'bad\n' > b; printf '%s  b\n' $zeros > BAD"
run_case -c --quiet BAD
SETUP="printf 'bad\n' > b; printf '%s  b\n' $zeros > BAD"
run_case -c -w BAD

# A named file that is not there, with and without --ignore-missing.
SETUP="printf '%s  gone\n' $zeros > GONE"
run_case -c GONE
SETUP="printf '%s  gone\n' $zeros > GONE"
run_case -c --ignore-missing GONE
SETUP="printf '%s  gone\n' $zeros > GONE"
run_case -c --ignore-missing --status GONE

# Some present, some not.
SETUP="printf 'one\n' > a; \$PROG a > SUMS; printf '%s  gone\n' $zeros >> SUMS"
run_case -c SUMS
SETUP="printf 'one\n' > a; \$PROG a > SUMS; printf '%s  gone\n' $zeros >> SUMS"
run_case -c --ignore-missing SUMS

# A named file that exists but cannot be read.
SETUP="printf 'x\n' > np; chmod 000 np; printf '%s  np\n' $zeros > NP"
run_case -c NP

# A named file that is a directory.
SETUP="mkdir d; printf '%s  d\n' $zeros > D"
run_case -c D

# =============================================================================
# 7. --check: lines that are not checksum lines
# =============================================================================

SETUP='printf "garbage line\n" > MIS'; run_case -c MIS
SETUP='printf "garbage line\n" > MIS'; run_case -c -w MIS
SETUP='printf "garbage line\n" > MIS'; run_case -c --strict MIS
SETUP='printf "garbage line\n" > MIS'; run_case -c -w --strict MIS
SETUP='printf "garbage line\n" > MIS'; run_case -c --status MIS

SETUP="$sums"' ; printf "garbage\n" >> SUMS'; run_case -c SUMS
SETUP="$sums"' ; printf "garbage\n" >> SUMS'; run_case -c -w --strict SUMS

SETUP=': > EMPTY'; run_case -c EMPTY
SETUP='printf "\n\n" > BLANK'; run_case -c BLANK
SETUP='printf "# comment\n\n" > CMT'; run_case -c CMT

# Digests that are the wrong shape.
SETUP="printf '%s  a\n' $shortd > SHORT; printf 'one\n' > a"
run_case -c SHORT
SETUP="printf '%szz  a\n' $shortd > NONHEX; printf 'one\n' > a"
run_case -c NONHEX
SETUP="printf '%s\n' $zeros > NONAME"
run_case -c NONAME
SETUP="printf '%s  \n' $zeros > BLANKNAME"
run_case -c BLANKNAME

# An uppercase digest still matches.
SETUP='printf "one\n" > a; $PROG a | tr "a-f" "A-F" > UP'
run_case -c UP

# A checksum file written on Windows.
SETUP="$sums"' ; sed -i "s/\$/\r/" SUMS'; run_case -c SUMS

# The check file itself is missing, or a directory.
run_case -c nosuchsums
SETUP='mkdir d'; run_case -c d

# Two check files at once, one good and one not.
SETUP="$sums"' ; printf "garbage\n" > MIS'; run_case -c SUMS MIS

# =============================================================================
# 8. --check: the BSD-reversed layout, and the rule against mixing
# =============================================================================
# `<hex> NAME` with one space and no indicator byte. It may not be mixed with
# the standard layout inside one file, because a reversed line whose name
# begins with a space or `*` would otherwise parse as a standard line naming a
# different file.

SETUP='printf "one\n" > a; printf "%s a\n" "$($PROG a | cut -d" " -f1)" > REV'
run_case -c REV
SETUP='printf "one\n" > a; printf "two\n" > b;
       printf "%s a\n" "$($PROG a | cut -d" " -f1)" > MIX;
       $PROG b >> MIX'
run_case -c MIX
SETUP='printf "one\n" > a; printf "two\n" > b;
       $PROG a > MIX;
       printf "%s b\n" "$($PROG b | cut -d" " -f1)" >> MIX'
run_case -c MIX
SETUP='printf "one\n" > a; printf "two\n" > b;
       $PROG a > MIX;
       printf "%s b\n" "$($PROG b | cut -d" " -f1)" >> MIX'
run_case -c -w MIX

# Leading blanks before the digest are skipped; trailing blanks belong to the
# name, so `a  ` names a file that is not `a`.
SETUP='printf "one\n" > a; { printf "   "; $PROG a; } > LEAD'
run_case -c LEAD
SETUP='printf "one\n" > a; $PROG a | sed "s/\$/  /" > TRAIL'
run_case -c TRAIL

# A tagged line whose tag is the wrong algorithm.
SETUP="printf 'one\n' > a; printf 'NOPE (a) = %s\n' $zeros > WRONGTAG"
run_case -c WRONGTAG
# A tagged line whose name contains a `)`.
SETUP='printf "one\n" > "p)q"; $PROG --tag "p)q" > TP'
run_case -c TP

# =============================================================================
# 9. Operands after --check that are not check files, and vice versa
# =============================================================================

SETUP="$sums"; run_case -c SUMS SUMS      # the same file twice
SETUP='printf "one\n" > a'; run_case -c a  # a data file read as a check file

  printf '  %s: %d passed, %d differed, %d differ on purpose\n' \
    "$PROG" "$pass" "$fail" "$xfail"
  total_pass=$((total_pass+pass)); total_fail=$((total_fail+fail))
  total_xfail=$((total_xfail+xfail)); total_xpass=$((total_xpass+xpass))
  ran="${ran:+$ran }$PROG"
  return 0
}

for p in $PROGS; do
  run_program "$p" || exit 1
done

# One tail line for the whole run, in the family's wording rather than this
# harness's own: `scripts/all-diff.sh` reads only `tail -1` and decides green by
# matching " 0 differed", so a summary that said "0 failed" — or that left the
# last program's own line as the tail while an earlier one had differed — would
# misreport the harness.
printf '\n%s: %d passed, %d differed, %d differ on purpose' \
  "${ran:-nothing}" "$total_pass" "$total_fail" "$total_xfail"
[ "$total_xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$total_xpass"
printf '\n'
[ "$total_fail" -eq 0 ] || exit 1
exit 0
