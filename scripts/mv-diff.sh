#!/usr/bin/env bash
# Differential test: our `mv` against GNU coreutils'.
#
# ## What is compared
#
# The same five things `cp-diff.sh` compares, and for the same reasons:
# standard output, standard error, the exit status, *what the case directory
# holds afterwards* — every surviving path with its octal mode and size, every
# symlink with the text it points at, and the bytes of every regular file — and
# *which of those paths are one file under two names*.
#
# The tree matters more here than anywhere else in this family. `mv`'s stdout is
# empty in every case but `-v`'s, and its whole observable effect is a rename:
# what moved, what did not, and what was destroyed on the way. A text-only
# harness would compare two empty strings a hundred times and certify nothing.
#
# `hardlinks` earns its place for a reason `cp`'s does not. `mv` is *supposed*
# to keep a hard-link group intact — a rename does not touch the inode — so the
# column is the only thing that can tell a rename from a copy-and-unlink that
# happened to produce the same bytes. That distinction is exactly what the
# `EXDEV` fallback is, and it is the part of this program least covered by
# anything else.
#
# Timestamps are compared in the `STAMPS` sections, which pin their fixture's
# times with `touch -d`. For `mv` this is not the side-issue it is for `cp`: a
# same-filesystem move keeps the inode, so *every* attribute survives by
# construction and the column is a constant rather than a coin-flip. A case
# where it stops being constant has found something.
#
# ## What this harness cannot do, and it is the interesting half
#
# **No case moves across a filesystem boundary.** The `EXDEV` fallback — copy,
# then unlink — is the part of `mv` that has to reproduce by hand everything a
# rename gets for free: the mode, the times, the ownership, the symlink's text
# rather than its target's bytes, and a directory's whole subtree. It is also
# the part with the most room to be quietly wrong, and three of the four bugs in
# `mv.rs`'s module docs lived there.
#
# Reaching it needs a second filesystem, which needs `mount`, which needs a
# privilege this harness must not ask for: the WSL user is uid 1000 and `sudo`
# wants a password. A harness that prompts for one is a harness that hangs in
# every unattended run.
#
# So the fallback is covered by `mv.rs`'s own unit tests, which can at least
# drive `copy_across_devices` directly, and the gap is written down here rather
# than left to be discovered. If a loop device or a user-namespace `tmpfs` ever
# becomes available without a password, the cases to add are: a regular file
# with a non-default mode and a pinned mtime, a symlink (relative, resolving),
# a dangling symlink, a hard-link pair moved together, and a directory.
#
# **No case moves a FIFO or a device**, for `cp-diff.sh`'s reason: reading a
# FIFO nothing writes to blocks until the timeout, which certifies a hang.
#
# **No case names a bare `..` or uses `..` as a destination.** A `mv` whose
# destination lies outside the case directory writes into the harness's own
# scratch tree. Sources ending in `..` are always written `tree/..`, which
# resolves to the case directory itself, so the blast radius is that one
# directory. This is the shape bug 3 in `mv.rs` was about, and the cases exist
# to certify it stays refused.
#
# ## Cases that differ on purpose
#
# Two kinds. The family's two — `--help` omits the GNU project's `Report bugs
# to:` block, and `--version` names SlateOS — then one per option that GNU has
# and this `mv` has not.
#
# The second group is an inventory, not a permission. `xfail_case` reports an
# XPASS the moment one starts agreeing, which is what forces an entry to be
# promoted to a real case as its option lands. `mv.rs`'s module docs explain why
# those options are *refused* rather than ignored: silently ignoring `-n` would
# overwrite a file the user asked to be left alone, and for this utility that is
# unrecoverable.
#
# ## The reference is built, not found
#
# `DIFF_GNU_SOURCE=9.4` makes `diff-wsl.sh` fetch and build coreutils 9.4 rather
# than compare against `/usr/bin/mv`. The reason is `cp-diff.sh`'s and it
# applies here with the same force: Ubuntu carries `debian/patches/cp-n.diff`,
# which makes `-n` a silent success on an existing destination where upstream
# prints `not replacing 'b'` and exits 1. `mv -n` is a shared code path with
# `cp -n` — both are `x.interactive = I_ALWAYS_NO` read by `copy.c` — so
# certifying `mv -n` against the installed binary would certify us into Debian's
# behaviour and look green doing it. See `diff-wsl.sh`'s "Why a built reference"
# and `design-decisions.md` §726.
#
# The version is pinned to the one Ubuntu ships, so that when a case's result
# changes, de-patching is the only thing it can be attributed to. Note also that
# `--exchange` and `--no-copy`'s behaviour are 9.5-era: `LONG_OPTIONS` in
# `mv.rs` tracks 9.4 deliberately, and that table was once wrong in both
# directions for exactly this reason (see its doc comment).
#
# Run `OURS=/usr/bin/mv ./scripts/mv-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else. With a
# built reference that check compares Ubuntu's `mv` against upstream's, so `-n`
# cases will legitimately differ rather than XPASS.
set -u

DIFF_PROG='mv'
# Not `/usr/bin/mv`; see "The reference is built, not found" above.
DIFF_GNU_SOURCE=9.4
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

work=$DIFF_TMP/work
mkdir -p "$work"

case_no=0

# --- the fixture --------------------------------------------------------------
# A small tree, a loose file, an empty directory, and one symlink, built in a
# fixed order — the same shape `cp-diff.sh` uses, so a reader who knows one
# knows the other.
#
# The order is load-bearing rather than tidy, for the reason spelled out there:
# both programs walk a directory in inode order, which on a fresh ext4 directory
# is creation order, so two case directories built by the same commands in the
# same sequence enumerate identically. That is what lets the diagnostics from a
# partly-failing multi-source move be compared line for line.
#
# `tree/link` points at a *sibling*, relatively. For `mv` that is the shape that
# says whether the link was moved as itself: moved, it still reads `-> a.txt`
# and resolves in its new directory only if `a.txt` is there too; dereferenced,
# it becomes a second copy of `a.txt`'s bytes with no `->` column at all.
mktree() {
  mkdir -p tree/sub
  printf 'a\n' > tree/a.txt
  printf 'bb\n' > tree/sub/b.txt
  ln -s a.txt tree/link
  printf 'top\n' > file.txt
  mkdir dir
}

# --- what a case leaves behind ------------------------------------------------
# Every path, its octal mode and its size — except a directory, whose size is
# that of the block holding its entries and so varies with what it *used* to
# hold (`d` stands in), and a symlink, whose mode is 0777 on Linux always and
# whose size is the length of its text (the text is printed instead).
#
# Errors are discarded and are meant to be: a case that leaves an unreadable
# directory leaves one on both sides, so `find` fails to descend on both sides
# and the blind spot is symmetric. The unreadable directory itself is still
# listed, with its mode.
#
# `%T@` and not a formatted time, for `cp-diff.sh`'s reason: seconds-and-
# fraction since the epoch is what `utimensat` carries, and a formatted column
# would hide a move that got the seconds right and threw the nanoseconds away.
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
# The stamped cases pin everything they make to 2001-2007; the cutoff is 2011
# and the harness runs long after it.
STAMP_CUTOFF=1300000000

# Rewrite a timestamp the fixture did not set as the literal `now`.
#
# Two things in a stamped case legitimately carry the moment the harness ran:
# the *directory* a file was moved into, whose mtime is bumped by the new entry,
# and any file the case itself creates without a `touch -d`. Comparing those raw
# would fail on the one thing that cannot agree, since the two sides run one
# after the other. Folding says which of the two happened without saying when,
# so a moved file's pinned mtime is still compared exactly — which is the whole
# point, since a `mv` that preserved it is a rename and one that did not is a
# copy wearing a rename's name.
fold_now() {
  if [ -z "$STAMPS" ]; then cat; return; fi
  awk -v cut="$STAMP_CUTOFF" '
    { if ($NF ~ /^[0-9]+\.[0-9]+$/ && $NF + 0 > cut) sub(/[^ ]+$/, "now"); print }'
}

# Which paths are one file under two names: one line per hard-link group,
# listing that group's members. Empty when the case made no hard links.
#
# The names and not the inode numbers, because the numbers are the one thing
# about a hard link that cannot be compared across the two sides — the two case
# directories are two different sets of inodes. Printing the group's membership
# says the only thing the harness needs to know, and says it identically on both
# sides.
#
# Sorted twice: by name before grouping so a group's members are listed in a
# fixed order, and by line afterwards so the groups themselves are.
hardlinks() {
  ( cd "$1" 2>/dev/null || return 0
    find . -mindepth 1 ! -type d -links +1 -printf '%i\t%P\n' 2>/dev/null \
      | LC_ALL=C sort -t"$(printf '\t')" -k2 \
      | awk -F'\t' '{ g[$1] = ($1 in g) ? g[$1] " " $2 : $2 }
                    END { for (k in g) print g[k] }' \
      | LC_ALL=C sort )
}

# And the bytes, so that a file which arrived with the right size and the wrong
# contents is still caught. `-type f` does not follow, so a symlink is not read
# here — its target is already in the snapshot above.
#
# NUL-separated and sorted with `sort -z`, because a case below creates a file
# whose name holds a newline. Read line by line that name arrives as two names,
# neither of which exists, so `cat` fails on both sides and the file's bytes are
# never compared at all — a blind spot that announces itself on the harness's
# own stderr and is still a blind spot.
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
# What `mv` reads on standard input, for `-i`'s prompts. Empty is end of input
# straight away, which is what every case that does not prompt gets. Written
# with `$'\n'` escapes rather than real newlines so a case's answers stay on the
# case's own line and can be read next to its options.
ANSWERS=
# Non-empty makes [`snapshot`] print each path's modification time. Set by the
# cases whose fixture pins those times.
STAMPS=
# `VAR=value` words placed in the environment of both sides. Two variables reach
# this program, both the backup section's: `VERSION_CONTROL` supplies the word
# that `--backup` was not given, and `SIMPLE_BACKUP_SUFFIX` the suffix that `-S`
# was not. They are knobs rather than a fixed export because each is *overridden*
# by its option, so the interesting cases are the pairs.
ENVV=()
reset_knobs() { TREE='mktree'; ANSWERS=''; STAMPS=''; ENVV=(); }
reset_knobs

# A fixture whose every path carries a pinned time, for the `STAMPS` cases.
# Distinct years so that a swap between two of them is visible rather than
# merely a different constant.
mkstamped() {
  mktree
  touch -d '2001-01-01 01:01:01.111111111' tree/a.txt
  touch -d '2002-02-02 02:02:02.222222222' tree/sub/b.txt
  touch -h -d '2003-03-03 03:03:03.333333333' tree/link
  touch -d '2004-04-04 04:04:04.444444444' file.txt
  touch -d '2005-05-05 05:05:05.555555555' tree/sub
  touch -d '2006-06-06 06:06:06.666666666' tree
  touch -d '2007-07-07 07:07:07.777777777' dir
}

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
  # One file per side rather than one shared one: the two sides run one after
  # the other and a shared file would be consumed by whichever ran first.
  local answers=$dir.stdin
  printf '%s' "$ANSWERS" >"$answers"
  (
    # `$out`/`$err` are absolute: they are opened after this `cd`.
    cd "$dir" || exit 1
    # Reached as the bare word `mv`, through the one-entry directory
    # `diff-wsl.sh` built, and *not* by the path of the symlink. gnulib's
    # `set_program_name` takes `argv[0]` whole, so GNU invoked as
    # `/tmp/xxx/bin/gnu/mv` prefixes every diagnostic with that entire path
    # while ours prints `mv:`, and every case that says anything at all differs
    # for a reason that has nothing to do with either program. Prepended rather
    # than replacing `PATH`, so `timeout` is still findable.
    PATH="$bindir/$side:$PATH"
    # `env` and not an assignment prefix, so that [`ENVV`] can hold a variable
    # whose *name* is chosen by the case rather than by this line.
    diff_run timeout -k 2 30 env "${ENVV[@]}" mv "$@" >"$out" 2>"$err"
  ) <"$answers"
  echo $? >"$rcf"
  return 0
}

# --- comparing the two sides --------------------------------------------------

judge() {
  local o_dir=$1 g_dir=$2 o_out=$3 g_out=$4 o_extra=$5 g_extra=$6 label=$7
  local o_snap g_snap o_body g_body o_show g_show o_link g_link
  o_snap=$(snapshot "$o_dir"); g_snap=$(snapshot "$g_dir")
  o_link=$(hardlinks "$o_dir"); g_link=$(hardlinks "$g_dir")
  o_body=$(contents "$o_dir" | scrub "$o_dir"); g_body=$(contents "$g_dir" | scrub "$g_dir")
  o_show=$(scrub "$o_dir" <"$o_out"); g_show=$(scrub "$g_dir" <"$g_out")
  o_extra=$(printf '%s' "$o_extra" | scrub "$o_dir")
  g_extra=$(printf '%s' "$g_extra" | scrub "$g_dir")

  if [ "$o_show" = "$g_show" ] && [ "$o_extra" = "$g_extra" ] \
     && [ "$o_snap" = "$g_snap" ] && [ "$o_body" = "$g_body" ] \
     && [ "$o_link" = "$g_link" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: %s\n        out{%s}\n        tree{%s} files{%s} links{%s}\n  gnu : %s\n        out{%s}\n        tree{%s} files{%s} links{%s}' \
    "$(printf '%s' "$o_extra" | tr '\n' '|')" "$(printf '%s' "$o_show" | tr '\n' '|')" \
    "$(printf '%s' "$o_snap" | tr '\n' '|')" "$(printf '%s' "$o_body" | tr '\n' '|')" \
    "$(printf '%s' "$o_link" | tr '\n' '|')" \
    "$(printf '%s' "$g_extra" | tr '\n' '|')" "$(printf '%s' "$g_show" | tr '\n' '|')" \
    "$(printf '%s' "$g_snap" | tr '\n' '|')" "$(printf '%s' "$g_body" | tr '\n' '|')" \
    "$(printf '%s' "$g_link" | tr '\n' '|')")
  LABEL=$label
}

compare() {
  case_no=$((case_no+1))
  local o_dir=$work/o$case_no g_dir=$work/g$case_no
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  local o_err=$work/oe$case_no g_err=$work/ge$case_no
  local o_rc=$work/or$case_no g_rc=$work/gr$case_no
  local label="mv $*"
  [ -z "$ANSWERS" ] || label="$label   [in: ${ANSWERS//$'\n'/\\n}]"
  [ "$TREE" = mktree ] || label="$label   [tree: $TREE]"
  [ ${#ENVV[@]} -eq 0 ] || label="$label   [env: ${ENVV[*]}]"
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
# and for the "not implemented" block below it is the signal that an option has
# landed and its case is now a real one.
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

# Shorthand for the inventory blocks: an option GNU acts on and this `mv`
# refuses.
missing() { xfail_case "not implemented by this mv" "$@"; }

echo "mv-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real"

# =============================================================================
# 1. Too few operands
# =============================================================================
# Zero and one are distinct diagnostics: "missing file operand" alone left the
# user to work out *which* operand was missing, and GNU names the one it did
# get. An empty string is an operand — it names no file, so one empty operand is
# still "missing destination".

run_case
run_case file.txt
run_case ''
run_case '' ''

# =============================================================================
# 2. Option errors, and where the option table is observable
# =============================================================================
# The abbreviation cases are the reason `LONG_OPTIONS` holds all fifteen entries
# rather than the two this implementation acts on: `getopt_long` lists an
# ambiguous prefix's candidates in table order, so an absent entry changes the
# text of an error about a *different* option. This table was once wrong in both
# directions; see its doc comment in `mv.rs`.

run_case -w file.txt dst
run_case --nosuchoption file.txt dst
run_case -- file.txt dst
run_case --force=yes file.txt dst
run_case --f file.txt dst
run_case --fo file.txt dst
# `--n` is ambiguous three ways — `no-clobber`, `no-copy`,
# `no-target-directory` — and `--no-c` two ways, which is the pair the missing
# `no-copy` entry used to get wrong.
run_case --n file.txt dst
run_case --no-c file.txt dst
# `--no-cl` is unambiguous, so it *resolves* — which made it a test of
# `--no-clobber`'s behaviour rather than of the abbreviation table, and it was
# an xfail until that option existed. It is a plain case now, and the fixture is
# an existing destination so that what it resolves *to* is visible: an
# abbreviation that resolved to the wrong option would otherwise agree with GNU
# by both doing nothing.
TREE='mktree; printf old > dst'
run_case --no-cl file.txt dst
run_case --=x file.txt dst
# A `-` on its own is a file called `-`, not a request to read standard input:
# `mv` has no standard-input operand for it to mean anything else.
run_case - dst
TREE='mktree; printf dash > ./-'
run_case ./- dst
run_case -- -foo dst
TREE='mktree; printf dash > ./-foo'
run_case -- -foo dst
# Options after an operand: `getopt_long` permutes by default, so this is
# `mv -f file.txt dst`.
run_case file.txt -f dst

# =============================================================================
# 3. Renaming one regular file
# =============================================================================

run_case file.txt new.txt
run_case file.txt ./new.txt
run_case file.txt dir/../new.txt
run_case tree/a.txt new.txt
run_case tree/sub/b.txt new.txt

# An empty source, and one with no trailing newline: both are ways for a move
# that got the size right to have got the bytes wrong.
TREE='mktree; : > empty'
run_case empty new.txt
TREE='mktree; printf nonl > partial'
run_case partial new.txt

# Where the destination cannot be created.
run_case file.txt nosuchdir/new.txt
run_case nosuchfile new.txt
run_case nosuchfile dir

# A source that is a dangling symlink moves as itself, and the diagnostic for a
# missing source must not be produced for it: the link is right there.
TREE='mktree; ln -s nowhere dangling'
run_case dangling moved
TREE='mktree; ln -s nowhere dangling'
run_case dangling dir

# =============================================================================
# 4. Into a directory
# =============================================================================
# Without `-T` the last operand being a directory decides the shape of the whole
# command, so each of the ways of writing one is its own case.

run_case file.txt dir
run_case file.txt dir/
run_case file.txt dir/.
run_case tree/a.txt dir
run_case tree/link dir
run_case tree dir
# Onto an existing name inside the directory.
TREE='mktree; printf old > dir/file.txt'
run_case file.txt dir
# A destination that exists and is not a directory, with a directory source.
run_case tree file.txt
# A symlink to a directory, which `mv` follows for the *destination* and not for
# the source.
TREE='mktree; ln -s dir dirlink'
run_case file.txt dirlink
TREE='mktree; ln -s dir dirlink'
run_case file.txt dirlink/

# =============================================================================
# 5. More than one source
# =============================================================================
# The last operand must be a directory. When it is not, GNU's diagnostic names
# it, and when one source of several fails the rest still move — the exit status
# is 1 and the tree shows the difference.

run_case file.txt tree/a.txt dir
# Three operands whose last is not a directory. The diagnostic is `target %s`
# followed by the *errno* from the failed `stat`, not a sentence of `mv`'s own,
# so the three ways of not being a directory read differently: absent is ENOENT,
# an existing regular file is ENOTDIR, and a dangling symlink is ENOENT again
# because the target operand is followed.
run_case file.txt tree/a.txt new.txt
TREE='mktree; printf blocked > taken'
run_case file.txt tree/a.txt taken
TREE='mktree; ln -s nowhere dangling'
run_case file.txt tree/a.txt dangling
# A symlink *to* a directory is a directory here, for the same reason: the
# operand is followed, so both sources land inside `dir`.
TREE='mktree; ln -s dir dirlink'
run_case file.txt tree/a.txt dirlink
run_case file.txt nosuchfile tree/a.txt dir
run_case nosuchfile file.txt dir
run_case file.txt file.txt dir
# Two sources with the same basename land on one name, and GNU refuses the
# second rather than let the first be lost — `will not overwrite just-created`.
# The check is a set of `(name, device, inode)` triples recorded as each move
# succeeds, so it is not "did two operands share a basename" but "is the thing
# now sitting there the thing I just put there".
TREE='mktree; mkdir -p one two; printf 1 > one/same; printf 2 > two/same'
run_case one/same two/same dir
# Three of them, so the refusal has to survive being reached twice.
TREE='mktree; mkdir -p one two three
printf 1 > one/same; printf 2 > two/same; printf 3 > three/same'
run_case one/same two/same three/same dir
# The same basename, but the destination was *already* there rather than
# just-created: nothing was recorded for it, so this overwrites silently.
TREE='mktree; mkdir -p one; printf 1 > one/same; printf old > dir/same'
run_case one/same dir
# A destination in the way that is a directory, and one that is a non-empty
# directory — the two refusals that are not about identity at all.
TREE='mktree; mkdir -p dir/file.txt'
run_case file.txt dir
TREE='mktree; mkdir -p dir/tree/x'
run_case tree dir
TREE='mktree; mkdir -p dir/tree'
run_case tree dir
# Every source missing.
run_case nosuchfile alsomissing dir

# =============================================================================
# 6. Directories
# =============================================================================
# A directory moves whole, by rename, with everything under it — including the
# symlink, whose text must still read `-> a.txt` afterwards.

run_case tree moved
run_case tree/sub moved
run_case dir moved
# Onto an existing empty directory. It is worth being clear about what this
# does *not* do, because the shape of `rename(2)` suggests otherwise: an empty
# directory destination is replaceable at the syscall level, but `mv` never
# reaches that syscall with `empty` as the target. `target_directory_operand`
# succeeds on any destination that is a directory, so the target becomes
# `empty/tree` and `empty` survives with the tree inside it. Replacing the
# directory itself is what `-T` is for, and that case is in section 15.
TREE='mktree; mkdir empty'
run_case tree empty
# Onto an existing *non*-empty directory, which `rename` refuses with ENOTEMPTY.
TREE='mktree; mkdir -p full/x'
run_case tree full
# Onto a name inside itself.
run_case tree tree/sub
run_case tree tree/sub/inner
# A directory onto a file, and a file onto a directory.
run_case dir file.txt
TREE='mktree; mkdir -p full/x'
run_case file.txt full

# =============================================================================
# 7. A source that names nothing to create
# =============================================================================
# `mv a/.. dst` asked the previous implementation to rename `a`'s *parent*, and
# was reachable from an ordinary glob. See `mv.rs` module docs, bug 3. `tree/..`
# and not a bare `..`, so a regression cannot escape the case directory.

run_case tree/.. dir
run_case tree/. dir
run_case tree/.. moved
run_case tree/. moved
run_case tree/ dir
run_case tree// dir
run_case tree/sub/ dir
# A `.` further down, and one whose parent is the destination's own parent: the
# component is appended verbatim, so these land on `dir/.` and `dir/..` and are
# judged from there rather than from the source's spelling.
run_case tree/sub/. dir
run_case tree/sub/.. dir
run_case ./. dir
# The same components where the destination is *not* a directory, which takes
# the other branch of the target computation entirely — the destination is used
# as given and the last component is never consulted.
run_case tree/. nosuchdir/x
run_case tree/.. nosuchdir/x

# =============================================================================
# 8. Moving something onto itself
# =============================================================================
# Every spelling of the same file, which GNU catches by device-and-inode rather
# than by comparing the two strings.

run_case file.txt file.txt
run_case file.txt ./file.txt
run_case ./file.txt file.txt
run_case tree tree
run_case tree/a.txt tree/a.txt
TREE='mktree; ln file.txt hardlink'
run_case file.txt hardlink
TREE='mktree; ln -s file.txt softlink'
run_case file.txt softlink
TREE='mktree; ln -s file.txt softlink'
run_case softlink file.txt
# A chain: `s2 -> s1 -> file.txt`. The refusal is on where the source *resolves*,
# not on the one link it holds, so this must be refused too.
TREE='mktree; ln -s file.txt s1; ln -s s1 s2'
run_case s2 file.txt
# Two distinct symlinks to one file. Both sides are links, so what matters is
# whether they are the same *link* — they are not, and replacing one link with
# the other touches nothing either points at.
TREE='mktree; ln -s file.txt s1; ln -s file.txt s2'
run_case s1 s2
# The example GNU spells out in `same_file_ok`'s own comment (`copy.c:1907`):
#   touch f && ln f l && ln -s f s
# `mv s f` must fail — it would destroy the only thing `s` names — while
# `mv s l` must succeed, because `f` survives as the other link.
TREE='mktree; : > f; ln f l; ln -s f s'
run_case s f
TREE='mktree; : > f; ln f l; ln -s f s'
run_case s l
# And the same pair with the link count back down to one, where `mv s l` becomes
# the refusal that `mv s f` was.
TREE='mktree; : > f; ln -s f s'
run_case s f

# =============================================================================
# 9. Overwriting, and when it is refused
# =============================================================================

TREE='mktree; printf old > dst'
run_case file.txt dst
# An unwritable destination file in a writable directory: `rename` does not care
# about the destination's own mode, only the directory's, so this succeeds.
TREE='mktree; printf old > dst; chmod 0444 dst'
run_case file.txt dst
# An unwritable *directory*, which is the one that stops it.
TREE='mktree; mkdir ro; chmod 0555 ro'
run_case file.txt ro
TREE='mktree; mkdir ro; printf old > ro/dst; chmod 0555 ro'
run_case file.txt ro/dst
# An unreadable source directory still renames: the walk never happens.
TREE='mktree; chmod 0000 tree/sub'
run_case tree moved

# =============================================================================
# 10. Names that are hard to hold
# =============================================================================
# A newline in a name is why [`contents`] is NUL-separated; the quoting of one
# in a diagnostic is `quotearg`'s shell-escape rules, which is what
# `coreutils::quote` reproduces.

TREE=$'mktree; printf x > $\'we\\nird\''
run_case $'we\nird' dir
TREE=$'mktree; printf x > $\'we\\nird\''
run_case $'we\nird' nosuchdir/x
TREE='mktree; printf x > "sp ace"'
run_case "sp ace" dir
TREE="mktree; printf x > \"qu'ote\""
run_case "qu'ote" dir
TREE='mktree; printf x > "back\\slash"'
run_case 'back\slash' dir
TREE=$'mktree; printf x > $\'\\xff\\xfe\''
run_case $'\xff\xfe' dir

# =============================================================================
# 11. Hard links survive a move
# =============================================================================
# The whole of [`hardlinks`]'s reason for existing. A rename does not touch the
# inode, so a group stays a group; a copy-and-unlink that got the bytes right
# would show two separate files here and nowhere else.

TREE='mktree; printf hl > a; ln a b'
run_case a dir
TREE='mktree; printf hl > a; ln a b'
run_case a b
TREE='mktree; printf hl > a; ln a b'
run_case a b dir
TREE='mktree; mkdir -p g; printf hl > g/a; ln g/a g/b'
run_case g moved

# =============================================================================
# 12. Times, modes and ownership survive a move
# =============================================================================
# `STAMPS` on. For a same-filesystem move every attribute survives by
# construction, so these are constants rather than coin-flips — and a case where
# one stops being constant has found a `mv` that copied when it should have
# renamed.

STAMPS=1; TREE='mkstamped'
run_case file.txt moved
STAMPS=1; TREE='mkstamped'
run_case file.txt dir
STAMPS=1; TREE='mkstamped'
run_case tree moved
STAMPS=1; TREE='mkstamped'
run_case tree/link moved
STAMPS=1; TREE='mkstamped; chmod 0741 file.txt'
run_case file.txt dir
STAMPS=1; TREE='mkstamped; chmod 2750 tree'
run_case tree moved

# =============================================================================
# 13. -f suppresses prompts and nothing else
# =============================================================================
# What `-f` does is §15's subject; what it does *not* do is this section's. It
# has never suppressed an error in GNU `mv` — the previous implementation had it
# swallow the diagnostic and keep the failure, so a failed move printed nothing
# and exited 1. See `mv.rs` module docs, bug 2. These cases are the ones where
# there is no prompt to suppress, so `-f` must be invisible.

run_case -f file.txt dst
TREE='mktree; printf old > dst'
run_case -f file.txt dst
run_case -f nosuchfile dst
run_case --force nosuchfile dst
TREE='mktree; mkdir ro; chmod 0555 ro'
run_case -f file.txt ro
run_case -ff file.txt dst
run_case -f -f file.txt dst

# =============================================================================
# 14. -v, --verbose
# =============================================================================
# The line goes to *stdout*, not stderr, so these cases are the only ones in the
# file where the two streams could be swapped without any of the others
# noticing. `renamed 'src' -> 'dst'`, both names quoted in one style, one line
# per source, and nothing at all when the move fails.
#
# The `copied` / `removed` pair that a cross-device move prints is not here, for
# the reason given at the head of this file: no case moves across a filesystem
# boundary. Those two sentences are pinned by unit test in `mv.rs` instead.

run_case -v file.txt dst
run_case -v file.txt dir
run_case --verbose tree moved
# Two sources: two lines, in operand order, each naming where it landed rather
# than the directory that was asked for.
run_case -v file.txt tree/a.txt dir
# A failure prints its diagnostic and no verbose line — `emit_verbose` sits
# inside the `rename_errno == 0` arm (`copy.c:2761`).
run_case -v nosuchfile dst
# An overwrite goes through the second rename, which has its own emission site.
TREE='mktree; printf old > dst'
run_case -v file.txt dst
# Clustered, and after the operands, since options permute.
run_case -fv file.txt dst
run_case file.txt dst -v
# A name that needs quoting, which is what the one-style rule is for: the reader
# has to be able to tell the space *in* the name from the space between names.
TREE='mktree; printf x > "a b.txt"'
run_case -v "a b.txt" dst

# =============================================================================
# 15. -i, -f and -n
# =============================================================================
# The three are one field, not three flags, so the last one on the line is the
# one in effect and the interesting cases are the pairs. They also sit *above*
# the directory sentences and above the same-file check, which is not where
# reading the diagnostics would put them — `mv -n tree dst` says `not replacing
# 'dst'` rather than `cannot overwrite non-directory`, and `mv -n f f` says the
# same rather than `'f' and 'f' are the same file`.
#
# One arm of the decision cannot be reached from here at all: with no option
# given, GNU asks about an unwritable destination only when stdin is a terminal,
# and every case in this file runs with stdin redirected from a file. What is
# reachable is the wording that arm shares with `-i`, which the `chmod 0444`
# cases below pin — `mv` says `replace 'dst', overriding mode …?` where `cp`
# says `unwritable 'dst' …; try anyway?`. The terminal arm itself is pinned by
# unit test in `mv.rs`.

# -n: refuse an existing destination, say so, and exit 1. The exit status is the
# half worth pinning — Ubuntu's patched `mv` exits 0 here; see
# `design-decisions.md` §726.
TREE='mktree; printf old > dst'
run_case -n file.txt dst
TREE='mktree; printf old > dst'
run_case --no-clobber file.txt dst
# A free name is not an existing destination: an ordinary, silent move.
run_case -n file.txt new.txt
run_case -n file.txt dir
# Into a directory, where the name that is refused is the computed target and
# not the operand.
TREE='mktree; printf old > dir/file.txt'
run_case -n file.txt dir
# A directory source onto a plain file: `-n` beats the directory sentence.
TREE='mktree; printf old > dst'
run_case -n tree dst
# And it beats the same-file check, by being the reason that check is skipped.
run_case -n file.txt file.txt
run_case -n nosuchfile dst

# -i: ask, on stderr, and obey. `ANSWERS` is what stdin holds; empty is end of
# input straight away, which gnulib reads as a no.
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case -i file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -i file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=''
run_case -i file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'yes\n'
run_case --interactive file.txt dst
# No destination, no question.
ANSWERS=$'n\n'
run_case -i file.txt new.txt
# A directory source is asked about, unlike `cp`'s, whose block is guarded by
# `! S_ISDIR (src_mode)` because `cp -r` descends and asks about the files
# inside instead.
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -i tree dst
# The same-file check runs first when `-i` is what is given, so this is a
# refusal and not a prompt — the mirror of the `-n` case above.
ANSWERS=$'y\n'
run_case -i file.txt file.txt
# One question per source, answered in order: the first moves, the second does
# not, and the command still fails.
TREE='mktree; printf a > dir/file.txt; printf b > dir/a.txt'; ANSWERS=$'y\nn\n'
run_case -i file.txt tree/a.txt dir

# The unwritable-destination wording, which is `mv`'s and not `cp`'s because
# upstream's `clears_destination` is `x->move_mode || …`.
TREE='mktree; printf old > dst; chmod 0444 dst'; ANSWERS=$'n\n'
run_case -i file.txt dst
TREE='mktree; printf old > dst; chmod 0444 dst'; ANSWERS=$'y\n'
run_case -i file.txt dst
# Without an option and without a terminal the same file is moved in silence.
# The prompt is a courtesy to a human, not a permission check.
TREE='mktree; printf old > dst; chmod 0444 dst'
run_case file.txt dst

# -f: never ask, in a case where `-i` would have.
TREE='mktree; printf old > dst; chmod 0444 dst'; ANSWERS=$'n\n'
run_case -f file.txt dst

# Last of the three wins, in both orders of each pair, clustered, spelled long,
# and after the operands.
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -i -f file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case -f -i file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -n -f file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case -f -n file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case -i -n file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -n -i file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -if file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case -fi file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -nfi file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case --force --no-clobber file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case file.txt dst -n
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case -n file.txt dst -i
# `-v` and `-i` in one command, which is the only shape that writes to both
# streams: the prompt on stderr, the `renamed` line on stdout.
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case -vi file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -vi file.txt dst

# =============================================================================
# 16. -t and -T, the two ways to say what the destination is
# =============================================================================
# Without either of them the *last operand* decides its own role, by being a
# directory or not. That is convenient and it is ambiguous, and both options
# exist to remove the ambiguity from opposite ends: `-t DIR` names the directory
# up front so every operand is a source, and `-T` says the destination is a
# name, never a directory to move into.
#
# The ambiguity is not academic. `mv "$f" "$d"` in a script does one thing when
# `$d` exists as a directory and a different thing when it does not, so a script
# that means "put this file in that directory" silently renames the file the day
# the directory is missing. `-t` is what `xargs mv -t dir` is for; `-T` is what
# makes `mv -T newdir olddir` replace a directory rather than nest it inside
# itself.
#
# The order of the checks below is upstream's (`mv.c:427-500`) and every step of
# it is observable, which is why the contradictory and the malformed lines are
# here rather than left to a unit test:
#
#   1. the operand count, which `-t` changes: one operand is a whole command
#   2. `-T` with `-t`, refused before `-t`'s directory is so much as stat'd
#   3. `-T` with a third operand, which has nowhere to go
#   4. `-t`'s directory, whose failure says `target directory` where the
#      trailing operand's says a bare `target`
#
# What `-T` switches off has no diagnostic of its own: the question "is the last
# operand a directory?", which every other two-operand case asks and answers
# before deciding anything. That is why `mv -T file dir` and `mv file dir` are
# paired below — the same two operands, one refusal and one move, and no option
# in sight that says which.

run_case -t dir file.txt
run_case --target-directory=dir file.txt tree/a.txt
# The clustered value, which only works through a table that says `t` takes one.
run_case -tdir file.txt
# After the operands, since options permute.
run_case file.txt -t dir
# A single source still goes *inside*, which is the shape that has no spelling
# without `-t`: `mv file.txt dir` would too, but only because `dir` happens to
# be a directory today.
run_case -t dir tree
run_case -v -t dir file.txt tree/a.txt
# A trailing slash on the directory, and a symlink to it: the operand is opened
# the way gnulib opens it, which follows.
run_case -t dir/ file.txt
TREE='mktree; ln -s dir dlink'
run_case -t dlink file.txt
# An overwrite inside the directory is an ordinary overwrite.
TREE='mktree; printf old > dir/file.txt'
run_case -t dir file.txt
# Two sources with one basename, which is the collision `dest_info` is for. It
# needs `-t` no more than it needs a trailing directory, but the operand list is
# a different shape here and the check must still fire.
TREE='mktree; mkdir two; printf x > two/a.txt'
run_case -t dir tree/a.txt two/a.txt

# `-t` and the operand count. One operand is enough; zero is not, and the
# diagnostic is the one for *no operands at all*, not for a missing destination.
run_case -t dir
run_case -t dir ''
# The directory itself, checked once and named as a *target directory* — a
# different sentence from the trailing operand's bare `target`.
run_case -t nosuchdir file.txt
run_case -t file.txt tree/a.txt
run_case -t '' file.txt
# A second `-t` is refused without the two being compared, so naming the same
# directory twice fails exactly as two different ones do.
run_case -t dir -t tree file.txt
run_case -t dir -t dir file.txt
run_case -t dir --target-directory=tree file.txt

# `-T`: the destination is a name. Against a directory that is a refusal, and
# without `-T` the very same line moves the file inside.
run_case -T file.txt dir
run_case file.txt dir
run_case -T file.txt new.txt
run_case --no-target-directory tree moved
# Directory onto directory, which is the case `-T` exists for: with an empty
# destination the rename succeeds and replaces it; with a full one it fails
# rather than nesting.
run_case -T tree dir
TREE='mktree; printf x > dir/keep'
run_case -T tree dir
# An existing plain destination is overwritten, with no question asked about
# whether the name was free.
TREE='mktree; printf old > dst'
run_case -T file.txt dst
# Repeating it is not an error: unlike `-t` there is no value to disagree with.
run_case -T -T file.txt new.txt
run_case file.txt new.txt -T

# `-T` and the operand count, which it does not change: two, no more and no
# fewer.
run_case -T
run_case -T file.txt
run_case -T file.txt tree/a.txt dir
# Both options at once is a diagnostic of its own, raised before `-t`'s
# directory is looked at — so a *missing* directory is not what gets reported.
run_case -T -t dir file.txt
run_case -t dir -T file.txt
run_case -T -t nosuchdir file.txt

# =============================================================================
# 17. --help and --version
# =============================================================================
# The family's two deliberate differences: `--help` omits the GNU project's
# `Report bugs to:` block, and `--version` names SlateOS.

xfail_case "our --help omits GNU's 'Report bugs to' block" --help
xfail_case "our --version names SlateOS" --version
# `--help` wins over an operand, and over an error later on the line.
xfail_case "our --help omits GNU's 'Report bugs to' block" --help file.txt dst
xfail_case "our --help omits GNU's 'Report bugs to' block" --help --nosuchoption

# =============================================================================
# 18. Options GNU has and this mv has not
# =============================================================================
# An inventory, not a permission. Each entry names its option and turns into an
# XPASS the moment the option lands, which is what forces it to be promoted to a
# real case above. Every one of these is refused rather than ignored: silently
# ignoring an option that decides whether a file is destroyed is how data is
# lost. `-v` was promoted this way and became §14; `-i`/`-f`/`-n` became §15;
# `-t`/`-T` became §16, where the eleven entries that were here turned into
# thirty-three.

# -u, --update: move only when the source is newer. The fixture pins both times
# so `older` has a fixed answer.
TREE='mkstamped; touch -d "2009-09-09" dst'; STAMPS=1
missing -u file.txt dst
TREE='mkstamped; touch -d "1999-09-09" dst'; STAMPS=1
missing -u file.txt dst
TREE='mkstamped; touch -d "2009-09-09" dst'; STAMPS=1
missing --update=older file.txt dst
TREE='mkstamped; touch -d "2009-09-09" dst'; STAMPS=1
missing --update=all file.txt dst
TREE='mkstamped; touch -d "1999-09-09" dst'; STAMPS=1
missing --update=none file.txt dst
missing --update=nosuchword file.txt dst

# -b, --backup, -S, and the two environment variables they override.
TREE='mktree; printf old > dst'
missing -b file.txt dst
TREE='mktree; printf old > dst'
missing --backup file.txt dst
TREE='mktree; printf old > dst'
missing --backup=simple file.txt dst
TREE='mktree; printf old > dst'
missing --backup=numbered file.txt dst
TREE='mktree; printf old > dst; printf b1 > dst.~1~'
missing --backup=existing file.txt dst
TREE='mktree; printf old > dst'
missing --backup=none file.txt dst
TREE='mktree; printf old > dst'
missing --backup=nosuchword file.txt dst
TREE='mktree; printf old > dst'
missing -S .bak file.txt dst
TREE='mktree; printf old > dst'
missing --suffix=.bak -b file.txt dst
TREE='mktree; printf old > dst'; ENVV=(VERSION_CONTROL=numbered)
missing -b file.txt dst
TREE='mktree; printf old > dst'; ENVV=(VERSION_CONTROL=numbered)
missing --backup=simple file.txt dst
TREE='mktree; printf old > dst'; ENVV=(SIMPLE_BACKUP_SUFFIX=.bk)
missing -b file.txt dst
TREE='mktree; printf old > dst'; ENVV=(SIMPLE_BACKUP_SUFFIX=.bk)
missing -b -S .cli file.txt dst
# `--backup` and `--no-clobber` are mutually exclusive, and the check is on
# whether the option was *given*, not on what it resolved to — so
# `--backup=none -n` is refused too.
TREE='mktree; printf old > dst'
missing -b -n file.txt dst
TREE='mktree; printf old > dst'
missing --backup=none -n file.txt dst

# --strip-trailing-slashes, which changes what a source ending in `/` names.
missing --strip-trailing-slashes tree/ dir
TREE='mktree; ln -s tree treelink'
missing --strip-trailing-slashes treelink/ moved

# --debug, which implies -v and explains the method.
missing --debug file.txt dst
missing --debug tree moved

# --no-copy, which turns the EXDEV fallback off. On one filesystem it changes
# nothing, so this case measures only that the option is accepted.
missing --no-copy file.txt dst

# -Z, --context. Without SELinux on the build host GNU takes the
# `selinux_enabled` false branch and does nothing at all, so this too measures
# only that the option is accepted.
missing -Z file.txt dst
missing --context file.txt dst

# =============================================================================
echo
printf '%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER DIFFER (XPASS)' "$xpass"
echo
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
