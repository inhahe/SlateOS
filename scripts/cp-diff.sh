#!/usr/bin/env bash
# Differential test: our `cp` against GNU coreutils'.
#
# ## What is compared
#
# Four things per case, all four of which have to agree: standard output,
# standard error, the exit status, and *what the two directories hold
# afterwards* — every surviving path with its octal mode and size, every
# symlink with the text it points at, and the bytes of every regular file.
#
# The last two matter more for `cp` than for anything else in this family,
# because `cp`'s whole observable effect is the tree it leaves and its stdout
# is empty in every case below. A text-only harness would compare two empty
# strings roughly ninety times and report a green column while certifying
# nothing. It is also the only way to see the three defects this program was
# rewritten for (module docs, bugs 1–3): whether a symlink was *cloned* or
# *followed* is a difference in the `-> target` column and nowhere else, and
# whether a copied directory came out 0700 or 0755 is a difference in the mode
# column and nowhere else. Both are invisible in the text.
#
# Timestamps are compared only in the `-p` section. Without `-p` both programs
# give the destination the time of the copy, so the column would differ between
# two runs of the same program and say nothing about either; with `-p` it is
# the one thing the option is for, and those cases pin their fixture's times
# with `touch -d` so the expected value is a constant. The knob is `STAMPS`.
#
# ## What this harness will not do
#
# **No case names a bare `..`, and no case uses `..` as a destination.** A
# recursive copy whose destination lies outside the case directory writes into
# the harness's own scratch tree, and one whose destination is an ancestor of
# its source is exactly the unbounded self-copy that bug 2 was about. The
# sources ending in `..` below are always written `tree/..`, which resolves to
# the case directory itself — so the worst a regression can do is fill that one
# directory, and the `timeout` in [`run_one`] bounds even that. Both programs
# are expected to refuse these outright, ours in `compute_target` and GNU in
# its `src_info`/`dest_info` cycle check; the case exists to certify that they
# still do.
#
# **No case copies a FIFO or a device.** Reading a FIFO that nothing writes to
# blocks until the timeout fires, which would cost thirty seconds per side and
# certify a hang rather than a copy. `--recursive`'s handling of special files
# is worth testing and will be, through `mknod` under a user namespace, once
# there is an option that acts on them.
#
# ## Why both sides run inside WSL
#
# The reasons in `cmp-diff.sh`'s header, plus two of this program's own: the
# mode bits it carries over do not exist on a Windows host, and neither does
# `symlink(2)` without a privilege the harness must not ask for. Ours refuses
# to clone a symlink at all off Unix — see `clone_symlink`'s `#[cfg(not(unix))]`
# arm — so half of these cases would be measuring the stub.
#
# ## Cases that differ on purpose
#
# Two kinds. The family's two — `--help` omits the GNU project's `Report bugs
# to:` block, and `--version` names SlateOS — and then one per option that GNU
# has and this `cp` has not. That second group is an inventory, not a
# permission: each entry names the option, and `xfail_case` reports an XPASS
# the moment one starts agreeing, which is what will force it to be promoted to
# a real case as the option lands. `cp.rs`'s module docs explain why those
# options are *refused* rather than ignored.
#
# ## The reference is built, not found
#
# `DIFF_GNU_SOURCE=9.4` below makes `diff-wsl.sh` fetch and build coreutils
# 9.4 rather than compare against `/usr/bin/cp`. This program is the reason
# that machinery exists. Ubuntu's coreutils carries
# `debian/patches/cp-n.diff`, which makes `cp -n` a silent success on an
# existing destination; upstream prints `cp: not replacing 'b'` and exits 1.
# Certifying `-n` against the installed binary would certify us into Debian's
# behaviour and away from the specification, and it would look green while
# doing it. See `diff-wsl.sh`'s "Why a built reference" and
# `design-decisions.md` §726.
#
# The version is the *same* one Ubuntu ships, on purpose. Pinning to 9.4
# rather than the newest release means that when a case's result changes, the
# de-patching is the only thing it can be attributable to -- upstream
# behaviour drift between releases is held constant. (`ls-diff.sh` pins 9.5
# instead, because its cases were written against 9.5's manual from the
# start.) Raising this later is a deliberate act with its own diff.
#
# Run `OURS=/usr/bin/cp ./scripts/cp-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else.
# Note that with a built reference that check now compares Ubuntu's `cp`
# against upstream's, so `-n` cases -- once they exist -- will legitimately
# differ rather than XPASS.
set -u

DIFF_PROG='cp'
# Not `/usr/bin/cp`; see "The reference is built, not found" above.
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

work=$DIFF_TMP/work
mkdir -p "$work"

case_no=0

# --- the fixture --------------------------------------------------------------
# A small tree, a loose file, an empty directory, and one symlink, built in a
# fixed order.
#
# The order is load-bearing rather than tidy. Both programs walk a directory in
# *inode* order -- GNU through `savedir (dir, SAVEDIR_SORT_FASTREAD)`, ours
# through `read_dir_fastread`, which reproduces it -- and on a fresh ext4
# directory inode order is the order the entries were created. So two case
# directories built by the same commands in the same sequence enumerate
# identically, which is what lets `-rv`'s stdout and the diagnostics from a
# partly-failing walk be compared line for line at all.
#
# It follows that a case which creates extra fixture files through `TREE` has
# them *after* the four `mktree` makes, in the order its own commands run.
#
# `tree/link` points at a *sibling*, relatively. That is the shape that tells a
# clone from a dereference without a second lookup: cloned, it still resolves
# in the destination; followed, it becomes a second copy of `a.txt`'s bytes
# with no `->` at all.
mktree() {
  mkdir -p tree/sub
  printf 'a\n' > tree/a.txt
  printf 'bb\n' > tree/sub/b.txt
  ln -s a.txt tree/link
  printf 'top\n' > file.txt
  mkdir dir
}

# --- what a case leaves behind ------------------------------------------------
# Every path, its octal mode, and its size — except:
#
#   * a directory, whose size is that of the block holding its entries and so
#     says nothing about what is in it while varying with what used to be. `d`
#     stands in.
#   * a symlink, whose mode is 0777 on Linux always and whose size is the
#     length of its text. Neither carries information; the text does, so that
#     is what is printed.
#
# Errors are discarded and are meant to be: a case that leaves an unreadable
# directory leaves one on both sides, so `find` fails to descend on both sides
# and the blind spot is symmetric. The unreadable directory itself is still
# listed, with its mode.
#
# The modification time is printed only when [`STAMPS`] is set, which is the
# `-p` section and nowhere else. It cannot be unconditional: without `-p` the
# destination gets the time of the copy, the two sides copy one after the
# other, and the column would differ between two runs of the *same* program.
# Under `-p` it is the whole point of the case, and the fixture pins every
# source's time with `touch -d` so that the expected value is a constant rather
# than whenever the harness happened to run.
#
# `%T@` and not `%TY-%Tm-…`: seconds-and-fraction since the epoch is what
# `utimensat` actually carries, and a formatted time would hide a copy that got
# the seconds right and threw the nanoseconds away.
snapshot() {
  local t=''
  [ -n "$STAMPS" ] && t=' %T@'
  ( cd "$1" 2>/dev/null && find . -mindepth 1 \
        \( -type d -printf "%P %m d$t\n" \
        -o -type l -printf "%P l -> %l$t\n" \
        -o -printf "%P %m %s$t\n" \) 2>/dev/null \
      | fold_now | LC_ALL=C sort )
}

# Anything after this is the harness's own clock rather than the fixture's.
# `mkstamped` pins everything it makes to 2001-2007; the cutoff is 2011 and the
# harness runs long after it.
STAMP_CUTOFF=1300000000

# Rewrite a timestamp the fixture did not set as the literal `now`.
#
# Without this the section below could assert half of what it is for. A case
# that preserves the times can be compared outright — both sides produce the
# fixture's constant. A case that does *not* preserve them gives the
# destination the moment it was copied, the two sides are copied one after the
# other, and the column differs every run. Dropping those cases from the
# comparison would mean never checking that `--preserve=mode` leaves the time
# alone, which is exactly the confusion — one option quietly doing all three —
# that one-attribute-at-a-time cases exist to catch. Folding says which of the
# two happened without saying when.
#
# `sub` on `$0` and not `$NF = "now"`: assigning to a field makes awk rebuild
# the record with single-space separators, which would silently rewrite any
# name holding a tab. Nothing in `mkstamped` does, and this stays true if
# something later does.
fold_now() {
  if [ -z "$STAMPS" ]; then cat; return; fi
  awk -v cut="$STAMP_CUTOFF" '
    { if ($NF ~ /^[0-9]+\.[0-9]+$/ && $NF + 0 > cut) sub(/[^ ]+$/, "now"); print }'
}

# And the bytes, so that a file which arrived with the right size and the wrong
# contents is still caught. `-type f` does not follow, so a symlink is not read
# here — its target is already in the snapshot above.
#
# NUL-separated, and sorted with `sort -z`, because section 12 creates a file
# whose name holds a newline. Read line by line, that name arrived here as two
# names, neither of which exists, so `cat` failed on both and the file's bytes
# were never compared at all -- on either side, so nothing ever went red and
# the only trace was `cat: b: No such file or directory` on the harness's own
# stderr. A blind spot that announces itself and is still a blind spot.
#
# The `cat: …: Permission denied` lines that remain are section 11's, and are
# the *symmetric* kind: the case makes a file unreadable on both sides, so both
# bodies come out empty and the comparison says nothing about that one file
# rather than saying something false about it. Same bargain as `snapshot`'s
# discarded `find` errors, for the same reason.
contents() {
  ( cd "$1" 2>/dev/null || return 0
    find . -type f -printf '%P\0' 2>/dev/null | LC_ALL=C sort -z \
      | while IFS= read -r -d '' f; do
      printf '== %s\n' "$f"
      cat -- "$f"
      printf '\n'
    done )
}

# --- knobs, reset after every case --------------------------------------------

# Shell run inside the case directory to build the fixture.
TREE=
# What `cp` reads on standard input, for `-i`'s prompts. Empty is end of input
# straight away, which is what every case before `-i` existed got -- `run_one`
# redirected `</dev/null` unconditionally, and still does when this is empty.
# Written with `$'\n'` escapes rather than real newlines so that a case's
# answers stay on the case's own line and can be read next to its options.
ANSWERS=
# Non-empty makes [`snapshot`] print each path's modification time. Set by the
# `-p` cases, which are the only ones whose fixture pins those times; see
# `snapshot`.
STAMPS=
reset_knobs() { TREE='mktree'; ANSWERS=''; STAMPS=''; }
reset_knobs

# The two sides run in two different directories, and a case that names an
# absolute path gets that path echoed back in the diagnostic. Comparing those
# raw would fail on the one thing that is supposed to differ. The replacement
# is per side, not of a common prefix: a path pointing anywhere other than that
# side's own directory survives and shows up as a difference.
scrub() { sed -e "s|$1|<DIR>|g"; }

# --- running one side ---------------------------------------------------------

run_one() {
  local side=$1 dir=$2 out=$3 err=$4 rcf=$5; shift 5
  mkdir -p "$dir"
  ( cd "$dir" && eval "$TREE" ) >/dev/null 2>&1
  # One file per side rather than one shared one: the two sides run one after
  # the other and a shared file would be consumed by whichever ran first.
  local answers=$dir.stdin
  printf '%s' "$ANSWERS" >"$answers"
  (
    # `$out`/`$err` are absolute: they are opened after this `cd`.
    cd "$dir" || exit 1
    # Reached as the bare word `cp`, through the one-entry directory
    # `diff-wsl.sh` built, and *not* by the path of the symlink. gnulib's
    # `set_program_name` takes `argv[0]` whole, so GNU invoked as
    # `/tmp/xxx/bin/gnu/cp` prefixes every diagnostic with that entire path
    # while ours prints `cp:`, and every case that says anything at all
    # differs for a reason that has nothing to do with either program.
    # Prepended rather than replacing `PATH`, so `timeout` is still findable.
    PATH="$bindir/$side:$PATH"
    diff_run timeout -k 2 30 cp "$@" >"$out" 2>"$err"
  ) <"$answers"
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
  local label="cp $*"
  [ -z "$ANSWERS" ] || label="$label   [in: ${ANSWERS//$'\n'/\\n}]"
  [ "$TREE" = mktree ] || label="$label   [tree: $TREE]"
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
# starts agreeing is reported too: a stale xfail is a claim nobody rechecked,
# and for the whole "not implemented" block below it is the signal that an
# option has landed and its case is now a real one.
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

# Shorthand for the inventory block at the end: an option GNU acts on and this
# `cp` refuses.
missing() { xfail_case "not implemented by this cp" "$@"; }

# The reason six copy-into-itself cases are expected to differ. Both sides
# print the same diagnostic and exit 1; what differs is the debris left on
# disk, and GNU's debris is not a fixed thing to match -- see section 9 and
# design-decisions.md 724.
SELF_RESIDUE='GNU leaves a partial copy, we leave the tree untouched'

echo "cp-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. Too few operands
# =============================================================================
# Zero and one are distinct diagnostics: "missing file operand" alone left the
# user to work out *which* operand was missing, and GNU names the one it did
# get. An empty string is an operand — it names no file, so one empty operand
# is still "missing destination".

run_case
run_case file.txt
run_case ''
run_case '' ''
run_case -r
run_case -r tree

# =============================================================================
# 2. Option errors, and where the option table is observable
# =============================================================================
# The abbreviation cases are the reason `LONG_OPTIONS` holds all thirty entries
# rather than the one this implementation acts on: `getopt_long` lists an
# ambiguous prefix's candidates in table order, so an absent entry changes the
# text of an error about a *different* option.

# `-w`, not `-x`: `-x` *is* a `cp` option — the short spelling of
# `--one-file-system` — so it belongs in the inventory at the end and not here.
# The first run of this harness had it here and reported a difference that was
# the harness's mistake rather than the program's.
run_case -w file.txt dst
run_case --nosuchoption file.txt dst
run_case -- file.txt dst
run_case --recursive=yes tree dst
run_case --rec tree dst
run_case --recu tree dst
# `--p` is genuinely ambiguous — `parents`, `path` and `preserve` — while
# `--pa` is not, because `path` and `parents` are one option under two names.
run_case --p tree dst
run_case --=x file.txt dst
# A `-` on its own is a file called `-`, not a request to read standard input:
# `cp` has no standard-input operand for it to mean anything else.
run_case - dst
TREE='mktree; printf dash > ./-'
run_case ./- dst
run_case -- -foo dst
TREE='mktree; printf dash > ./-foo'
run_case -- -foo dst

# =============================================================================
# 3. One regular file
# =============================================================================

run_case file.txt new.txt
run_case file.txt dir
run_case file.txt dir/
run_case file.txt dir/new.txt
run_case file.txt tree/a.txt
run_case file.txt ./new.txt
run_case file.txt dir/../new.txt
run_case tree/a.txt new.txt
run_case tree/sub/b.txt dir

# An empty source, and a source with no trailing newline: both are ways for a
# copy that got the size right to have got the bytes wrong.
TREE='mktree; : > empty'
run_case empty new.txt
TREE='mktree; printf nonl > partial'
run_case partial new.txt

# Where the destination cannot be created.
run_case file.txt nosuchdir/new.txt
run_case file.txt tree/a.txt/under
run_case nosuch new.txt
run_case nosuch/deeper new.txt

# A source that is a symlink, without `-r`: the link is followed, so the
# destination is a regular file holding the target's bytes.
run_case tree/link new.txt
run_case tree/link dir
TREE='mktree; ln -s nowhere dangling'
run_case dangling new.txt
TREE='mktree; ln -s loop loop'
run_case loop new.txt

# A trailing slash on something that is not a directory.
run_case file.txt/ new.txt
run_case tree/a.txt/ dir

# =============================================================================
# 4. The same file twice
# =============================================================================
# GNU refuses, by comparing the two `stat` results rather than the two strings,
# so every spelling below is caught. It has to: the destination is opened with
# `O_TRUNC`, so a `cp` that went ahead would empty the file it was asked to
# duplicate — data loss from a command that looks like a no-op.

run_case file.txt file.txt
run_case file.txt ./file.txt
run_case ./file.txt file.txt
run_case file.txt dir/../file.txt
run_case tree/a.txt tree/./a.txt
# A hard link is the same file under two names, and so is a symlink to it once
# resolved. The first is caught by inode; the second is the case where GNU
# copies anyway, because the *link* is not the file.
TREE='mktree; ln file.txt hard.txt'
run_case file.txt hard.txt
TREE='mktree; ln -s file.txt soft.txt'
run_case file.txt soft.txt
TREE='mktree; ln -s file.txt soft.txt'
run_case soft.txt file.txt
# Under `-r` the source is *not* followed, so the link and its target are two
# different files and the question has a different answer.
TREE='mktree; ln -s file.txt soft.txt'
run_case -r soft.txt file.txt
# The destination reached through a link that lives elsewhere in the tree.
run_case tree/link tree/a.txt
# A directory named twice, with and without `-r`: without it the refusal is
# about the missing `-r` and the sameness never comes up.
run_case tree tree
run_case dir dir

# =============================================================================
# 5. Several sources
# =============================================================================

run_case file.txt tree/a.txt dir
run_case file.txt tree/a.txt tree/sub/b.txt dir
# Two sources and a destination that is not a directory: refused before
# anything is copied, so neither source lands.
run_case file.txt tree/a.txt new.txt
run_case file.txt tree/a.txt file.txt
# One bad source among good ones. The exit status must be 1 and the *others*
# must still arrive — a `cp` that stops at the first failure leaves a partial
# copy that looks complete to anything not checking the status.
run_case nosuch file.txt dir
run_case file.txt nosuch tree/a.txt dir
run_case nosuch alsonosuch dir
# Two sources with the same basename into one directory. The second would land
# on the copy the first just made, so the copy the user asked for would be gone
# with nothing said: both sides refuse, and the exit status is 1. The variants
# pin *where* the check sits — it is asked once per pair, so a third operand
# repeating the first is caught too, and a source named twice is a different
# complaint again.
TREE='mktree; mkdir other; printf other > other/file.txt'
run_case file.txt other/file.txt dir
TREE='mktree; mkdir other; printf other > other/file.txt'
run_case other/file.txt file.txt dir
TREE='mktree; mkdir other; printf other > other/file.txt'
run_case file.txt other/file.txt file.txt dir
TREE='mktree; mkdir other; printf other > other/file.txt'
run_case file.txt file.txt other/file.txt dir
# The destination just created is a *symlink*, and the second source is not.
# The check above stats the destination followed, so it cannot see this one:
# without a second check on the link itself, the copy is written through the
# link and lands on whatever it points at.
TREE='mktree; mkdir other; printf plain > other/link'
run_case -r tree/link other/link dir
# One source named twice. Not an error on either side — the user asked for a
# file that is already there, and it is — but it is warned about and the second
# copy is skipped. `./` and a trailing slash are the same request spelled
# differently; a hard link is *not*, because two entries sharing an inode are
# two files as far as this question goes.
run_case file.txt file.txt dir
run_case file.txt ./file.txt dir
run_case ./file.txt file.txt dir
TREE='mktree; ln file.txt hard.txt'
run_case file.txt hard.txt dir
TREE='mktree; ln -s file.txt soft.txt'
run_case file.txt soft.txt dir
run_case -r tree tree dir
run_case -r tree ./tree dir
run_case -r tree tree/ dir
# The repeat is recorded even when the copy it belonged to failed, so the
# second operand is warned about rather than refused a second time. Directories
# are the other way round: their check sits *after* the refusal, so both
# operands are refused.
TREE='mktree; mkdir dir/file.txt'
run_case file.txt file.txt dir
TREE='mktree; printf x > dir/tree'
run_case -r tree tree dir
# Three sources, one repeated and one colliding: both complaints appear.
TREE='mktree; mkdir other; printf other > other/file.txt'
run_case file.txt file.txt other/file.txt dir

# =============================================================================
# 6. A directory without -r
# =============================================================================
# Refused, and the refusal counts against the exit status — it is not a
# warning. The trailing-slash and dot spellings are here because they are the
# ones that reach the message with a different string in it.

run_case dir dst
run_case tree dst
run_case tree/ dst
run_case tree/. dst
run_case tree dir
run_case file.txt tree dir
run_case tree file.txt dir
# A directory *and* a source that names nothing to create. Which of the two
# complaints comes first is what pins the order of the checks: the refusal is a
# fact about the source alone, so it is asked before the destination is worked
# out at all.
run_case tree/.. dst
run_case tree/.. dir

# =============================================================================
# 7. Recursive copy
# =============================================================================

run_case -r tree dst
run_case -R tree dst
run_case --recursive tree dst
run_case -r tree dir
run_case -r tree/sub dst
run_case -r dir dst
run_case -r file.txt new.txt
run_case -r file.txt dir
run_case -r tree/a.txt dst
# Onto a destination that already exists, as a directory and as a file.
TREE='mktree; mkdir dst; printf keep > dst/keep.txt'
run_case -r tree dst
TREE='mktree; mkdir -p dir/tree; printf keep > dir/tree/keep.txt'
run_case -r tree dir
TREE='mktree; printf blocking > dst'
run_case -r tree dst
# Two trees into one directory. The second names the destination as a source
# as well, which is a copy into itself; see section 9 for why the residue is
# not something to match.
run_case -r tree dir dst
xfail_case "$SELF_RESIDUE" -r tree dir dir

# A tree several levels deep, so that a walk which only descends one level is
# visible.
TREE='mktree; mkdir -p tree/sub/deeper/deepest; printf d > tree/sub/deeper/deepest/d.txt'
run_case -r tree dst

# =============================================================================
# 8. Symlinks inside a recursive copy
# =============================================================================
# Module docs, bug 1. `-r` does not dereference, so each link below must appear
# in the destination as a link with the same text. Followed instead, the link
# to an ancestor makes the walk descend for ever, the dangling one fails, and
# every other one silently becomes a second full copy of its target.

run_case -r tree dst
TREE='mktree; ln -s .. tree/up'
run_case -r tree dst
TREE='mktree; ln -s . tree/self'
run_case -r tree dst
TREE='mktree; ln -s nowhere tree/dangling'
run_case -r tree dst
TREE='mktree; ln -s /etc/hostname tree/absolute'
run_case -r tree dst
TREE='mktree; ln -s sub tree/todir'
run_case -r tree dst
TREE='mktree; ln -s loop tree/loop'
run_case -r tree dst
# A symlink as the operand itself: cloned under `-r`, followed without it.
run_case -r tree/link dst
run_case -r tree/link dir
TREE='mktree; ln -s tree treelink'
run_case -r treelink dst
TREE='mktree; ln -s tree treelink'
run_case -r treelink/ dst

# =============================================================================
# 9. Copying a directory into itself
# =============================================================================
# Module docs, bug 2. Every spelling below resolves the destination to a path
# inside the source, so a `cp` that went ahead would copy what it had just
# written. Both sides are expected to refuse; the header explains why no case
# here names a bare `..`.
#
# The six marked `xfail_case` agree on stderr and on exit status and differ
# only in what is left on disk. Ours refuses before creating anything; GNU
# notices only when its walk trips over the destination directory it made
# earlier, and keeps whatever it copied first. That residue is not a behaviour
# to match: `copy_dir` visits entries in inode order
# (`savedir (…, SAVEDIR_SORT_FASTREAD)`), so how much gets copied first depends
# on the inode number the kernel allocated for the destination. Measured on
# ext4 under coreutils 9.4, `cp -r . dst` over the same 100 files left the
# complete copy when the destination drew a high inode and a single empty
# directory when 1900 deletions made a low one available. See
# design-decisions.md 724.
#
# The four left as `run_case` agree completely, because there the destination
# already exists and GNU creates no directory to trip over.

xfail_case "$SELF_RESIDUE" -r tree tree
run_case -r tree .
run_case -r tree ./
xfail_case "$SELF_RESIDUE" -r tree tree/sub
xfail_case "$SELF_RESIDUE" -r . dst
xfail_case "$SELF_RESIDUE" -r ./ dst
xfail_case "$SELF_RESIDUE" -r tree/.. dst
run_case -r tree/. dst
run_case -r tree/sub/.. dst
run_case -r tree/ dst
# Not into itself: a sibling with a name that merely *starts* the same way. A
# prefix comparison done on the string rather than on the resolved path refuses
# this one wrongly.
TREE='mktree; mkdir treeish'
run_case -r tree treeish
TREE='mktree; mkdir treeish'
run_case -r treeish tree

# =============================================================================
# 10. Modes
# =============================================================================
# Module docs, bug 3: a directory made by `create_dir_all` takes the umask's
# default, so a 0700 source published everything under it as 0755. The mode is
# in the snapshot, and nowhere in the text, so this section is invisible to any
# harness that compares only what was printed.
#
# The file cases are the other half of the same question and are not the same
# answer: GNU masks a *new* destination's mode with the umask and leaves an
# *existing* destination's mode alone entirely.

TREE='mktree; chmod 700 tree'
run_case -r tree dst
TREE='mktree; chmod 700 tree/sub'
run_case -r tree dst
TREE='mktree; chmod 750 tree; chmod 705 tree/sub'
run_case -r tree dst
TREE='mktree; chmod 777 file.txt'
run_case file.txt new.txt
TREE='mktree; chmod 600 file.txt'
run_case file.txt new.txt
TREE='mktree; chmod 755 file.txt'
run_case file.txt new.txt
TREE='mktree; chmod 400 tree/a.txt'
run_case -r tree dst
TREE='mktree; chmod 777 file.txt; printf old > old.txt; chmod 600 old.txt'
run_case file.txt old.txt
TREE='mktree; chmod 600 file.txt; printf old > old.txt; chmod 777 old.txt'
run_case file.txt old.txt
# A set-user-ID bit on the source. Copying it onto a destination the copier
# owns is a privilege question, not a bookkeeping one, and the two programs are
# entitled to differ — which is exactly why it is measured rather than assumed.
TREE='mktree; chmod 4755 file.txt'
run_case file.txt new.txt
TREE='mktree; chmod 1777 tree'
run_case -r tree dst

# =============================================================================
# 11. What cannot be read or written
# =============================================================================
# One failure must not abandon the rest — module docs, bug 6. These run as an
# ordinary user, so a mode of 000 really does deny; they are skipped when the
# harness runs as root, where it would not.

if [ "$(id -u)" -ne 0 ]; then
  TREE='mktree; chmod 000 file.txt'
  run_case file.txt new.txt
  TREE='mktree; chmod 000 tree/a.txt'
  run_case -r tree dst
  TREE='mktree; chmod 000 tree/sub'
  run_case -r tree dst
  TREE='mktree; chmod 500 dir'
  run_case file.txt dir
  TREE='mktree; chmod 500 dir; mkdir other; printf o > other/x'
  run_case -r other dir
  TREE='mktree; chmod 000 tree/a.txt'
  run_case tree/a.txt new.txt
  # Good entries either side of a bad one, so that "reported and carried on"
  # can be told from "reported and stopped".
  TREE='mktree; printf 1 > tree/1.txt; chmod 000 tree/1.txt; printf 2 > tree/2.txt'
  run_case -r tree dst
fi

# =============================================================================
# 12. Names that quoting has an opinion about
# =============================================================================
# `quotearg` picks its style from the bytes in the name, so these are the cases
# where a diagnostic can be right about the file and wrong about how it spells
# it. A name that is not UTF-8 at all is the one that used to make this program
# panic outright — `known-issues.md` ->
# `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.

run_case "$(printf 'na\377me')" dst
run_case "$(printf '\377')" dst
TREE='mktree; printf x > "$(printf "na\377me")"'
run_case "$(printf 'na\377me')" new.txt
TREE='mktree; printf x > "tree/$(printf "na\377me")"'
run_case -r tree dst
run_case 'two words' dst
TREE='mktree; printf x > "two words"'
run_case 'two words' new.txt
# A single quote is the one byte that makes `quotearg` reach for double quotes,
# and only when everything else in the name is double-quote-safe. The `$` in
# the second name is not, so that one falls back to the escaped form.
run_case "it's" dst
run_case "it's\$" dst
TREE='mktree; printf x > "tree/it'"'"'s"'
run_case -r tree dst
run_case "$(printf 'a\nb')" dst
TREE='mktree; printf x > "$(printf "tree/a\nb")"'
run_case -r tree dst
TREE='mktree; d=$(printf "na\377me"); mkdir "$d"; printf x > "$d/x"'
run_case -r "$(printf 'na\377me')" dst

# =============================================================================
# 13. Where the destination comes from: -t and -T
# =============================================================================
# Every case above lets the *last operand* decide, and decide by being a
# directory or not. These two options take that decision away from it in
# opposite directions -- `-t` names the directory itself and leaves every
# operand a source, `-T` says the destination is a name and must never be
# copied into -- so between them they change the arity of the command, which
# error is reported, and where the bytes land. All three are observable, and
# the cases below are grouped by which.

# Arity. `-t` needs one operand where the default needs two, and under `-T` a
# third operand is not a misplaced source but an operand with nowhere to go.
run_case -t dir file.txt
run_case -t dir file.txt tree/a.txt
run_case -t dir
run_case -T file.txt new.txt
run_case -T file.txt
run_case -T
run_case -T file.txt tree/a.txt dir

# Spelling. `-tdir` is the one that can only parse through a table saying the
# letter takes a value; the trailing `--` and the interleaved form pin that the
# value is taken from the option and not from the operand stream.
run_case -tdir file.txt
run_case --target-directory=dir file.txt
run_case file.txt -t dir
run_case -t dir -- file.txt
run_case -T -- file.txt new.txt
run_case -t dir/ file.txt

# Which complaint. A second `-t` is refused before its value is even looked at,
# and so is the combination -- the directory named there does not exist and is
# still not what gets reported.
run_case -t dir -t dir file.txt
run_case -t dir -t tree file.txt
run_case -t nosuch -T file.txt
run_case -T -t nosuch file.txt

# The target directory itself, and the ways it can fail to be one. The wording
# is `target directory`, not the bare `target` the last operand gets.
run_case -t nosuch file.txt
run_case -t file.txt tree/a.txt
run_case -t tree/link file.txt
run_case -t '' file.txt

# Where the bytes land. Without `-T` each of these copies *into* the
# destination; with it, onto it.
run_case -T file.txt dir
run_case -T tree dir
run_case -r -T tree dir
run_case -r -T tree newdir
run_case -r -T tree tree
run_case -r -T tree/sub dir

# The pair guards count sources, and under `-t` every operand is one -- so two
# operands are two sources here where they would be one source and a
# destination without it.
run_case -t dir file.txt ./file.txt
run_case -t dir file.txt tree/a.txt tree/sub/b.txt
run_case -t tree file.txt tree/a.txt

# =============================================================================
# 14. What --verbose says, and when
# =============================================================================
# The first option in this program whose whole effect is on *stdout*, which is
# why every case here is worth having even though the tree it leaves is
# identical to the same case without `-v`. Three separable facts:
#
#   * the line goes to stdout, not stderr, so a case that got the sink wrong
#     would still pass a harness comparing only their concatenation. These
#     compare them apart;
#   * a copy is announced *before* it is attempted, so a failure that happens
#     after the announcement is announced anyway and one that happens before it
#     is not -- and the boundary between "before" and "after" is a fact about
#     GNU's order that has to be measured rather than reasoned about;
#   * a directory is announced only when it is *created*, which is the one rule
#     upstream wrote a comment to explain.

# The plain line, and the exit status it does not change.
run_case -v file.txt new.txt
run_case --verbose file.txt new.txt
run_case -v file.txt dir
run_case -v file.txt tree/a.txt

# Quoting. The same `quoteaf` a diagnostic uses -- so a name with a space in it
# is quoted and a name without one is not, in the same line.
TREE='mktree; printf x > "a b"'
run_case -v 'a b' new.txt
TREE='mktree; printf x > "a b"'
run_case -v 'a b' dir
TREE="mktree; printf x > \"it's\""
run_case -v "it's" new.txt

# Which failures are announced first. A source that cannot be stat'd never
# reaches the announcement; a destination that cannot be written does.
run_case -v nosuch new.txt
run_case -v tree new.txt
run_case -v file.txt nosuch/new.txt
run_case -v dir file.txt
run_case -v file.txt file.txt
run_case -v file.txt ./file.txt

# Several operands: one line each, in operand order, and the warning about a
# repeat still goes to stderr while the lines go to stdout.
run_case -v file.txt tree/a.txt dir
run_case -v file.txt file.txt dir
run_case -v -t dir file.txt tree/a.txt
run_case -v -T file.txt new.txt

# Directories. The order of these lines is the order the directory was *read*
# in, which is the one thing about `-rv` that is not obvious: both programs
# walk a directory in inode order, which on ext4 is neither name order nor
# `readdir` order. `tree` here holds `a.txt`, `sub` and `link`, created in the
# order sub, a.txt, link, and both name them in that order and not the other
# two -- so these cases certify `read_dir_fastread`'s sort and would go red
# without it.
run_case -rv tree dst
run_case -rv tree dir
run_case -rv tree/sub dst
run_case -rv tree/sub dir
TREE='mktree; mkdir -p dst/sub'
run_case -rv tree/sub dst
TREE='mktree; mkdir -p dst/sub; printf old > dst/sub/b.txt'
run_case -rv tree/sub dst
TREE='mktree; mkdir -p tree/sub/deeper; printf d > tree/sub/deeper/d.txt'
run_case -rv tree dst
TREE='mktree; mkdir dst; printf keep > dst/keep.txt'
run_case -rv tree dst

# A symlink is announced like anything else that is not a directory, and the
# line names the link rather than what it points at.
run_case -rv tree/link dst
run_case -v tree/link dst

# =============================================================================
# 15. -P, -H, -L: the link or what it points at
# =============================================================================
# Three options that set one field, so the interesting cases are not "does -P
# work" but the two places the field is read:
#
#   * the operand -- `-P` keeps a link named on the command line, `-L` and `-H`
#     follow it, and with none of the three given it depends on `-r`; and
#   * everything found underneath it -- where `-L` follows and `-H` does *not*,
#     which is the only difference between those two and is invisible in any
#     case that copies a single file.
#
# Two more read it indirectly: the same-file guard (`cp -P linkA linkB` is
# allowed, `cp linkA linkB` is "the same file") and the announcement, which is
# why every case here is also `-v` -- the stdout line names what was copied and
# so says which of the two a case actually did.
#
# No case here puts a link to an ancestor under `-L`. Measured: GNU descends
# through it until the path is too long, taking minutes and megabytes of
# output, and ours does the same by doing nothing special -- agreeing about an
# unbounded walk is not something a harness can wait for. Section 8's `tree/up`
# covers the same link under the policy that terminates.

# The operand. `tree/link` is a link to a sibling file, `treelink` a link to a
# directory: the first shows the file/link distinction, the second the one that
# decides whether a walk happens at all.
run_case -Pv tree/link dst
run_case -Lv tree/link dst
run_case -Hv tree/link dst
run_case -v tree/link dst
run_case -rPv tree/link dst
run_case -rLv tree/link dst
run_case -rHv tree/link dst
# A link to a directory without `-r`: followed, it is a directory and the copy
# is refused; kept, it is a link and copies fine. So `-P` succeeds here and the
# other two print `-r not specified`.
TREE='mktree; ln -s tree treelink'
run_case -Pv treelink dst
TREE='mktree; ln -s tree treelink'
run_case -Lv treelink dst
TREE='mktree; ln -s tree treelink'
run_case -Hv treelink dst
TREE='mktree; ln -s tree treelink'
run_case -rPv treelink dst
TREE='mktree; ln -s tree treelink'
run_case -rLv treelink dst
TREE='mktree; ln -s tree treelink'
run_case -rHv treelink dst

# A dangling operand. `-P` copies the link; `-L` and `-H` have nothing to stat
# and fail, with the name of the *link* in the diagnostic.
TREE='mktree; ln -s nowhere dangling'
run_case -Pv dangling dst
TREE='mktree; ln -s nowhere dangling'
run_case -Lv dangling dst
TREE='mktree; ln -s nowhere dangling'
run_case -Hv dangling dst
TREE='mktree; ln -s loop loop'
run_case -Lv loop dst
TREE='mktree; ln -s loop loop'
run_case -Pv loop dst

# Inside the walk, which is where `-H` and `-L` part company. `tree` already
# holds `link -> a.txt`; the extra links below give the walk a directory link
# and a broken one to disagree about too.
run_case -rPv tree dst
run_case -rLv tree dst
run_case -rHv tree dst
TREE='mktree; ln -s sub tree/todir'
run_case -rLv tree dst
TREE='mktree; ln -s sub tree/todir'
run_case -rHv tree dst
TREE='mktree; ln -s nowhere tree/dangling'
run_case -rLv tree dst
TREE='mktree; ln -s nowhere tree/dangling'
run_case -rHv tree dst
TREE='mktree; ln -s /etc/hostname tree/absolute'
run_case -rLv tree dst
# A link to a directory as the operand *and* links inside it: `-H` follows the
# first and keeps the rest, which no other combination does.
TREE='mktree; ln -s tree treelink; ln -s sub tree/todir'
run_case -rHv treelink dst
TREE='mktree; ln -s tree treelink; ln -s sub tree/todir'
run_case -rLv treelink dst

# The same-file guard reads the policy, not `-r`. Two distinct links to one
# file are two things when the links are what is being copied, and one thing
# when they are not.
TREE='mktree; ln -s file.txt one; ln -s file.txt two'
run_case -Pv one two
TREE='mktree; ln -s file.txt one; ln -s file.txt two'
run_case -v one two
TREE='mktree; ln -s file.txt one; ln -s file.txt two'
run_case -Lv one two
TREE='mktree; ln -s file.txt soft.txt'
run_case -Pv soft.txt file.txt
TREE='mktree; ln -s file.txt soft.txt'
run_case -Lv soft.txt file.txt

# Last one wins, and giving two is not an error.
run_case -PLv tree/link dst
run_case -LPv tree/link dst
run_case -HPv tree/link dst
run_case -PHv tree/link dst
run_case --no-dereference -v tree/link dst
run_case --dereference -v tree/link dst
# Abbreviations, which the two long spellings make interesting: `--no-d` is
# unambiguous but `--d` is not -- `--debug` is in the table too.
run_case --no-d -v tree/link dst
run_case --dere -v tree/link dst
run_case --d -v tree/link dst

# =============================================================================
# 16. The four overwrite policies: -f, --remove-destination, -n, -i
# =============================================================================
# Four options that all sound like "about overwriting" and are not variants of
# one another. They act at four different points, and only two of them are ever
# reached on the ordinary path where the destination opens for writing without
# complaint:
#
#   * `-f` acts *after* an open for writing has already failed, and only then.
#     On an ordinary destination it changes nothing at all, which is why the
#     first case below is `-fv` on a plain file and is expected to look exactly
#     like plain `-v`.
#   * `--remove-destination` acts *before* any open is attempted, so it changes
#     the ordinary path too -- it unlinks a perfectly writable destination and
#     makes a new one.
#   * `-n` is none of that: it refuses, on stderr, and exits 1, before any of
#     the above is attempted.
#   * `-i` asks, in the same place `-n` refuses, and a non-`y` answer is `-n`'s
#     outcome without `-n`'s message. `-n` and `-i` are two values of one
#     field, so the last of the two given wins -- but they are not mirror
#     images: `-n` also suppresses the same-file check and `-i` does not.
#
# The order the two verbose lines come out in is the sharpest way to tell the
# first two apart, and is why every case here is `-v`: `-f` prints the arrow
# first and `removed` second (the removal is a recovery from a failure already
# in progress), `--remove-destination` prints `removed` first (the removal is
# the first thing it does). A harness comparing only exit status would call
# them the same option.
#
# The destinations are chosen for *how the open fails*, since that is what
# `-f` keys on:
#
#   | destination        | plain `cp`                | what `-f` does        |
#   |--------------------|---------------------------|-----------------------|
#   | writable file      | truncates it              | nothing; open worked  |
#   | mode 400 file      | EACCES                    | unlink, create new    |
#   | dangling symlink   | refuses to write through  | nothing; see below    |
#   | good symlink       | writes through to target  | nothing; open worked  |
#   | self-loop symlink  | cannot stat: ELOOP        | unlink, create new    |
#
# The dangling-symlink row is the one worth stating outright, because "force"
# suggests otherwise: `-f` does not rescue it. The refusal comes from the
# create-new arm, which `-f` never re-enters -- it retries by creating, and
# creating is what already failed. The self-loop row is the opposite surprise:
# there the *stat* fails, not the open, and `-f` is consulted about that too.
#
# `-i` reads its answers from standard input, which is what the `ANSWERS` knob
# feeds; every case without it gets an empty file, which is end of input, which
# declines. Its own questions are keyed on something different again -- not on
# how the open fails but on whether the destination is *writable* at all, by
# `euidaccess`, which is why the mode-400 row below has `-i` cases of its own.
#
# See `design-decisions.md` 727 and `cp.rs`'s "The four overwrite policies are
# four different options".

# --- -f, and what it does not do ---------------------------------------------
# On a destination that opens, `-f` is dead weight; the pair is here so that a
# future change which makes `-f` unlink unconditionally is caught.
TREE='mktree; printf old > old.txt'
run_case -fv file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case -v file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case --force -v file.txt old.txt
run_case -f file.txt new.txt
run_case -fv file.txt tree/a.txt

# --- --remove-destination, which acts on that same destination ---------------
# Same fixture, different answer: the destination is replaced rather than
# truncated, and `removed` is printed before the arrow rather than after.
TREE='mktree; printf old > old.txt'
run_case --remove-destination -v file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case --rem -v file.txt old.txt
run_case --remove-destination -v file.txt new.txt
# A destination that does exist, reached through a directory operand.
run_case --remove-destination -v file.txt tree

# --- -n, which refuses -------------------------------------------------------
# The diagnostic is on stderr and the status is 1, both of which Ubuntu's
# `cp-n.diff` changes -- see "The reference is built, not found" at the top.
TREE='mktree; printf old > old.txt'
run_case -n file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case -nv file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case --no-clobber file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case --no-c -v file.txt old.txt
# Nothing there to refuse, so it copies and the status is 0.
run_case -nv file.txt new.txt
# Several sources, one of which exists: the refusal must not stop the rest, and
# the status must still be 1.
TREE='mktree; printf 1 > s1; printf 2 > s2; printf 3 > s3; printf x > dir/s2'
run_case -nv s1 s2 s3 dir
# Into a directory, where the destination name is derived rather than given.
run_case -nv file.txt dir
TREE='mktree; printf x > dir/file.txt'
run_case -nv file.txt dir

# --- -i, which asks ----------------------------------------------------------
# The prompt has no trailing newline, so a case with several of them puts them
# all on one line of stderr -- which is why they are compared as raw text
# rather than line by line, and is itself worth pinning: a stray `\n` here
# would be invisible to a line-wise comparison and glaring to a person.
#
# The answer is `^[yY]` and nothing else (gnulib `rpmatch` under LC_ALL=C, which
# `diff-wsl.sh` sets for both sides), so the accepting and declining spellings
# below are the rule stated as cases.
TREE='mktree; printf old > old.txt'
ANSWERS='y'$'\n'
run_case -i file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS='n'$'\n'
run_case -i file.txt old.txt
# No newline after the answer: `getline` returns it anyway at end of input.
TREE='mktree; printf old > old.txt'
ANSWERS='y'
run_case --interactive file.txt old.txt
# The spellings that accept and the ones that do not. Only the first byte is
# looked at, the match is anchored, and a leading space therefore declines.
TREE='mktree; printf old > old.txt'
ANSWERS='Y'$'\n'
run_case -iv file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS='yeah, fine'$'\n'
run_case -iv file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS=' y'$'\n'
run_case -iv file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS=$'\n'
run_case -iv file.txt old.txt
# Nothing at all on stdin: end of input declines, and declining is a *silent*
# exit 1 -- the question is the whole of stderr, with no `not replacing`.
TREE='mktree; printf old > old.txt'
run_case -iv file.txt old.txt
# Nothing there to ask about, so it copies without reading the answer.
ANSWERS='n'$'\n'
run_case -iv file.txt new.txt
# One question per operand, taken in order, and a decline that does not stop
# the sources after it.
TREE='mktree; printf 1 > s1; printf 2 > s2; printf 3 > s3
      printf x > dir/s1; printf x > dir/s2; printf x > dir/s3'
ANSWERS='y'$'\n''n'$'\n''y'$'\n'
run_case -iv s1 s2 s3 dir
# Fewer answers than questions: the queue runs out and the rest decline.
TREE='mktree; printf 1 > s1; printf 2 > s2; printf 3 > s3
      printf x > dir/s1; printf x > dir/s2; printf x > dir/s3'
ANSWERS='y'$'\n'
run_case -iv s1 s2 s3 dir
# Into a directory, where the destination name in the question is the derived
# one and not the operand.
TREE='mktree; printf x > dir/file.txt'
ANSWERS='y'$'\n'
run_case -iv file.txt dir
# `-i` and `-n` are one field with two values, so the last one given wins --
# including when they are clustered into one argument.
TREE='mktree; printf old > old.txt'
ANSWERS='y'$'\n'
run_case -niv file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS='y'$'\n'
run_case -inv file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS='y'$'\n'
run_case -n -i -v file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS='y'$'\n'
run_case -i -n -v file.txt old.txt

# --- the same four against a destination that will not open ------------------
# Mode 400 denies only for a non-root copier, so these are guarded exactly as
# section 11's are.
if [ "$(id -u)" -ne 0 ]; then
  TREE='mktree; printf old > ro.txt; chmod 400 ro.txt'
  run_case -v file.txt ro.txt
  TREE='mktree; printf old > ro.txt; chmod 400 ro.txt'
  run_case -fv file.txt ro.txt
  TREE='mktree; printf old > ro.txt; chmod 400 ro.txt'
  run_case --remove-destination -v file.txt ro.txt
  TREE='mktree; printf old > ro.txt; chmod 400 ro.txt'
  run_case -nv file.txt ro.txt
  # `-i` is where a destination that will not open changes the *question*: the
  # plain `overwrite 'x'?` becomes one that quotes the mode, and which of the
  # two mode-quoting wordings comes out depends on whether `cp` means to write
  # through the bits or to unlink the file first. All three wordings, and the
  # `04lo` mode and the `rwx` string inside two of them, are pinned here.
  TREE='mktree; printf old > ro.txt; chmod 400 ro.txt'
  ANSWERS='n'$'\n'
  run_case -iv file.txt ro.txt
  TREE='mktree; printf old > ro.txt; chmod 400 ro.txt'
  ANSWERS='y'$'\n'
  run_case -iv file.txt ro.txt
  TREE='mktree; printf old > ro.txt; chmod 400 ro.txt'
  ANSWERS='y'$'\n'
  run_case -ifv file.txt ro.txt
  TREE='mktree; printf old > ro.txt; chmod 400 ro.txt'
  ANSWERS='n'$'\n'
  run_case -ifv file.txt ro.txt
  TREE='mktree; printf old > ro.txt; chmod 400 ro.txt'
  ANSWERS='y'$'\n'
  run_case -i --remove-destination -v file.txt ro.txt
  # A mode with more of the bits set, so that the `rwx` string in the question
  # is not the same nine characters every time.
  TREE='mktree; printf old > ro.txt; chmod 461 ro.txt'
  ANSWERS='n'$'\n'
  run_case -iv file.txt ro.txt
  TREE='mktree; printf old > ro.txt; chmod 4451 ro.txt'
  ANSWERS='n'$'\n'
  run_case -iv file.txt ro.txt
  TREE='mktree; printf old > ro.txt; chmod 2000 ro.txt'
  ANSWERS='n'$'\n'
  run_case -iv file.txt ro.txt
  # The mode of what `-f` leaves behind is the source's, not the destination's,
  # because the destination is gone -- the snapshot is where that shows.
  TREE='mktree; chmod 750 file.txt; printf old > ro.txt; chmod 400 ro.txt'
  run_case -fv file.txt ro.txt
  # Reported and carried on, with a good destination either side of the bad one.
  TREE='mktree; printf 1 > 1; printf 2 > 2; printf 3 > 3
        printf x > dir/1; printf x > dir/2; chmod 400 dir/2; printf x > dir/3'
  run_case -v 1 2 3 dir
  TREE='mktree; printf 1 > 1; printf 2 > 2; printf 3 > 3
        printf x > dir/1; printf x > dir/2; chmod 400 dir/2; printf x > dir/3'
  run_case -fv 1 2 3 dir
fi

# --- against a symlink destination -------------------------------------------
# A link that resolves: the open follows it, so `-f` is not consulted and the
# *target* is what changes. `--remove-destination` replaces the link itself,
# which is the whole difference between the two options stated in one case.
TREE='mktree; printf B > b.txt; ln -s b.txt lnk'
run_case -v file.txt lnk
TREE='mktree; printf B > b.txt; ln -s b.txt lnk'
run_case -fv file.txt lnk
TREE='mktree; printf B > b.txt; ln -s b.txt lnk'
run_case --remove-destination -v file.txt lnk
TREE='mktree; printf B > b.txt; ln -s b.txt lnk'
run_case -nv file.txt lnk
# `-i` asks about the *link*, not about the target it will actually write, and
# never quotes a mode: the writability short-circuit takes `S_ISLNK` as
# writable outright, because the bits on a symlink mean nothing.
TREE='mktree; printf B > b.txt; ln -s b.txt lnk'
ANSWERS='y'$'
'
run_case -iv file.txt lnk
TREE='mktree; printf B > b.txt; ln -s b.txt lnk'
ANSWERS='n'$'
'
run_case -iv file.txt lnk
# And still does not quote a mode when the target denies, for the same reason.
if [ "$(id -u)" -ne 0 ]; then
  TREE='mktree; printf B > b.txt; chmod 400 b.txt; ln -s b.txt lnk'
  ANSWERS='n'$'
'
  run_case -iv file.txt lnk
fi

# A link that resolves to nothing. `-f` does not help; `--remove-destination`
# does, because it never asks whether the link resolves.
TREE='mktree; ln -s nowhere dang'
run_case -v file.txt dang
TREE='mktree; ln -s nowhere dang'
run_case -fv file.txt dang
TREE='mktree; ln -s nowhere dang'
run_case --remove-destination -v file.txt dang
TREE='mktree; ln -s nowhere dang'
run_case -nv file.txt dang
# Nothing is there as far as the follow-stat is concerned, so `-i` has nothing
# to ask about and refuses on its own grounds instead.
TREE='mktree; ln -s nowhere dang'
ANSWERS='y'$'
'
run_case -iv file.txt dang

# A link to the source. Plain `cp` calls it the same file;
# `--remove-destination` is excused from that check, because after the unlink
# it would not be the same file -- and so it is the one case where "same file"
# is not an error.
TREE='mktree; ln -s file.txt self'
run_case -v file.txt self
TREE='mktree; ln -s file.txt self'
run_case -fv file.txt self
TREE='mktree; ln -s file.txt self'
run_case --remove-destination -v file.txt self
TREE='mktree; ln -s file.txt self'
run_case -nv file.txt self
# The one place `-i` and `-n` disagree about the *order* rather than the
# answer: GNU guards the same-file check with `interactive != I_ALWAYS_NO`, so
# `-n` never reaches it and says `not replacing`, while `-i` reaches it, says
# `are the same file`, and asks nothing -- the `y` below goes unread.
TREE='mktree; ln -s file.txt self'
ANSWERS='y'$'
'
run_case -iv file.txt self
TREE='mktree; ln -s file.txt self'
ANSWERS='y'$'
'
run_case -iv --remove-destination file.txt self

# A link to itself, where it is the stat that fails rather than the open.
TREE='mktree; ln -s loop loop'
run_case -v file.txt loop
TREE='mktree; ln -s loop loop'
run_case -fv file.txt loop
TREE='mktree; ln -s loop loop'
run_case --remove-destination -v file.txt loop
TREE='mktree; ln -s loop loop'
run_case -nv file.txt loop
# The destination whose mode cannot be read at all. GNU's question here is
# built from an uninitialised `struct stat`, so this case pins only that the
# two agree about the *outcome*; see `overwrite_ok` in `cp.rs`.
TREE='mktree; ln -s loop loop'
ANSWERS='y'$'
'
run_case -iv file.txt loop

# --- against a directory destination, which none of the four removes ---------
# `-T` is what makes the destination a name to be replaced rather than a place
# to copy into, so it is the only way to aim these at a directory at all. None
# of them unlinks it: `-f` and `--remove-destination` both stop at "is a
# directory", `-n` refuses first, and `-i` asks first and then stops there too.
run_case -Tv file.txt dir
run_case -Tfv file.txt dir
run_case -T --remove-destination -v file.txt dir
run_case -Tnv file.txt dir
# The question comes *before* the "cannot overwrite directory" refusal, so a
# `y` gets both on one line of stderr and a `n` gets only the question. That
# ordering is the point of the pair.
ANSWERS='y'$'
'
run_case -Tiv file.txt dir
ANSWERS='n'$'
'
run_case -Tiv file.txt dir

# --- inside a recursive copy -------------------------------------------------
# The walked entries go through the same four policies as the operands, which
# is only visible when the destination tree already has the names in it. The
# pre-made `dst/tree` carries a symlink, and replacing an existing symlink is a
# thing this `cp` used to fail at outright -- `known-issues.md` ->
# `B-CP-R-COULD-NOT-REPLACE-AN-EXISTING-SYMLINK`.
PRE='mkdir -p dst/tree/sub; printf x > dst/tree/a.txt
     printf y > dst/tree/sub/b.txt; ln -s elsewhere dst/tree/link'
TREE="mktree; $PRE"
run_case -rv tree dst
TREE="mktree; $PRE"
run_case -rvn tree dst
TREE="mktree; $PRE"
run_case -rfv tree dst
TREE="mktree; $PRE"
run_case -rv --remove-destination tree dst
# `-i` is asked per entry the walk finds, not once for the operand, and the
# directories themselves are exempt -- so the number of questions is the number
# of non-directory names the destination already had.
TREE="mktree; $PRE"
ANSWERS='y'$'
''y'$'
''y'$'
'
run_case -riv tree dst
TREE="mktree; $PRE"
ANSWERS='n'$'
''n'$'
''n'$'
'
run_case -riv tree dst
TREE="mktree; $PRE"
ANSWERS='y'$'
''n'$'
'
run_case -riv tree dst
# The second copy of the same tree, which is the shape the symlink bug was
# found in: everything already exists and every entry takes the overwrite path.
TREE='mktree; mkdir -p dst2/tree/sub; printf a > dst2/tree/a.txt
      printf bb > dst2/tree/sub/b.txt; ln -s a.txt dst2/tree/link'
run_case -rv tree dst2
# A kind mismatch under the walk: a directory landing where a file is, and a
# file landing where a directory is. Both are refusals, and the wording is not
# the one the underlying `mkdir`/`open` would give.
TREE='mktree; mkdir -p dst/tree; printf x > dst/tree/sub'
run_case -rv tree dst
TREE='mktree; mkdir -p dst/tree/a.txt'
run_case -rv tree dst
TREE='mktree; mkdir -p dst/tree; printf x > dst/tree/sub'
run_case -rfv tree dst
TREE='mktree; mkdir -p dst/tree; printf x > dst/tree/sub'
run_case -rv --remove-destination tree dst
# The question again precedes the kind-mismatch refusal, in both directions.
TREE='mktree; mkdir -p dst/tree; printf x > dst/tree/sub'
ANSWERS='y'$'
''y'$'
'
run_case -riv tree dst
TREE='mktree; mkdir -p dst/tree/a.txt'
ANSWERS='y'$'
''y'$'
'
run_case -riv tree dst

# --- the four together -------------------------------------------------------
# They are not mutually exclusive and GNU does not reject any pairing; what it
# does with each is measured rather than reasoned about. `-i` and `-n` are the
# exception in that they are one field, so the last of *those two* wins while
# `-f` and `--remove-destination` accumulate alongside whichever won.
TREE='mktree; printf old > old.txt'
run_case -fnv file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case -nfv file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case -f --remove-destination -v file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case -n --remove-destination -v file.txt old.txt
TREE='mktree; printf old > old.txt'
run_case --remove-destination -nv file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS='y'$'
'
run_case -i --remove-destination -v file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS='n'$'
'
run_case -i --remove-destination -v file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS='y'$'
'
run_case -fiv file.txt old.txt
TREE='mktree; printf old > old.txt'
ANSWERS='y'$'
'
run_case -f --remove-destination -inv file.txt old.txt

# =============================================================================
# 17. -p, --preserve and --no-preserve
# =============================================================================
# The only section that compares modification times, and the only one that can:
# `mkstamped` pins every path the fixture creates to a fixed instant, so the
# expected value is a constant instead of whenever the harness ran. `STAMPS=1`
# turns the column on; see `snapshot`.
#
# Each path gets a *different* instant, and the nanoseconds of each are
# different too. Both matter. One shared instant would let a copy that stamped
# every destination with the first source's time pass, and a whole-second value
# would let one that rounded the nanoseconds away pass — which is the exact
# shape of the bug a `utimensat` fed `st_mtime` instead of `st_mtim` produces.
#
# The order below is children first, then their directory: writing into a
# directory moves its own modification time, so stamping `tree` before
# `tree/a.txt` would leave `tree` carrying the time of the `touch` and not the
# time asked for. `dir` is stamped too although nothing is copied into it —
# `mktree` creates it, so it is in every snapshot, and an unstamped empty
# directory carries the time the fixture ran, which is a different instant on
# each side.
mkstamped() {
  mktree
  touch -d '2001-02-03 04:05:06.123456789' tree/a.txt
  touch -d '2002-03-04 05:06:07.987654321' tree/sub/b.txt
  touch -h -d '2003-04-05 06:07:08.192837465' tree/link
  touch -d '2004-05-06 07:08:09.564738291' tree/sub
  touch -d '2005-06-07 08:09:10.918273645' tree
  touch -d '2006-07-08 09:10:11.246813579' file.txt
  touch -d '2007-08-09 10:11:12.135792468' dir
}

# The three POSIX attributes together, which is what `-p` and a bare
# `--preserve` both mean.
STAMPS=1 TREE='mkstamped'
run_case -p file.txt new.txt
STAMPS=1 TREE='mkstamped'
run_case --preserve file.txt new.txt
STAMPS=1 TREE='mkstamped'
run_case -p -r tree dst
STAMPS=1 TREE='mkstamped'
run_case --preserve=mode,ownership,timestamps -r tree dst

# One attribute at a time, so that a `-p` which quietly does all three whatever
# it was asked for cannot pass. The mode column and the time column move
# independently here and in no other section.
STAMPS=1 TREE='mkstamped; chmod 741 file.txt'
run_case --preserve=mode file.txt new.txt
STAMPS=1 TREE='mkstamped; chmod 741 file.txt'
run_case --preserve=timestamps file.txt new.txt
STAMPS=1 TREE='mkstamped; chmod 741 file.txt'
run_case --preserve=ownership file.txt new.txt
# `--preserve=ownership` is the one that withholds group and other permissions
# at creation and puts them back afterwards — GNU's `omitted_permissions`, which
# is `src & 0077` under that option and `src & 0022` for a directory without it.
# A copy that skipped the putting-back leaves 0700 where 0741 belongs.
STAMPS=1 TREE='mkstamped; chmod 741 tree; chmod 775 tree/sub'
run_case --preserve=ownership -r tree dst

# Abbreviated words. Every one of the seven has a distinct first letter, so a
# single character is unambiguous — and `--preserve=m` reaching `mode` is the
# difference between gnulib's `XARGMATCH` and a table lookup.
STAMPS=1 TREE='mkstamped; chmod 741 file.txt'
run_case --preserve=m,t file.txt new.txt
STAMPS=1 TREE='mkstamped'
run_case --preserve=timestamp file.txt new.txt

# The mode `-p` restores is the source's whole 07777 and is *not* narrowed by
# the umask — the one thing separating a preserved mode from a fresh file's.
STAMPS=1 TREE='mkstamped; chmod 4755 file.txt'
run_case -p file.txt new.txt
STAMPS=1 TREE='mkstamped; chmod 2755 file.txt'
run_case -p file.txt new.txt
STAMPS=1 TREE='mkstamped; chmod 777 file.txt'
run_case -p file.txt new.txt
STAMPS=1 TREE='mkstamped; chmod 1777 tree'
run_case -p -r tree dst
# 0500: no owner-write, so the copy has to be filled before the mode goes on —
# and the mode has to go on after the ownership, not instead of it.
STAMPS=1 TREE='mkstamped; chmod 500 tree'
run_case -p -r tree dst
STAMPS=1 TREE='mkstamped; chmod 400 tree/a.txt'
run_case -p -r tree dst

# An existing destination. `-p` replaces its mode, where a plain copy leaves it
# alone — section 10 asserts the second half and this asserts the first.
STAMPS=1 TREE='mkstamped; printf old > old.txt; chmod 600 old.txt; touch -d "2010-01-01 00:00:00" old.txt'
run_case -p file.txt old.txt
STAMPS=1 TREE='mkstamped; chmod 4755 file.txt; printf old > old.txt; chmod 600 old.txt; touch -d "2010-01-01 00:00:00" old.txt'
run_case -p file.txt old.txt
STAMPS=1 TREE='mkstamped; chmod 600 file.txt; printf old > old.txt; chmod 777 old.txt; touch -d "2010-01-01 00:00:00" old.txt'
run_case -p file.txt old.txt

# A symbolic link copied as itself. The link's own times are preserved and its
# mode is not touched at all — nothing portable can chmod a link, and GNU
# returns before its mode block for exactly that reason.
STAMPS=1 TREE='mkstamped'
run_case -P -p tree/link dst
STAMPS=1 TREE='mkstamped'
run_case -p -r tree dst2

# --no-preserve. On a destination this run created it is not the same as not
# having asked: `--no-preserve=mode` gives 0666 (0777 for a directory) less the
# umask, where a plain copy gives the source's mode less the umask.
STAMPS=1 TREE='mkstamped; chmod 700 file.txt'
run_case --no-preserve=mode file.txt new.txt
STAMPS=1 TREE='mkstamped; chmod 700 tree'
run_case -r --no-preserve=mode tree dst
STAMPS=1 TREE='mkstamped; chmod 700 file.txt; printf old > old.txt; chmod 600 old.txt'
run_case --no-preserve=mode file.txt old.txt
# Order decides, because each word is applied as it is read.
STAMPS=1 TREE='mkstamped; chmod 700 file.txt'
run_case -p --no-preserve=mode file.txt new.txt
STAMPS=1 TREE='mkstamped; chmod 700 file.txt'
run_case --no-preserve=mode -p file.txt new.txt
STAMPS=1 TREE='mkstamped; chmod 700 file.txt'
run_case -p --no-preserve=timestamps file.txt new.txt
STAMPS=1 TREE='mkstamped; chmod 700 file.txt'
run_case --preserve=mode --no-preserve=mode,timestamps file.txt new.txt
# `--no-preserve=all` turns off the three that exist and is accepted for the
# four that do not: refusing a word there would be refusing an instruction the
# program has already obeyed by never having done the thing.
STAMPS=1 TREE='mkstamped; chmod 700 file.txt'
run_case -p --no-preserve=all file.txt new.txt
STAMPS=1 TREE='mkstamped'
run_case --no-preserve=links,xattr,context file.txt new.txt

# Words and lists that are refused, by both programs and in the same sentence.
run_case --preserve=bogus file.txt new.txt
run_case --preserve=xyz file.txt new.txt
run_case --preserve= file.txt new.txt
run_case --no-preserve= file.txt new.txt
run_case --preserve=mode,bogus file.txt new.txt
# `--no-preserve` takes a required argument, so a trailing one has nothing to
# take and both programs say so before looking at the operands.
run_case file.txt new.txt --no-preserve
run_case file.txt new.txt --preserve=

# =============================================================================
# 18. Options GNU has and this cp has not
# =============================================================================
# An inventory, one line per option, kept as `xfail` so that the count is
# visible in the summary and so that `xpass` fires the moment one is
# implemented. Every one of these is *refused* by ours rather than ignored;
# `cp.rs`'s module docs give the reason, which is that each one ignored
# produces a destination that looks right and is not.

missing -a tree dst
missing --archive tree dst
missing --attributes-only file.txt new.txt
missing -b file.txt tree/a.txt
missing --backup file.txt tree/a.txt
missing --backup=numbered file.txt tree/a.txt
missing --copy-contents -r tree dst
missing --debug file.txt new.txt
# `-d` outlives `-P`, which it contains: GNU's `-d` is `--no-dereference`
# *and* `--preserve=links`, and honouring only the half that exists would turn
# two hard-linked sources into two independent copies with nothing said.
missing -d tree/link dst
missing -l file.txt new.txt
missing --link file.txt new.txt
missing --one-file-system -r tree dst
# The four `--preserve` words this cp has not, refused one word at a time
# rather than by the option — `--preserve=mode` is honoured in the section
# above and refusing it because `xattr` exists would be a lie. `--preserve=all`
# is refused for containing `links`, which is the only one of the four that
# changes what ends up on disk rather than what is attached to it.
missing --preserve=links tree/a.txt new.txt
missing --preserve=xattr file.txt new.txt
missing --preserve=context file.txt new.txt
missing --preserve=all -r tree dst
missing --preserve=mode,links file.txt new.txt
missing --parents tree/a.txt dir
missing --path tree/a.txt dir
missing --reflink=auto file.txt new.txt
missing -s file.txt new.txt
missing --symbolic-link file.txt new.txt
missing --sparse=auto file.txt new.txt
missing -S .bak -b file.txt tree/a.txt
missing --suffix=.bak -b file.txt tree/a.txt
missing --strip-trailing-slashes tree/ dst
missing -u file.txt tree/a.txt
missing --update file.txt tree/a.txt
missing -x -r tree dst
missing -Z file.txt new.txt
missing --context file.txt new.txt

# =============================================================================
# 19. --help and --version
# =============================================================================

xfail_case 'help omits GNU bug-report block' --help
xfail_case 'version names SlateOS' --version

# The wording is the family's, not this harness's own: `scripts/all-diff.sh`
# decides green by matching " 0 differed" in the tail line, so a summary that
# said "0 failed" would be reported as a failing harness forever.
printf '\ncp: %d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER differ (update the harness)' "$xpass"
printf '\n'
[ "$fail" -eq 0 ] || exit 1
exit 0
