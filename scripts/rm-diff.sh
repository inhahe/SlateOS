#!/usr/bin/env bash
# Differential test: our `rm` against GNU coreutils'.
#
# ## What is compared
#
# Four things per case, all four of which have to agree: standard output,
# standard error, the exit status, and — the point of this harness — *what is
# left on disk*. The last is a snapshot of the case directory afterwards (every
# path with its octal mode, and its size if it is not a directory) plus the
# bytes of every surviving file. A `rm` that prints the right sentence and
# removes the wrong file passes a text-only comparison; it does not pass this
# one.
#
# That matters more here than for any other utility in this family. `rm`'s
# output is almost always empty — `-v` and the prompts are the exceptions — so
# a harness that compared only text would be comparing nothing at all for most
# of the cases below, and would report a full green column while certifying
# that two programs are equally silent.
#
# ## Prompts, without a terminal
#
# Half of `rm`'s behaviour is behind `isatty(0)`: the default mode asks before
# removing a write-protected file, but only when input is a terminal. Rather
# than drive a pty as `nohup-diff.sh` must, this uses `---presume-input-tty`,
# the (deliberately hard to type) option GNU added for exactly this and which
# ours implements. The answers arrive on stdin from `$INPUT`, which ends: a
# prompt with nothing left to read gets end-of-file, which is "no", which is
# deterministic in a way that `yes |` is not.
#
# ## What this harness will not do
#
# **No case recursively removes `/`.** The root failsafe is the whole reason
# this program was rewritten, and the obvious way to certify it — run
# `rm -rf /` and check that both sides refuse — is a test whose failure mode is
# deleting the operator's home directory, their WSL installation, and, through
# `/mnt/d`, the repository this file is in. A test that destroys the tree when
# the code under test regresses is not a test worth having at any level of
# confidence.
#
# So the failsafe is certified in three safer pieces:
#
#   * the *non-recursive* `/` cases below, which are safe whatever the code
#     does, because `unlink("/")` and `rmdir("/")` cannot succeed;
#   * the `.` and `..` refusals, which are exercised for real, but written so
#     that the operand is always *inside* the case directory (`tree/..`, never
#     a bare `..`), so a regression destroys a fixture and nothing else;
#   * `rm.rs`'s own unit test `a_recursive_operand_that_is_the_root_is_refused`,
#     which points `Rm::root` — GNU's `x.root_dev_ino` — at a scratch directory
#     and so tests the comparison itself with no `/` anywhere near it.
#
# `--one-file-system` and `--preserve-root=all` are likewise absent: both need
# a mount point to mean anything, and mounting needs privileges this harness
# does not have and should not ask for. See `known-issues.md` ->
# `TD-B-RM-ONE-FILE-SYSTEM-AND-PRESERVE-ROOT-ALL-ARE-IMPLEMENTED-BUT-UNCERTIFIED`,
# which also records the `unshare --map-root-user --mount` section that would
# close the gap.
#
# ## Why both sides run inside WSL
#
# The reasons in `cmp-diff.sh`'s header, plus one of this program's own: it
# calls `euidaccess(3)` to decide whether to say "write-protected", and the
# Windows host has neither that function nor a mode bit for it to read.
#
# ## Cases that differ on purpose
#
# Two, both the family's: `--help` omits the GNU project's `Report bugs to:`
# block, and `--version` names SlateOS.
#
# Run `OURS=/usr/bin/rm ./scripts/rm-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
set -u

DIFF_PROG='rm'
# Not the installed binary: WSL's coreutils is Ubuntu's `9.4-3ubuntu6.1` and
# carries behavioural patches, so a green run against it certifies agreement
# with Debian rather than with GNU. See `diff-wsl.sh`'s "Why a built reference"
# and `design-decisions.md` 726.
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

work=$DIFF_TMP/work
mkdir -p "$work"

case_no=0

# --- the fixture --------------------------------------------------------------
# A small tree, built in a fixed order.
#
# What makes `-v` output comparable line for line is that neither `rm` sorts:
# GNU passes a null comparison function to `fts_open`, and ours iterates
# `read_dir`, so both walk a directory in raw readdir order and get the same
# answer from the same directory.
#
# This used to say that readdir order is insertion order "on ext4 a directory
# this small". Measured on 2026-08-30, that is false — `dir_index` is on, so
# even a three-entry directory is hashed:
#
#     $ ls -f tree            $ ls -i tree
#     .. a.txt link sub .     674264 a.txt   674266 link   674263 sub
#
# — readdir gives a.txt, link, sub while insertion order was sub, a.txt, link.
# The conclusion survives and is stronger than the reasoning it had: hash order
# is a function of the *names*, so two case directories holding the same names
# enumerate identically however they were built. What would break that is a
# fixture whose two sides hold different names — a `for` loop over an unsorted
# glob, say, or a name derived from a timestamp or a pid.
#
# `cp` is the one place in this family where the sides *did* diverge, and for a
# different reason: GNU's `cp` does sort, by inode, via `savedir (…,
# SAVEDIR_SORT_FASTREAD)`. See `design-decisions.md` §725 and
# `read_dir_fastread` in `cp.rs`.
mktree() {
  mkdir -p tree/sub
  printf 'a\n' > tree/a.txt
  printf 'bb\n' > tree/sub/b.txt
}

# --- what a case leaves behind ------------------------------------------------
# Every surviving path, its octal mode, and its size — except for a directory,
# whose size is the size of the *block* holding its entries and so says nothing
# about what is in it, while varying with what used to be. `d` stands in.
#
# Errors are discarded, and are meant to be: a case that leaves an unreadable
# directory behind leaves one on both sides, and `find` then fails to descend
# on both sides, so the blind spot is symmetric. What survives it is that the
# unreadable directory itself is still listed, with its mode.
snapshot() {
  ( cd "$1" 2>/dev/null && find . -mindepth 1 \
        \( -type d -printf '%P %m d\n' -o -printf '%P %m %s\n' \) 2>/dev/null \
      | LC_ALL=C sort )
}

# And the bytes, so that a file which survived with the right size and the
# wrong contents is still caught.
contents() {
  ( cd "$1" 2>/dev/null || return 0
    find . -type f -printf '%P\n' 2>/dev/null | LC_ALL=C sort | while read -r f; do
      printf '== %s\n' "$f"
      cat -- "$f"
      printf '\n'
    done )
}

# --- knobs, reset after every case --------------------------------------------

# Shell run inside the case directory to build the fixture.
TREE=
# Answers, fed to the program's stdin through `printf '%b'`, so `\n` works.
# Empty means end-of-file at the first prompt, which is "no".
INPUT=
reset_knobs() { TREE='mktree'; INPUT=; }
reset_knobs

# The two sides run in two different directories, and a case that names an
# absolute path gets that path echoed back in the diagnostic. Comparing those
# raw would fail on the one thing that is supposed to differ. The replacement is
# per side, not of a common prefix: a path pointing anywhere other than that
# side's own directory survives and shows up as a difference.
scrub() { sed -e "s|$1|<DIR>|g"; }

# --- running one side ---------------------------------------------------------

run_one() {
  local side=$1 dir=$2 out=$3 err=$4 rcf=$5; shift 5
  mkdir -p "$dir"
  ( cd "$dir" && eval "$TREE" ) >/dev/null 2>&1
  (
    # `$out`/`$err` are absolute: they are opened after this `cd`.
    cd "$dir" || exit 1
    # Reached as the bare word `rm`, through the one-entry directory
    # `diff-wsl.sh` built, and *not* by the path of the symlink. gnulib's
    # `set_program_name` takes `argv[0]` whole, so GNU invoked as
    # `/tmp/xxx/bin/gnu/rm` prefixes every diagnostic with that entire path
    # while ours prints `rm:`, and all 124 cases that say anything at all
    # differ for a reason that has nothing to do with either program.
    # Prepended rather than replacing `PATH`, so `timeout` is still findable.
    PATH="$bindir/$side:$PATH"
    printf '%b' "$INPUT" | diff_run timeout -k 2 30 rm "$@" >"$out" 2>"$err"
  )
  echo $? >"$rcf"
  return 0
}

# --- comparing the two sides --------------------------------------------------

judge() {
  local o_dir=$1 g_dir=$2 o_out=$3 g_out=$4 o_extra=$5 g_extra=$6 label=$7
  local o_snap g_snap o_body g_body o_show g_show
  o_snap=$(snapshot "$o_dir"); g_snap=$(snapshot "$g_dir")
  o_body=$(contents "$o_dir" | scrub "$o_dir"); g_body=$(contents "$g_dir" | scrub "$g_dir")
  o_show=$(scrub "$o_dir" <"$o_out"); g_show=$(scrub "$g_dir" <"$g_out")
  o_extra=$(printf '%s' "$o_extra" | scrub "$o_dir")
  g_extra=$(printf '%s' "$g_extra" | scrub "$g_dir")

  if [ "$o_show" = "$g_show" ] && [ "$o_extra" = "$g_extra" ] \
     && [ "$o_snap" = "$g_snap" ] && [ "$o_body" = "$g_body" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: %s\n        out{%s}\n        tree{%s} files{%s}\n  gnu : %s\n        out{%s}\n        tree{%s} files{%s}' \
    "$(printf '%s' "$o_extra" | tr '\n' '|')" "$(printf '%s' "$o_show" | tr '\n' '|')" \
    "$(printf '%s' "$o_snap" | tr '\n' '|')" "$(printf '%s' "$o_body" | tr '\n' '|')" \
    "$(printf '%s' "$g_extra" | tr '\n' '|')" "$(printf '%s' "$g_show" | tr '\n' '|')" \
    "$(printf '%s' "$g_snap" | tr '\n' '|')" "$(printf '%s' "$g_body" | tr '\n' '|')")
  LABEL=$label
}

compare() {
  case_no=$((case_no+1))
  local o_dir=$work/o$case_no g_dir=$work/g$case_no
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  local o_err=$work/oe$case_no g_err=$work/ge$case_no
  local o_rc=$work/or$case_no g_rc=$work/gr$case_no
  local label="rm $*"
  [ "$TREE" = mktree ] || label="$label   [tree: $TREE]"
  [ -z "$INPUT" ] || label="$label   [in: $INPUT]"
  run_one ours "$o_dir" "$o_out" "$o_err" "$o_rc" "$@"
  run_one gnu  "$g_dir" "$g_out" "$g_err" "$g_rc" "$@"
  judge "$o_dir" "$g_dir" "$o_out" "$g_out" \
    "rc=$(cat "$o_rc") err{$(cat "$o_err")}" \
    "rc=$(cat "$g_rc") err{$(cat "$g_err")}" \
    "$label"
  reset_knobs
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$LABEL"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$LABEL" "$REPORT"
  fi
  return 0
}

run_case() { compare "$@"; report; }

# A case expected to differ, with the reason. Counted apart so that one which
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked.
xfail_case() {
  local why="$1"; shift
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s  (expected to differ: %s)\n' "$LABEL" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s  (%s)\n' "$LABEL" "$why"
  fi
  return 0
}

echo "rm-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. No operands, and who is allowed to have none
# =============================================================================
# The guard is `-f`'s *ignore-missing-files* flag and not, as it reads, the
# interactivity setting: `--interactive=never` sets the same internal state
# `-f` does and still demands an operand, while `-f --interactive=always` puts
# the interactivity back and still does not.

run_case
run_case -f
run_case --interactive=never
run_case --interactive=always
run_case -i
run_case -I
run_case -f --interactive=always
run_case --interactive=never -f
run_case -r
run_case -rf

# An empty operand is an operand: it names no file, so it is a plain failure —
# and `-f` swallows it, because the failure is "no such file".
run_case ''
run_case -f ''
run_case -rf ''

# =============================================================================
# 2. Option errors
# =============================================================================

run_case -x tree
run_case -rx tree
run_case --nope tree
run_case --recursive=x tree
run_case --help=x
run_case --version=x
run_case --=x                    # names every long option, in table order

# `--interactive` takes an optional argument, so a bad one is `argmatch`'s
# diagnostic — which lists the valid words — and an empty one is ambiguous
# rather than empty, because it is a prefix of all of them.
run_case --interactive=bogus tree
run_case --interactive= tree
run_case --interactive=n tree    # never/no/none all mean the same: not ambiguous
run_case --interactive=o tree
run_case --interactive=a tree
run_case --interactive=never tree/a.txt
run_case --interactive=no tree/a.txt
run_case --interactive=none tree/a.txt

# `--preserve-root` takes a *required* argument and validates it by hand, so
# its rejection is `rm`'s own sentence and has no `Try ... --help` referral.
run_case --preserve-root=bad tree
run_case --preserve-root= tree
run_case -r --preserve-root=all tree/a.txt

# =============================================================================
# 3. Abbreviations
# =============================================================================
# Which option a prefix resolves to. `--pre` is unambiguous only because
# `---presume-input-tty`'s long name begins with a dash.

run_case --r tree/a.txt
run_case --rec tree/a.txt
run_case --d tree/sub
run_case --v tree/a.txt
run_case --f
run_case --o tree/a.txt
# `--no-preserve-root` is the one long option that may *not* be abbreviated.
# Every prefix of it resolves unambiguously — nothing else in the table begins
# with `n` — so getopt is happy and `rm` rejects it by hand afterwards, because
# this is the switch that lets `rm -rf /` proceed. One character short still
# counts; the full spelling still works on an ordinary operand.
run_case --n tree/a.txt
run_case --no-p tree/a.txt
run_case --no-preserve tree/a.txt
run_case --no-preserve-roo tree/a.txt
run_case --no-preserve-root tree/a.txt
run_case --pre tree/a.txt
run_case --p tree/a.txt
run_case --i tree/a.txt

# =============================================================================
# 4. Removing a file
# =============================================================================

run_case tree/a.txt
run_case -v tree/a.txt
run_case tree/a.txt tree/sub/b.txt
run_case -v tree/a.txt tree/sub/b.txt
run_case nosuch
run_case -v nosuch
run_case -f nosuch
run_case -fv nosuch
# One failure does not stop the rest, and the status is 1 at the end.
run_case -v tree/a.txt nosuch tree/sub/b.txt
run_case -v nosuch tree/a.txt

# A dash is an operand, not an option, and `--` ends the options.
run_case -- -v
run_case -
TREE='mktree; printf x > -v'
run_case -- -v
TREE='mktree; printf x > -v'
run_case ./-v

# =============================================================================
# 5. A directory, and the three ways to be told no
# =============================================================================
# `rm dir` is `Is a directory`; `rm -d nonempty` is `Directory not empty`;
# only `-r` or an empty `-d` gets anywhere. None of the three prompts first,
# even under `-i` — the refusal is decided before the question would be asked.

run_case tree
run_case -v tree
run_case tree/sub
run_case -d tree
run_case -d tree/sub
TREE='mktree; mkdir tree/empty'
run_case -d tree/empty
TREE='mktree; mkdir tree/empty'
run_case -dv tree/empty
INPUT='y\n'
run_case -i tree/sub
INPUT='y\n'
run_case -i -d tree/sub
run_case -f tree/sub

# =============================================================================
# 6. Removing a tree
# =============================================================================

run_case -r tree
run_case -rv tree
run_case -R tree
run_case --recursive tree
run_case -rf tree
run_case -rv tree/sub
TREE='mktree; mkdir -p tree/x/y/z; printf f > tree/x/y/z/f'
run_case -rv tree/x
TREE='mktree; mkdir tree/e'
run_case -rv tree/e
run_case -rv nosuch
run_case -rfv nosuch

# =============================================================================
# 7. How the operand's spelling is echoed back down the tree
# =============================================================================
# gnulib's `fts` trims a run of trailing slashes on a root operand to one, but
# never to none and never touches a lone `/`; a child is the parent with *one*
# trailing slash dropped, plus `/name`. So an interior double slash survives
# all the way down into the child paths, and a trailing one survives into the
# operand's own line and into its diagnostics.

run_case -rv tree/
run_case -rv tree//
run_case -rv ./tree
run_case -rv tree//sub
run_case -rv tree/sub/
run_case -rv tree/./sub
run_case -v tree/sub/
run_case -dv tree/sub/
run_case -rv ./
run_case -r ./

# =============================================================================
# 8. The `.` and `..` refusal
# =============================================================================
# Recursive only: without `-r` the same operand gets the ordinary `Is a
# directory`, which is what stops this reading as a ban on the spelling.
#
# Every operand here stays inside the case directory — `tree/..` and not `..` —
# so that a regression in the refusal destroys a fixture rather than the
# harness's own scratch tree. See the header.

run_case -r .
run_case -rf .
run_case -rv ./
run_case -r tree/..
run_case -r tree/sub/..
run_case -rv tree/sub/../
run_case .
run_case -d .
run_case -rf tree/./
run_case -rf tree/.

# =============================================================================
# 9. The root, in the ways that are safe to try
# =============================================================================
# No `-r` anywhere in this section, on purpose: `unlink("/")` and `rmdir("/")`
# cannot succeed, so these cases are safe whatever either program does, whereas
# `rm -rf /` is safe only for as long as the failsafe works — which is the
# thing under test. The recursive half is covered by the unit test instead.

run_case /
run_case -d /
run_case -f /
run_case -v /
run_case --preserve-root=bad /
run_case --preserve-root -d /
run_case --no-preserve-root -d /

# =============================================================================
# 10. Prompting: `-i`
# =============================================================================
# The prompt goes to *stderr*, without a trailing newline, so `rm -i 2>/dev/null`
# loses the question and not the answer.

INPUT='y\n'; run_case -i tree/a.txt
INPUT='n\n'; run_case -i tree/a.txt
INPUT='\n';  run_case -i tree/a.txt
INPUT='maybe\n'; run_case -i tree/a.txt
INPUT='Y\n'; run_case -i tree/a.txt
INPUT='yes please\n'; run_case -i tree/a.txt
INPUT='ye\n'; run_case -i tree/a.txt
INPUT='';    run_case -i tree/a.txt          # end of file is "no"
INPUT='y';   run_case -i tree/a.txt          # ...and so is an unterminated line
INPUT='n\ny\n'; run_case -iv tree/a.txt tree/sub/b.txt
INPUT='y\nn\n'; run_case -iv tree/a.txt tree/sub/b.txt

# Declining is not a failure: the status stays 0.
INPUT='n\n'; run_case -i tree/a.txt
INPUT='n\nn\n'; run_case -i tree/a.txt tree/sub/b.txt

# `-i` last wins over `-f`, and `-f` last wins over `-i`.
INPUT='y\n'; run_case -fi tree/a.txt
INPUT='y\n'; run_case -if tree/a.txt
INPUT='y\n'; run_case -i --interactive=never tree/a.txt
INPUT='y\n'; run_case --interactive=never -i tree/a.txt
INPUT='y\n'; run_case -f -i nosuch
INPUT='y\n'; run_case -i -f nosuch

# =============================================================================
# 11. Prompting: the shape of the question for each kind of file
# =============================================================================
# `regular empty file` and `regular file` are two different words, and a
# symbolic link is never followed even when it points at a directory.

TREE='mktree; : > tree/zero'
INPUT='y\n'; run_case -i tree/zero
TREE='mktree; mkfifo tree/pipe'
INPUT='y\n'; run_case -i tree/pipe
TREE='mktree; ln -s sub tree/dlink'
INPUT='y\n'; run_case -i tree/dlink
TREE='mktree; ln -s sub tree/dlink'
INPUT='y\n'; run_case -ir tree/dlink
TREE='mktree; ln -s /nowhere tree/dangling'
INPUT='y\n'; run_case -i tree/dangling
TREE='mktree; ln -s /nowhere tree/dangling'
run_case -v tree/dangling
TREE='mktree; mkdir tree/e'
INPUT='y\n'; run_case -i -d tree/e
TREE='mktree; mkdir tree/e'
INPUT='n\n'; run_case -i -d tree/e

# =============================================================================
# 12. Prompting: descending
# =============================================================================
# A non-empty directory is two questions — descend, then remove — with the
# children in between; an empty one is a single `remove`. Declining the descend
# abandons the whole subtree *in silence* and is still a success; declining one
# child leaves the parent to fail its `rmdir` with `Directory not empty`, which
# is not silent and is not a success.

INPUT='y\ny\ny\ny\ny\ny\n'; run_case -i -rv tree
INPUT='n\n';                run_case -i -rv tree
INPUT='y\nn\ny\ny\ny\n';    run_case -i -rv tree
INPUT='y\ny\nn\ny\ny\n';    run_case -i -rv tree
INPUT='y\ny\ny\nn\ny\n';    run_case -i -rv tree
TREE='mktree; mkdir tree/e'
INPUT='y\ny\ny\ny\ny\ny\ny\n'; run_case -i -rv tree
TREE='mktree; mkdir tree/e'
INPUT='y\nn\ny\ny\ny\ny\n';    run_case -i -rv tree

# =============================================================================
# 13. Prompting: `-I` and `--interactive=once`
# =============================================================================
# One question for the whole command line, asked when there are more than three
# operands or when `-r` is in force and there is at least one.

INPUT='y\n'; run_case -I tree/a.txt
INPUT='y\n'; run_case -Iv tree/a.txt tree/sub/b.txt
TREE='mktree; printf 1 > f1; printf 2 > f2; printf 3 > f3; printf 4 > f4'
INPUT='y\n'; run_case -Iv f1 f2 f3
TREE='mktree; printf 1 > f1; printf 2 > f2; printf 3 > f3; printf 4 > f4'
INPUT='y\n'; run_case -Iv f1 f2 f3 f4
TREE='mktree; printf 1 > f1; printf 2 > f2; printf 3 > f3; printf 4 > f4'
INPUT='n\n'; run_case -Iv f1 f2 f3 f4
INPUT='y\n'; run_case -I -rv tree
INPUT='n\n'; run_case -I -rv tree
INPUT='y\n'; run_case --interactive=once -rv tree
INPUT='n\n'; run_case --interactive=once -rv tree
INPUT='y\n'; run_case -I -rv                       # no operands: no question
INPUT='y\n'; run_case -I -rv nosuch
# `-I` then `-i` is `-i`: the once-prompt is not asked and every file is.
INPUT='y\ny\ny\n'; run_case -I -i -rv tree
INPUT='y\ny\ny\n'; run_case -i -I -rv tree

# =============================================================================
# 14. The write-protected prompt, which needs no `-i` at all
# =============================================================================
# The default mode asks about a file it cannot write — but only when input is a
# terminal, which is what `---presume-input-tty` asserts. Without it the same
# fixture removes silently, and that pair is the whole of the rule.

TREE='mktree; printf p > tree/ro; chmod 400 tree/ro'
run_case -v tree/ro
TREE='mktree; printf p > tree/ro; chmod 400 tree/ro'
INPUT='y\n'; run_case ---presume-input-tty -v tree/ro
TREE='mktree; printf p > tree/ro; chmod 400 tree/ro'
INPUT='n\n'; run_case ---presume-input-tty -v tree/ro
TREE='mktree; printf p > tree/ro; chmod 400 tree/ro'
INPUT='y\n'; run_case ---presume-input-tty -f -v tree/ro
TREE='mktree; printf p > tree/ro; chmod 400 tree/ro'
INPUT='y\n'; run_case ---presume-input-tty -i -v tree/ro
# A write-protected *directory* gets the word too, in both questions.
TREE='mktree; chmod 500 tree/sub'
INPUT='y\ny\ny\ny\n'; run_case -i -rv tree
TREE='mktree; chmod 500 tree/sub'
INPUT='y\ny\ny\ny\n'; run_case ---presume-input-tty -rv tree
TREE='mktree; mkdir tree/roe; chmod 500 tree/roe'
INPUT='y\n'; run_case ---presume-input-tty -d tree/roe
TREE='mktree; mkdir tree/roe; chmod 500 tree/roe'
INPUT='y\n'; run_case -i -d tree/roe
# Two dashes is not the option; it is an unknown one.
run_case --presume-input-tty tree/a.txt

# =============================================================================
# 15. Directories that cannot be read or emptied
# =============================================================================
# A *non-empty* unreadable directory under `-r` is reported *without* a prompt,
# because the read is attempted before the question is asked; its ancestors are
# then abandoned in silence, with the status already earned. An unremovable
# child is the same shape one level down.

TREE='mktree; chmod 000 tree/sub'
run_case -rv tree
TREE='mktree; chmod 000 tree/sub'
INPUT='y\ny\ny\ny\n'; run_case -i -rv tree
TREE='mktree; chmod 000 tree/sub'
INPUT='y\ny\ny\ny\n'; run_case ---presume-input-tty -rv tree
TREE='mktree; chmod 000 tree/sub'
run_case -rfv tree
TREE='mktree; chmod 000 tree/sub'
run_case -rv tree/sub
TREE='mktree; chmod 000 tree/sub'
run_case tree/sub
TREE='mktree; chmod 000 tree/sub'
run_case -d tree/sub
TREE='mktree; chmod 500 tree/sub'
run_case -rv tree
TREE='mktree; chmod 500 tree/sub'
run_case -rfv tree

# An *empty* unreadable directory is the other half, and the half every case
# above misses: `tree/sub` holds `b.txt`, so GNU's `rmdir` fails too and prints
# the same substituted `Permission denied` — which means a remover that never
# attempted the `rmdir` at all looks identical here. It is not identical.
# Reading a directory needs `r`, while removing an empty one needs only `w`+`x`
# on its *parent*, so `chmod 300 d` is a directory nobody can list and anybody
# can delete, and GNU deletes it. The read failure is therefore *held*, not
# reported, and becomes the diagnostic only if the `rmdir` also fails with an
# errno that says less than it did (`remove.c:420`, shared here as
# `coreutils::remove::blame`).
#
# The two flags part company because `-r` has a question it cannot ask.
# Descend-or-remove is not decidable without the listing, so a prompt that
# comes due under `-r` is fatal — but with no prompt due, the `rmdir` still
# runs. Under `-d` there is nothing to descend into, so the question *can* be
# asked, and upstream asks a third sentence for it.
#
# These cases exist because their absence let a real divergence live: see
# `known-issues.md` → `TD-B-TWO-RECURSIVE-REMOVERS-NOW-EXIST-IN-COREUTILS`.

TREE='mkdir d; chmod 300 d'
run_case -rv d
TREE='mkdir d; chmod 300 d'
run_case -rfv d
TREE='mkdir d; chmod 300 d'
INPUT='y\n'; run_case -i -rv d
TREE='mkdir d; chmod 300 d'
INPUT='y\n'; run_case -I -rv d
# Writable, so no question comes due even on a tty, and it goes.
TREE='mkdir d; chmod 300 d'
run_case ---presume-input-tty -rv d
# Unwritable, so the write-protected question comes due — and under `-r` that
# is the fatal case, because it is the question that cannot be worded.
TREE='mkdir d; chmod 100 d'
run_case ---presume-input-tty -rv d
TREE='mkdir d; chmod 000 d'
INPUT='y\n'; run_case -i -rv d
# Readable but write-protected: the listing succeeds, so this is the ordinary
# write-protected route and not this rule at all. Here to prove the boundary.
TREE='mkdir d; chmod 500 d'
INPUT='y\n'; run_case ---presume-input-tty -rv d

# Under `-d`: `rm: attempt removal of inaccessible directory 'd'? `, the third
# of upstream's three prompts, which no other case in this file reaches.
TREE='mkdir d; chmod 300 d'
INPUT='y\n'; run_case -d -iv d
TREE='mkdir d; chmod 100 d'
INPUT='y\n'; run_case -d -iv d
TREE='mkdir d; chmod 000 d'
INPUT='y\n'; run_case -d -iv d
# Declined leaves the directory and still exits 0: the answer was obeyed, not
# failed. That is what makes it different from the `-r` refusals above.
TREE='mkdir d; chmod 300 d'
INPUT='n\n'; run_case -d -iv d
# Writable, so nothing is asked and it goes silently...
TREE='mkdir d; chmod 300 d'
run_case ---presume-input-tty -dv d
# ...while unwritable reaches the same prompt by the write-protected route,
# which is what shows the sentence replaces the wording rather than adding a
# question.
TREE='mkdir d; chmod 100 d'
INPUT='y\n'; run_case ---presume-input-tty -dv d
# Not empty, so the `rmdir` answers `ENOTEMPTY` — uninformative, being the
# mechanical consequence of the entry that could not be listed — and the held
# read error is printed in its place.
TREE='mkdir -p d/sub; printf x > d/sub/b.txt; chmod 300 d'
INPUT='y\n'; run_case -d -iv d

# =============================================================================
# 16. Bytes that are not valid UTF-8
# =============================================================================
# A name is a byte string. The quoting of one in a diagnostic and in a prompt
# is `quotearg`'s, which escapes what it cannot print.

TREE='mktree; printf x > "$(printf "na\377me")"'
run_case -v "$(printf 'na\377me')"
TREE='mktree; printf x > "$(printf "na\377me")"'
INPUT='y\n'; run_case -iv "$(printf 'na\377me')"
TREE='mktree; printf x > "tree/$(printf "na\377me")"'
run_case -rv tree
run_case -v "$(printf 'na\377me')"
run_case -v "$(printf '\377')"

# A name with a newline, a quote, or a space in it — the other half of what
# `quotearg` is for, and the half that changes which quoting style it picks.
TREE='mktree; printf x > "tree/two words"'
run_case -rv tree
# A name holding a single quote is the one case that comes out in *double*
# quotes — `removed "tree/it's"` — and it only does so when every other byte in
# it is double-quote-safe. The path separator is such a byte, which our table
# used to deny; this pair of cases is what caught it. The `$` in the second
# name is not, so that one falls back to `'tree/it'\''s$'`.
TREE='mktree; printf x > "tree/it'"'"'s"'
run_case -rv tree
TREE='mktree; printf x > "tree/it'"'"'s\$"'
run_case -rv tree
TREE='mktree; printf x > "$(printf "tree/a\nb")"'
run_case -rv tree

# =============================================================================
# 17. --help and --version
# =============================================================================

xfail_case 'help omits GNU bug-report block' --help
xfail_case 'version names SlateOS' --version

# The wording is the family's, not this harness's own: `scripts/all-diff.sh`
# decides green by matching " 0 differed" in the tail line, so a summary that
# said "0 failed" would be reported as a failing harness forever.
printf '\nrm: %d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
exit 0
