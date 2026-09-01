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
# ## The second filesystem, which this harness used to say it could not have
#
# The `EXDEV` fallback — copy, then unlink — is the part of `mv` that has to
# reproduce by hand everything a rename gets for free: the mode, the times, the
# ownership, the symlink's text rather than its target's bytes, and a
# directory's whole subtree. It is also the part with the most room to be
# quietly wrong, and three of the four bugs in `mv.rs`'s module docs lived
# there.
#
# This file used to say that reaching it needed `mount`, which needed a password
# no unattended harness may ask for, and left the fallback to `mv.rs`'s unit
# tests. **That was wrong, and it cost real coverage.** Linux already has a
# second filesystem mounted and world-writable: `/dev/shm` is a tmpfs wherever
# POSIX shared memory exists, `$XDG_RUNTIME_DIR` is a per-user tmpfs wherever
# systemd-logind does, and either is a different `st_dev` from the ext4 that
# `mktemp -d` lands on. §22 uses one, and the first run of it found two bugs
# that the reasoning above had been quietly protecting for as long as the file
# said what it said. The lesson is `known-issues.md`'s recurring one: *an
# argument for why something cannot be tested is not evidence about the thing.*
#
# See [`FAR`] for how a case names the other filesystem, and [`far_root`] for
# how one is found — including why the device is *checked* rather than assumed.
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

# --- the other filesystem -----------------------------------------------------
# Where §22's cases put the half of their fixture that has to be on a different
# device. Empty means none was found, and §22 is skipped rather than run against
# one device and reported as if it had crossed a boundary.
#
# `$XDG_RUNTIME_DIR` is tried first because it is private to this user and is
# removed at logout; `/dev/shm` is shared, so the directory below it is made with
# `mktemp -d` and removed on the way out. `/run/user/$(id -u)` is the same place
# as the first under a shell that did not inherit the variable.
#
# **The device is compared, not assumed.** A container can bind-mount `/dev/shm`
# off the root filesystem, and a `/tmp` that is itself a tmpfs may be the very
# same one. A §22 that ran on a single device would exercise `rename(2)` while
# reporting that it had exercised the copy fallback — a harness lying in the one
# direction that matters, which is worse than a harness that skips.
far_root=
for far_candidate in "${XDG_RUNTIME_DIR:-}" /dev/shm "/run/user/$(id -u)"; do
  # A trailing slash is legal in the variable and survives every path built from
  # it, which puts a `//` in the middle of the names [`scrub`] matches. They
  # still match, because both sides are built the same way — but the reported
  # path is what a reader retypes, so it is normalised here rather than trusted.
  far_candidate=${far_candidate%/}
  [ -n "$far_candidate" ] && [ -d "$far_candidate" ] && [ -w "$far_candidate" ] || continue
  [ "$(stat -c %d "$far_candidate")" != "$(stat -c %d "$work")" ] || continue
  far_root=$(mktemp -d "$far_candidate/mv-diff.XXXXXX") && break
  far_root=
done

# `diff-wsl.sh` asks callers to extend its cleanup rather than set a second
# `EXIT` trap, which would replace the first and leak `$DIFF_TMP`.
diff_cleanup() {
  chmod -R u+rwx "$DIFF_TMP" 2>/dev/null
  rm -rf "$DIFF_TMP"
  if [ -n "$far_root" ]; then
    chmod -R u+rwx "$far_root" 2>/dev/null
    rm -rf "$far_root"
  fi
  return 0
}

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
# A case with a [`FAR`] fixture has *two* trees to account for, and both matter:
# what arrived on the near side and what is left on the far one. They are listed
# one after the other, the far one's paths prefixed `far/`, rather than merged
# and re-sorted — two sorted blocks are as deterministic as one and a reader can
# see at a glance which side a line is about.
snapshot() {
  snapshot_in '' "$1"
  [ -n "${2:-}" ] && snapshot_in 'far/' "$2"
  return 0
}

snapshot_in() {
  local t=''
  [ -n "$STAMPS" ] && t=' %T@'
  ( cd "$2" 2>/dev/null && find . -mindepth 1 \
        \( -type d -printf "$1%P %m d$t\n" \
        -o -type l -printf "$1%P l -> %l$t\n" \
        -o -printf "$1%P %m %s$t\n" \) 2>/dev/null \
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
#
# The two trees are listed separately rather than together, and here that is
# forced rather than chosen: a hard link cannot span filesystems, so a group can
# never have members on both sides, and the inode numbers used to group them are
# only unique *within* one device.
hardlinks() {
  hardlinks_in '' "$1"
  [ -n "${2:-}" ] && hardlinks_in 'far/' "$2"
  return 0
}

hardlinks_in() {
  ( cd "$2" 2>/dev/null || return 0
    find . -mindepth 1 ! -type d -links +1 -printf "%i\t$1%P\n" 2>/dev/null \
      | LC_ALL=C sort -t"$(printf '\t')" -k2 \
      | awk -F'\t' '{ g[$1] = ($1 in g) ? g[$1] " " $2 : $2 }
                    END { for (k in g) print g[k] }' \
      | LC_ALL=C sort )
}

# And the extended attributes — see `diff_xattrs_in` in `diff-wsl.sh` for what
# is compared, what is elided and why it is read with Python rather than
# `getfattr`. Both trees, the far one prefixed, exactly as the snapshot does it.
#
# `mv` carries attributes across a filesystem boundary by reading and rewriting
# them, and across a rename by not touching the inode at all — so this column is
# nearly always empty and is nearly always right, and the one shape where it can
# be wrong is §22's.
xattrs() {
  diff_xattrs_in '' "$1"
  [ -n "${2:-}" ] && diff_xattrs_in 'far/' "$2"
  return 0
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
  contents_in '' "$1"
  [ -n "${2:-}" ] && contents_in 'far/' "$2"
  return 0
}

contents_in() {
  ( cd "$2" 2>/dev/null || return 0
    find . -type f -printf '%P\0' 2>/dev/null | LC_ALL=C sort -z \
      | while IFS= read -r -d '' f; do
      printf '== %s%s\n' "$1" "$f"
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
# Shell run inside the case's directory *on the other filesystem* to build the
# half of the fixture that has to live there. Empty — every case but §22's —
# means no such directory is made and the case is an ordinary one.
#
# A case that sets it writes `@FAR@` wherever it wants that directory's path in
# an argument, and [`run_one`] substitutes the side's own. The placeholder is
# necessary rather than tidy: the two sides run in two different directories on
# both filesystems, so the path cannot be a constant, and a case that named
# `$far_root` directly would name whichever side happened to be set up last.
FAR=
# The umask `mv` runs under, when a case needs one other than the harness's.
# Applied to the move and not to either half of the fixture, which [`run_one`]
# builds in subshells of its own — so a case can give a source a mode the umask
# would not have allowed and still see what the move does with it.
#
# Empty means "whatever this shell has", which is every case but the read-only
# pair in §23. Those set `0222`, a umask that strips the owner-write bit — the
# bit a cross-device move grants its new destination *on purpose*, so that a
# read-only file's extended attributes can still be written onto it. Nothing
# else in the suite can tell a `mv` that repairs that stripping from one that
# merely asked for the bit and did not check.
UMASK=
reset_knobs() { TREE='mktree'; ANSWERS=''; STAMPS=''; ENVV=(); FAR=''; UMASK=''; }
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
scrub() {
  local exprs=(-e "s|$1|<DIR>|g")
  # And the same for the other filesystem's directory, which a §22 case names on
  # the command line and both programs then echo back.
  [ -n "${2:-}" ] && exprs+=(-e "s|$2|<FAR>|g")
  sed "${exprs[@]}"
}

# --- running one side ---------------------------------------------------------

run_one() {
  local side=$1 dir=$2 out=$3 err=$4 rcf=$5 far=$6; shift 6
  mkdir -p "$dir"
  ( cd "$dir" && eval "$TREE" ) >/dev/null 2>&1
  # The other filesystem's half of the fixture, and the substitution that lets a
  # case refer to it. See [`FAR`] for why the placeholder exists.
  local args=() a
  if [ -n "$far" ]; then
    mkdir -p "$far"
    ( cd "$far" && eval "$FAR" ) >/dev/null 2>&1
  fi
  for a in "$@"; do args+=("${a//@FAR@/$far}"); done
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
    # After the `cd` and before the run, so it reaches `mv` and nothing else —
    # neither half of the fixture above, nor the snapshot afterwards.
    [ -z "$UMASK" ] || umask "$UMASK"
    # `env` and not an assignment prefix, so that [`ENVV`] can hold a variable
    # whose *name* is chosen by the case rather than by this line.
    diff_run timeout -k 2 30 env "${ENVV[@]}" mv "${args[@]}" >"$out" 2>"$err"
  ) <"$answers"
  echo $? >"$rcf"
  return 0
}

# --- comparing the two sides --------------------------------------------------

judge() {
  local o_dir=$1 g_dir=$2 o_out=$3 g_out=$4 o_extra=$5 g_extra=$6 label=$7
  local o_far=$8 g_far=$9
  local o_snap g_snap o_body g_body o_show g_show o_link g_link o_xat g_xat
  o_snap=$(snapshot "$o_dir" "$o_far"); g_snap=$(snapshot "$g_dir" "$g_far")
  o_link=$(hardlinks "$o_dir" "$o_far"); g_link=$(hardlinks "$g_dir" "$g_far")
  o_xat=$(xattrs "$o_dir" "$o_far"); g_xat=$(xattrs "$g_dir" "$g_far")
  o_body=$(contents "$o_dir" "$o_far" | scrub "$o_dir" "$o_far")
  g_body=$(contents "$g_dir" "$g_far" | scrub "$g_dir" "$g_far")
  o_show=$(scrub "$o_dir" "$o_far" <"$o_out"); g_show=$(scrub "$g_dir" "$g_far" <"$g_out")
  o_extra=$(printf '%s' "$o_extra" | scrub "$o_dir" "$o_far")
  g_extra=$(printf '%s' "$g_extra" | scrub "$g_dir" "$g_far")

  if [ "$o_show" = "$g_show" ] && [ "$o_extra" = "$g_extra" ] \
     && [ "$o_snap" = "$g_snap" ] && [ "$o_body" = "$g_body" ] \
     && [ "$o_link" = "$g_link" ] && [ "$o_xat" = "$g_xat" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours: %s\n        out{%s}\n        tree{%s} files{%s} links{%s} xattr{%s}\n  gnu : %s\n        out{%s}\n        tree{%s} files{%s} links{%s} xattr{%s}' \
    "$(printf '%s' "$o_extra" | tr '\n' '|')" "$(printf '%s' "$o_show" | tr '\n' '|')" \
    "$(printf '%s' "$o_snap" | tr '\n' '|')" "$(printf '%s' "$o_body" | tr '\n' '|')" \
    "$(printf '%s' "$o_link" | tr '\n' '|')" "$(printf '%s' "$o_xat" | tr '\n' '|')" \
    "$(printf '%s' "$g_extra" | tr '\n' '|')" "$(printf '%s' "$g_show" | tr '\n' '|')" \
    "$(printf '%s' "$g_snap" | tr '\n' '|')" "$(printf '%s' "$g_body" | tr '\n' '|')" \
    "$(printf '%s' "$g_link" | tr '\n' '|')" "$(printf '%s' "$g_xat" | tr '\n' '|')")
  LABEL=$label
}

compare() {
  case_no=$((case_no+1))
  local o_dir=$work/o$case_no g_dir=$work/g$case_no
  local o_out=$work/oo$case_no g_out=$work/go$case_no
  local o_err=$work/oe$case_no g_err=$work/ge$case_no
  local o_rc=$work/or$case_no g_rc=$work/gr$case_no
  local o_far='' g_far=''
  [ -z "$FAR" ] || { o_far=$far_root/o$case_no; g_far=$far_root/g$case_no; }
  local label="mv $*"
  [ -z "$ANSWERS" ] || label="$label   [in: ${ANSWERS//$'\n'/\\n}]"
  [ "$TREE" = mktree ] || label="$label   [tree: $TREE]"
  [ -z "$FAR" ] || label="$label   [far: $FAR]"
  [ ${#ENVV[@]} -eq 0 ] || label="$label   [env: ${ENVV[*]}]"
  [ -z "$UMASK" ] || label="$label   [umask: $UMASK]"
  run_one ours "$o_dir" "$o_out" "$o_err" "$o_rc" "$o_far" "$@"
  run_one gnu  "$g_dir" "$g_out" "$g_err" "$g_rc" "$g_far" "$@"
  judge "$o_dir" "$g_dir" "$o_out" "$g_out" \
    "rc=$(cat "$o_rc") err{$(cat "$o_err")}" \
    "rc=$(cat "$g_rc") err{$(cat "$g_err")}" \
    "$label" "$o_far" "$g_far"
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
if [ -n "$far_root" ]; then
  echo "  far:  $far_root"
else
  echo "  far:  none found on a second device -- section 22 skipped"
fi
# Said out loud in both directions, because a harness that quietly stops
# checking something is worse than one that never checked it: the run would
# otherwise report the same counts while comparing one fewer thing.
#
# Two independent facts, and both are needed. The harness must be able to *set*
# an attribute (Python, and a filesystem that accepts one), and the reference
# must have been built with `USE_XATTR` — a coreutils compiled without libattr
# has a `copy_attr` whose whole body is `return true`, so it drops every
# attribute silently and a case comparing against it would report the loss as
# *ours*.
if [ -z "$DIFF_XATTR" ]; then
  echo "  xattr: NOT COMPARED -- no working setxattr found, section 23 skipped"
elif [ "$DIFF_XATTR_REF" != yes ]; then
  echo "  xattr: NOT COMPARED -- reference built without USE_XATTR, section 23 skipped"
else
  echo "  xattr: $DIFF_XATTR, reference has USE_XATTR"
fi
# And the same pair of facts for section 24. The comparison itself is shared —
# an ACL is stored as an extended attribute and `xattrs` already reads it — so
# what these two lines report is only whether that section's cases can run.
if [ -z "$DIFF_SETFACL" ]; then
  echo "  acl:   NOT EXERCISED -- no working setfacl, section 24 skipped"
elif [ "$DIFF_ACL_REF" != yes ]; then
  echo "  acl:   NOT EXERCISED -- reference built without USE_ACL, section 24 skipped"
else
  echo "  acl:   $DIFF_SETFACL, reference has USE_ACL"
fi

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
# The example GNU spells out in `same_file_ok`'s own comment (`copy.c:1909`):
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

# A destination that cannot be stat'd at all, which is a different diagnostic
# from a destination that can be stat'd and refused. `mv` looks the destination
# up *after* the rename has already failed, and reports what that lookup said —
# naming the destination alone, because nothing was moved and the source is not
# implicated. `cannot move A to B` here would be a false claim that a rename was
# attempted, carrying an errno left over from the speculative one.
#
# The three ways to fail it: a trailing slash on a name that is not a directory,
# a path *through* a regular file, and a symlink loop in the path. The loop is
# the one upstream nearly excuses — `copy.c:2326` lets `ELOOP` past when
# `unlink_dest_after_failed_open` is set, and `mv` sets it false (`mv.c:128`),
# so it does not.
run_case file.txt tree/a.txt/
run_case file.txt file.txt/x
TREE='mktree; ln -s loop loop'
run_case file.txt loop/x
# And the loop as the destination *itself* rather than a component of it, which
# does not fail: `lstat` does not follow, so the dangling symlink is an ordinary
# thing in the way and is replaced.
TREE='mktree; ln -s loop loop'
run_case file.txt loop

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
# 17. -u and --update
# =============================================================================
# `STAMPS` on throughout, because the whole option is a comparison of two
# modification times and a harness that did not print them would agree with a
# `mv` that compared the sizes instead.
#
# The fixture stamps `file.txt` at 2004 (`mkstamped`), so a `dst` touched to
# 1999 is *older* than the source and one touched to 2009 is newer, with no
# clock and no ordering left to chance. `dst` is given bytes as well as a time
# so that "the move happened" is visible in the contents column too — an empty
# `dst` and a `dst` that was replaced by an empty file look alike.
#
# Three things make this section bigger than the option looks. First,
# `--update`'s three words write *two* fields — `all` and `none` both turn the
# newer-only comparison off, and `none` turns `interactive` to a fifth value
# that skips *and succeeds* — so the observable outcomes are four, not two.
# Second, those fields are the same ones `-i`/`-f`/`-n` write, so order on the
# command line decides, and the pairs are the cases. Third, upstream guards the
# `-n`-wins rule on the **long form only** (`mv.c:378`) and puts the argument
# lookup *inside* that guard (`mv.c:381`), which makes `-n --update=older` and
# `-n -u` differ, and makes `-n --update=nosuchword` a silently accepted line.
# None of those three is guessable; each is pinned below.

# The comparison itself. Newer destination: nothing moves, and it is a success —
# the difference from `-n`, which prints and exits 1.
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case -u file.txt dst
TREE='mkstamped; printf old > dst; touch -d "1999-09-09" dst'; STAMPS=1
run_case -u file.txt dst
# `--update` bare and `--update=older` are the same thing spelled at length.
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case --update file.txt dst
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case --update=older file.txt dst
TREE='mkstamped; printf old > dst; touch -d "1999-09-09" dst'; STAMPS=1
run_case --update=older file.txt dst
# Equal times are *not* older, so an unchanged file is left alone. This is the
# case a whole-second truncation would still pass and a reversed comparison
# would not, and the fixture's nanoseconds are what make it exact.
TREE='mkstamped; printf old > dst; touch -r file.txt dst'; STAMPS=1
run_case -u file.txt dst
# No destination at all: an ordinary move, since there is nothing to compare.
STAMPS=1
run_case -u file.txt new.txt
STAMPS=1
run_case -u file.txt dir
# A directory source is exempt from the comparison (`copy.c:2353`'s
# `! S_ISDIR (src_mode)`), so this is the ordinary refusal and not a skip.
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case -u tree dst
run_case -u nosuchfile dst

# `--update=all` is "replace whatever is there", which is what no option at all
# already means — it exists to *cancel* an earlier `-u` or `-i`, so both orders
# are cases.
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case --update=all file.txt dst
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case -u --update=all file.txt dst
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case --update=all -u file.txt dst
# It writes `interactive` too, so it cancels a `-i` before it and not after.
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -i --update=all file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case --update=all -i file.txt dst

# `--update=none` is the skip that succeeds: same effect on the tree as `-n`,
# opposite exit status and no diagnostic. The pair is the point.
TREE='mktree; printf old > dst'
run_case --update=none file.txt dst
TREE='mktree; printf old > dst'
run_case -n file.txt dst
# It is checked before the directory sentence and before the same-file check,
# exactly where `-n` is — so it swallows both of those diagnostics rather than
# printing them, which is the only way to see that it took the same branch.
TREE='mktree; printf old > dst'
run_case --update=none tree dst
run_case --update=none file.txt file.txt
# And it beats a `-u` that came earlier, being the later of the two.
TREE='mkstamped; printf old > dst; touch -d "1999-09-09" dst'; STAMPS=1
run_case -u --update=none file.txt dst
TREE='mkstamped; printf old > dst; touch -d "1999-09-09" dst'; STAMPS=1
run_case --update=none -u file.txt dst

# gnulib's `argmatch` is a prefix match, so a unique prefix of any of the three
# words resolves — the same rule that makes `--up` resolve to `--update`,
# applied one level down to its argument. An unknown word is refused with the
# list; an empty one is *ambiguous* rather than unknown, since it is a prefix of
# all three.
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case --update=o file.txt dst
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case --update=n file.txt dst
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1
run_case --update=a file.txt dst
run_case --update=nosuchword file.txt dst
run_case --update= file.txt dst
# The option name in the diagnostic is the long spelling even when the short one
# was typed — but `-u` takes no argument, so the only way to reach it is
# `--update=`.
run_case --up=nosuchword file.txt dst

# The `-n` precedence rule, and the two places it is narrower than it reads.
# `mv.c:378` guards the whole argument branch on `interactive != I_ALWAYS_NO`,
# and `mv.c:509` clears `update` after the loop — so a `-n` on either side of
# the option wins, but by different mechanisms.
TREE='mkstamped; printf old > dst; touch -d "1999-09-09" dst'; STAMPS=1
run_case -n --update=all file.txt dst
TREE='mkstamped; printf old > dst; touch -d "1999-09-09" dst'; STAMPS=1
run_case --update=all -n file.txt dst
TREE='mkstamped; printf old > dst; touch -d "1999-09-09" dst'; STAMPS=1
run_case -u -n file.txt dst
TREE='mkstamped; printf old > dst; touch -d "1999-09-09" dst'; STAMPS=1
run_case -n -u file.txt dst
# The asymmetry `--help` denies. `--update[=older]` is documented as what `-u`
# means, but the guard is on the long form alone: a later `-i` re-enables the
# skip that `-n` had disabled, and then `-n -u -i` still compares times while
# `-n --update=older -i` does not.
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1; ANSWERS=$'n\n'
run_case -n --update=older -i file.txt dst
TREE='mkstamped; printf old > dst; touch -d "2009-09-09" dst'; STAMPS=1; ANSWERS=$'n\n'
run_case -n -u -i file.txt dst
# And, from the lookup being inside the guard: with `-n` first the word is never
# read, so a word that is otherwise an error is accepted in silence.
TREE='mktree; printf old > dst'
run_case -n --update=nosuchword file.txt dst
TREE='mktree; printf old > dst'
run_case --update=nosuchword -n file.txt dst

# A hard-link pair. `--update=none` reaches the same `abandon_move` branch `-n`
# does, before the same-file check, so the group is left whole and unmentioned;
# `-u` gets there by the time comparison instead. Either way a skip must not
# unlink the source — upstream sets `*rename_succeeded` on the skip path
# (`copy.c:2373` for `-u`, `copy.c:2414` for `--update=none`) precisely so it
# does not, and this column is where getting that wrong would show as one file
# where there were two.
TREE='mkstamped; printf hl > a; ln a b; touch -d "2009-09-09" a b'; STAMPS=1
run_case -u a b
TREE='mktree; printf hl > a; ln a b'
run_case --update=none a b

# =============================================================================
# 18. -b, --backup, -S, and the two environment variables
# =============================================================================
# Every case here needs something at the destination, because a backup is a
# rename of the file that is about to be destroyed and a move onto a free name
# destroys nothing. So `TREE` carries `printf old > dst` almost throughout, and
# `dst` is given bytes rather than being touched empty: the whole question is
# *which* of two files ended up under which name, and two empty files answer it
# the same way whatever happened.
#
# `-v` is on wherever the backup's *name* is the thing being measured. The name
# is otherwise only visible in the tree column, which says a file called `dst~`
# exists but not that `mv` chose to call it that — and for the numbered forms,
# which number it picked is the entire behaviour.
#
# Four spellings ask for a backup, not two: `-S`/`--suffix` set `make_backups`
# as well as the suffix (`mv.c:405`), so `-S .bak` alone backs up. That is the
# one fact about this option a from-memory implementation gets wrong, and the
# first four cases pin it.
TREE='mktree; printf old > dst'
run_case -v -b file.txt dst
TREE='mktree; printf old > dst'
run_case -v --backup file.txt dst
TREE='mktree; printf old > dst'
run_case -v -S .bak file.txt dst
TREE='mktree; printf old > dst'
run_case -v --suffix=.bak file.txt dst

# The four control words and the four aliases of them, each of which has to be
# spelled out because they are a table lookup and a table can be mistyped.
# `none` and `off` really do nothing at all, `--backup` given or not.
TREE='mktree; printf old > dst'
run_case -v --backup=none file.txt dst
TREE='mktree; printf old > dst'
run_case -v --backup=off file.txt dst
TREE='mktree; printf old > dst'
run_case -v --backup=numbered file.txt dst
TREE='mktree; printf old > dst'
run_case -v --backup=t file.txt dst
TREE='mktree; printf old > dst'
run_case -v --backup=simple file.txt dst
TREE='mktree; printf old > dst'
run_case -v --backup=never file.txt dst
# `existing`/`nil` is the one whose answer depends on the tree rather than on
# the command line, so it is run both ways round: with a numbered backup already
# there it numbers, and without one it falls back to the simple suffix.
TREE='mktree; printf old > dst'
run_case -v --backup=existing file.txt dst
TREE='mktree; printf old > dst; printf b1 > dst.~1~'
run_case -v --backup=existing file.txt dst
TREE='mktree; printf old > dst; printf b1 > dst.~1~'
run_case -v --backup=nil file.txt dst

# The word is matched by unambiguous prefix and so is the option name, because
# both go through gnulib's `argmatch`/`getopt_long`. An unknown word is an
# error that prints the whole table, which is four lines of quoted words this
# `mv` has to reproduce exactly.
TREE='mktree; printf old > dst'
run_case -v --backup=num file.txt dst
TREE='mktree; printf old > dst'
run_case -v --back=numbered file.txt dst
TREE='mktree; printf old > dst'
run_case --backup=nosuchword file.txt dst
# An empty word is not the same as an unknown one: `argmatch` is not reached at
# all, because gnulib's `xget_version` sends an empty string on to
# `$VERSION_CONTROL` (`backup-find.c:87`), which is unset here, and
# `get_version` answers `numbered_existing` for a null one. So this ends up
# making a plain `dst~` rather than the error it looks like.
TREE='mktree; printf old > dst'
run_case -v --backup= file.txt dst
# An empty suffix falls back to `~` rather than backing a file up onto itself.
TREE='mktree; printf old > dst'
run_case -v -S '' -b file.txt dst

# The two environment variables. Each supplies what its option did not, and
# each is *overridden* by that option — so the pairs are the cases, not the
# variables on their own. Neither is consulted at all unless a backup was asked
# for, which is why an invalid `$VERSION_CONTROL` is fatal with `-b` and
# invisible without it.
TREE='mktree; printf old > dst'; ENVV=(VERSION_CONTROL=numbered)
run_case -v -b file.txt dst
TREE='mktree; printf old > dst'; ENVV=(VERSION_CONTROL=numbered)
run_case -v --backup=simple file.txt dst
TREE='mktree; printf old > dst'; ENVV=(VERSION_CONTROL=numbered)
run_case -v file.txt dst
TREE='mktree; printf old > dst'; ENVV=(VERSION_CONTROL=nosuchword)
run_case -b file.txt dst
TREE='mktree; printf old > dst'; ENVV=(VERSION_CONTROL=nosuchword)
run_case -v file.txt dst
TREE='mktree; printf old > dst'; ENVV=(SIMPLE_BACKUP_SUFFIX=.bk)
run_case -v -b file.txt dst
TREE='mktree; printf old > dst'; ENVV=(SIMPLE_BACKUP_SUFFIX=.bk)
run_case -v -b -S .cli file.txt dst

# Two of the family on one line. A later bare `-b` does not erase an earlier
# `--backup=WORD`, because `-b` writes only the flag and the word is a separate
# variable that nothing clears (`mv.c:344`). `--backup=none` after `-b` does
# clear it, since that one *is* a word.
TREE='mktree; printf old > dst'
run_case -v --backup=numbered -b file.txt dst
TREE='mktree; printf old > dst'
run_case -v -b --backup=numbered file.txt dst
TREE='mktree; printf old > dst'
run_case -v -b --backup=none file.txt dst

# `--backup` and `--no-clobber` are refused together, and the check is on
# whether a backup was *asked for*, not on what it resolved to — so
# `--backup=none -n` is refused even though it would have done nothing. `-S`
# reaches the same check. `--update=none` skips like `-n` but is a different
# value of the same field, so it is not caught and the line is legal.
TREE='mktree; printf old > dst'
run_case -b -n file.txt dst
TREE='mktree; printf old > dst'
run_case -n -b file.txt dst
TREE='mktree; printf old > dst'
run_case -S .bak -n file.txt dst
TREE='mktree; printf old > dst'
run_case --backup=none -n file.txt dst
TREE='mktree; printf old > dst'
run_case -v --backup --update=none file.txt dst

# The prompt comes *before* the backup, so answering no leaves the tree exactly
# as it was — no `dst~` either. `-f` skips the prompt and backs up as usual.
TREE='mktree; printf old > dst'; ANSWERS=$'y\n'
run_case -v -b -i file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=$'n\n'
run_case -v -b -i file.txt dst
TREE='mktree; printf old > dst'; ANSWERS=''
run_case -v -b -i file.txt dst
TREE='mktree; printf old > dst'
run_case -v -b -f file.txt dst

# Nothing at the destination is not an error and not a backup: the verbose line
# has no `(backup: …)` clause at all.
run_case -v -b file.txt nonexistent
# A backup name already in use is overwritten without a word, which is the
# behaviour the numbered forms exist to avoid.
TREE='mktree; printf old > dst; printf prev > dst~'
run_case -v -b file.txt dst
TREE='mktree; printf old > dst; printf b1 > dst.~1~; printf b2 > dst.~2~'
run_case -v --backup=numbered file.txt dst

# Moving a file onto the thing that would become its own backup destroys the
# source, so it is refused — except under `--backup=numbered`, where the backup
# gets a fresh name and the source survives. The refusal is on the *name*, so a
# same-named file in another directory is fine.
TREE='mktree; printf old > dst; printf bk > dst~'
run_case -b dst~ dst
TREE='mktree; printf old > dst; printf bk > dst~'
run_case -v --backup=numbered dst~ dst
TREE='mktree; printf old > dst; printf bk > dst.bak'
run_case -b -S .bak dst.bak dst
TREE='mktree; printf old > dst; mkdir other; printf bk > other/dst~'
run_case -v -b other/dst~ dst

# The two refusals `--backup` lifts. A directory onto a non-directory and a
# non-directory onto a directory are both refused outright, because the rename
# would destroy the one that is a directory — unless there is a backup, which
# is exactly what stops it being destroyed. `-T` is needed for the second:
# without it the directory is a place to move *into* rather than a thing to
# overwrite.
TREE='mktree; printf old > dst'
run_case -v -b dir dst
TREE='mktree; printf old > dst'
run_case -v dir dst
run_case -v -T -b file.txt dir
run_case -v -T file.txt dir
# Directory onto directory needs no backup to be allowed when the destination
# is empty, but with one the destination survives under its new name.
run_case -v -T -b tree dir
run_case -v -T --backup=numbered tree dir

# The just-created refusal is lifted only by *numbered* backups, and the reason
# is that a simple backup of `into/f` is `into/f~` every time: the second source
# would back the first source's arrival up over the first source's own backup,
# which is the destruction the refusal exists to prevent. `existing` is not
# numbered even where it would number, because the check reads the type that
# was asked for.
TREE='mktree; mkdir -p from1 from2 into; printf 1 > from1/f; printf 2 > from2/f'
run_case -v from1/f from2/f into
TREE='mktree; mkdir -p from1 from2 into; printf 1 > from1/f; printf 2 > from2/f'
run_case -v -b from1/f from2/f into
TREE='mktree; mkdir -p from1 from2 into; printf 1 > from1/f; printf 2 > from2/f'
run_case -v --backup=existing from1/f from2/f into
TREE='mktree; mkdir -p from1 from2 into; printf 1 > from1/f; printf 2 > from2/f'
run_case -v --backup=numbered from1/f from2/f into

# Several sources into a directory: each gets its own backup, named inside the
# destination directory rather than beside the source.
TREE='mktree; mkdir into; printf x > into/file.txt; printf y > into/a.txt'
run_case -v -b file.txt tree/a.txt into
# And a destination in a subdirectory, where the backup has to land in that
# subdirectory and not in the working one.
TREE='mktree; printf old > tree/sub/dst'
run_case -v -b file.txt tree/sub/dst

# A backup that cannot be made stops the move, and the file that was going to be
# overwritten is still there afterwards. Backing up is the *first* thing that
# writes, so a failure here costs nothing.
TREE='mktree; mkdir ro; printf old > ro/dst; chmod 0555 ro'
run_case -b file.txt ro/dst
# The last operand still has to be a directory when there are three of them,
# whatever `-b` says.
TREE='mktree; printf old > dst'
run_case -b file.txt dst dst2

# =============================================================================
# 19. --help and --version
# =============================================================================
# The family's two deliberate differences: `--help` omits the GNU project's
# `Report bugs to:` block, and `--version` names SlateOS.

xfail_case "our --help omits GNU's 'Report bugs to' block" --help
xfail_case "our --version names SlateOS" --version
# `--help` wins over an operand, and over an error later on the line.
xfail_case "our --help omits GNU's 'Report bugs to' block" --help file.txt dst
xfail_case "our --help omits GNU's 'Report bugs to' block" --help --nosuchoption

# =============================================================================
# 20. Options GNU has and this mv has not
# =============================================================================
# An inventory, not a permission. Each entry names its option and turns into an
# XPASS the moment the option lands, which is what forces it to be promoted to a
# real case above. Every one of these is refused rather than ignored: silently
# ignoring an option that decides whether a file is destroyed is how data is
# lost. `-v` was promoted this way and became §14; `-i`/`-f`/`-n` became §15;
# `-t`/`-T` became §16, where the eleven entries that were here turned into
# thirty-three; `-u`/`--update` became §17, where six turned into thirty-seven —
# nearly all of them about the *order* two options were given in, because
# `--update`'s three words write the same two fields `-i`/`-f`/`-n` do.
# `-b`/`--backup`/`-S` became §18, where fifteen turned into fifty-eight: a backup
# is a second file with a chosen name, so every case has a name to check as well
# as a tree, and the option lifts three of §13's refusals besides.
# `--strip-trailing-slashes` became §21, where two turned into twenty-four — half
# of them pairs, because the option has no behaviour of its own and can only be
# measured as the difference between a command line carrying it and the same one
# without.

# --debug, which implies -v and explains the method.
missing --debug file.txt dst
missing --debug tree moved

# --no-copy, which turns the EXDEV fallback off. On one filesystem it changes
# nothing, so this case measures only that the option is accepted; the one that
# measures what the option *does* is at the end of §22, where there is a
# boundary for it to refuse to cross.
missing --no-copy file.txt dst

# -Z, --context. Without SELinux on the build host GNU takes the
# `selinux_enabled` false branch and does nothing at all, so this too measures
# only that the option is accepted.
missing -Z file.txt dst
missing --context file.txt dst

# =============================================================================
# 21. --strip-trailing-slashes
# =============================================================================
# The option has no policy of its own. It edits the operand list and then every
# other decision is taken exactly as it would have been, so there is nothing to
# measure about it in isolation — which is why half the cases here are pairs,
# the same command line with the option and without, and the case is the
# difference between them.
#
# What the difference *is* comes down to one line of GNU's `main` (`mv.c:505`)
# and where it sits:
#
#   if (remove_trailing_slashes)
#     for (int i = 0; i < n_files; i++)
#       strip_trailing_slashes (file[i]);
#
# It is late — after the missing-operand check, after `-T`'s extra-operand
# check, after `-t`'s directory is opened, after the speculative
# `renameatu (…, RENAME_NOREPLACE)`, and after the probe that asks whether the
# last operand is a directory. And it runs over `n_files`, which that probe has
# already decremented when the destination *was* a directory. So the option
# reaches less of the program than its help text suggests, and the cases below
# are mostly about which side of that line each observable effect falls on.
#
# `-v` is on wherever the case would otherwise be silent, because the names
# `mv` prints are the only place the stripping is directly visible: the tree
# column says a file arrived under some name but not which spelling of the
# source `mv` believed it was moving.

# The option's actual purpose, and the one shape it was added for: a symlink to
# a directory whose trailing slash the shell's tab-completion supplied. The
# slash makes the kernel resolve the link, so the unstripped command asks to
# rename the *directory* through it and gets ENOTDIR; stripped, the symlink
# itself moves and is still a symlink afterwards, which the tree column's `->`
# is what proves.
TREE='mktree; ln -s tree treelink'
run_case -v --strip-trailing-slashes treelink/ dir
TREE='mktree; ln -s tree treelink'
run_case -v treelink/ dir
# `-t` reaches the same place by the other route: its directory came from the
# option and never was one of `file[]`, so every operand is a source and every
# operand is stripped.
TREE='mktree; ln -s tree treelink'
run_case -v --strip-trailing-slashes -t dir treelink/
TREE='mktree; ln -s tree treelink'
run_case -v -t dir treelink/

# And the shape it does *not* rescue, which is the sharpest thing in this
# section. With two operands and a destination that does not exist, GNU has
# already tried `renameatu (file[0], file[1], RENAME_NOREPLACE)` on the
# unstripped words and kept the errno; nothing afterwards retries it. So the
# move still fails ENOTDIR — but the diagnostic quotes the *stripped* name,
# naming a rename that was never attempted. The name in the message and the
# error in it come from two different command lines. Upstream's, and reproduced
# deliberately: see `strip_operands` in `mv.rs`.
TREE='mktree; ln -s tree treelink'
run_case --strip-trailing-slashes treelink/ moved
TREE='mktree; ln -s tree treelink'
run_case treelink/ moved

# gnulib's `strip_trailing_slashes` removes a run, not a slash.
TREE='mktree; ln -s tree treelink'
run_case -v --strip-trailing-slashes treelink/// dir

# A real directory is the boring half and is here to bound the interesting one:
# a trailing slash on a name that really is a directory changes nothing about
# what the kernel does, at any depth.
run_case -v --strip-trailing-slashes tree/ dir
run_case -v --strip-trailing-slashes tree/sub/ dir
# So with a real directory source the option's whole observable effect is the
# word `-v` prints — `dir` where the unstripped run says `dir/`. A case that
# compares only trees would certify these two as the same run.
run_case -v --strip-trailing-slashes dir/ newname
run_case -v dir/ newname

# The loop covers every source, not the first, and the two failures it prevents
# are different failures: the directory link fails at the rename, the file link
# fails at the `stat` before it. Both sides of the pair have to agree about the
# order those two lines are printed in as well as their text.
TREE='mktree; ln -s tree treelink; ln -s file.txt filelink'
run_case -v --strip-trailing-slashes treelink/ filelink/ dir
TREE='mktree; ln -s tree treelink; ln -s file.txt filelink'
run_case -v treelink/ filelink/ dir

# The destination is an operand too when it is not a directory, so a slashed
# name for an existing *file* is stripped and the move succeeds. Unstripped it
# does not even reach the rename: the probe fails, the speculative rename's
# ENOTDIR stands, and `mv` reports it as a failure to `stat`.
run_case -v --strip-trailing-slashes file.txt tree/a.txt/
run_case -v file.txt tree/a.txt/

# The four diagnostics asked before the loop, each quoting the slash the user
# typed. These are the cases a from-memory implementation gets wrong, because
# the natural place to strip operands is on the way in, and there every one of
# these messages loses its slash.
run_case --strip-trailing-slashes file.txt/
run_case --strip-trailing-slashes -T file.txt dst extra/
run_case --strip-trailing-slashes file.txt dir tree/a.txt/
run_case --strip-trailing-slashes -t nodir/ file.txt

# `-T` takes neither the probe nor the speculative rename, so both its operands
# are stripped — and that is observable on the destination rather than the
# source: stripped, `treelink` is a symlink and is replaced; unstripped,
# `treelink/` is a directory and `mv` refuses to overwrite one with a file.
TREE='mktree; ln -s tree treelink'
run_case -v --strip-trailing-slashes -T file.txt treelink/
TREE='mktree; ln -s tree treelink'
run_case -v -T file.txt treelink/
run_case -v --strip-trailing-slashes -T tree/ dir/

# Spelling. The name is matched by unambiguous prefix like every other long
# option, and `getopt_long` permutes, so the option may follow the operands it
# governs.
TREE='mktree; ln -s tree treelink'
run_case -v --str treelink/ dir
# Permuted *and* the one asymmetry worth its own case: the destination here is a
# directory, so `n_files--` took it out of the loop and it keeps its slash —
# which the verbose line then joins to the source's basename. The option was
# given, and the destination is the one operand it did not touch.
TREE='mktree; ln -s tree treelink'
run_case -v file.txt treelink/ --strip-trailing-slashes

# =============================================================================
# 22. Across a filesystem boundary
# =============================================================================
# The one section whose cases are not about an option. `rename(2)` returns
# `EXDEV` when its two operands are on different filesystems, and every `mv`
# answers that by doing the move the long way: copy the source, then unlink it.
# What makes the section worth its length is that a rename gets for free
# everything the copy has to reproduce deliberately — the mode including its
# set-ID and sticky bits, both timestamps at nanosecond resolution, the owner,
# the extended attributes, a symlink's text rather than its target's bytes,
# a directory's whole subtree, and the identity of a hard-linked group — and
# each of those is a separate chance to be wrong in a way that only a second
# filesystem can show.
#
# GNU's fallback is `copy.c:2833-2892`, reached from `movefile`'s
# `if (rename_errno == EXDEV)`. Read in order it is: refuse unless the errno
# really was `EXDEV` and `--no-copy` was not given; **unlink the destination**;
# print `copied` if verbose; set `new_dst`; then copy with `preserve_ownership`
# on, which is what makes the group and other permission bits get held back
# until after the `chown`. The unlink is the part that is easiest to miss and
# the hardest to justify from the help text — its comment says it plainly:
#
#   /* Remove any existing destination file so that a cross-device `mv' acts
#      as if it were really using the rename syscall.  */
#
# A rename replaces the destination *directory entry*; it does not write
# through it. So a destination with a second hard link must come out of a
# cross-device `mv` with that other link still pointing at the old bytes, and a
# fallback that opens the destination for writing instead of unlinking it
# rewrites a file the user never named. That is what case 3 below measures.
#
# ## Where the second filesystem comes from
#
# `$far_root`, found at startup — see its definition for why the device is
# compared rather than assumed. If none was found the whole section is skipped
# rather than silently run on one device, because a §22 that ran on one device
# would exercise `rename(2)` while reporting that it had exercised the copy.
# A case names the far directory as `@FAR@` and builds its half of the fixture
# with [`FAR`]; `FAR=':'` is how a case that wants an *empty* far directory
# (because it is moving *towards* it) asks for one to exist.
#
# ## Both directions, and a control
#
# The fallback is not symmetric in the way it first appears: what is copied is
# read off the far side and written to the near one in the near→far cases and
# the reverse in the others, and the destination-clearing happens on whichever
# side the destination is. Both directions are here. So is a case whose two
# operands are *both* far, which is a plain rename and must agree — it is the
# control that certifies the section is testing the fallback because of where
# the files are and not because something about the far directory makes every
# case differ.

if [ -z "$far_root" ]; then
  echo "SKIP section 22: no writable directory found on a second device"
else

# The plainest possible fallback: one regular file, a fresh name on the other
# filesystem. It carries a set-user-ID bit and a pinned sub-second time so that
# the case measures the three things the copy has to carry across rather than
# only that the bytes arrived.
TREE=''
FAR='printf hello > f; chmod 4741 f; touch -d "2001-02-03 04:05:06.123456789" f'
STAMPS=1
run_case -v @FAR@/f g

# The same file onto a name that already exists. Nothing about the fallback
# changes, but the destination's own mode and times must not survive under the
# new contents: a rename would have replaced the inode entirely.
TREE='printf XXXXXXXXXX > g; chmod 606 g'
FAR='printf hello > f; chmod 4741 f; touch -d "2001-02-03 04:05:06.123456789" f'
STAMPS=1
run_case -v @FAR@/f g

# The case the unlink quoted above exists for. `g` and `g2` are one inode with
# two names, and only `g` is named on the command line. After a rename `g2`
# still holds the old ten bytes at mode 606; after a fallback that truncates
# and rewrites the destination in place it holds the new five at mode 644, and
# a file nobody mentioned has been overwritten.
#
# The source is left at the default mode and carries no pinned time, unlike
# almost every other fixture in this section. That is deliberate: a set-user-ID
# bit here would make the case differ for the copy's *attribute* losses too, and
# it would go on differing after the unlink landed — a case that measures two
# defects at once cannot report either of them being fixed.
TREE='printf XXXXXXXXXX > g; chmod 606 g; ln g g2'
FAR='printf hello > f'
run_case -v @FAR@/f g

# A symlink. The fallback must copy the link — its text, not its target's bytes
# — which means `readlink` and `symlink` and not `open`. The target is left
# behind on the far side deliberately: if the copy resolved the link the
# arriving `g` would be a five-byte regular file and the tree column says so.
TREE=''
FAR='printf hello > t; ln -s t l; touch -h -d "2001-02-03 04:05:06" l'
STAMPS=1
run_case -v @FAR@/l g

# The same, dangling. Nothing can be opened at all, so a fallback that resolves
# links cannot even appear to work. It has no pinned time, unlike the case
# above: `touch -h` on a link to nowhere is refused on some filesystems, so the
# pair is deliberately split — the case above measures the link's own stamp and
# this one measures that the link text survives with nothing behind it.
TREE=''
FAR='ln -s nowhere l'
run_case -v @FAR@/l g

# An absolute link text, which must arrive unrewritten. A fallback that
# reconstructed the link relative to its new directory would silently repoint
# it, and the tree column's `->` is where that shows.
TREE=''
FAR='ln -s /etc/hostname l'
run_case -v @FAR@/l g

# Two names for one far inode, both moved, in one command. A rename would have
# kept them one inode; the fallback has to notice the second source is already
# copied and link to the first result instead of copying twice. The link column
# is the whole case — the tree and the bytes agree either way.
TREE='mkdir d'
FAR='printf hello > a; ln a b'
run_case -v @FAR@/a @FAR@/b d

# Three names, so that the middle one is reached with a count still above one
# and the last with a count of exactly one — the far side's `a` is gone by then.
# It is the case that separates `remember` from `lookup`: a rule that consulted
# the table only when the count was above one would copy `c` afresh and leave
# `d/c` a separate file.
TREE='mkdir d'
FAR='printf hello > a; ln a b; ln a c'
run_case -v @FAR@/a @FAR@/b @FAR@/c d

# The same pair with the destinations already occupied. The link is then made
# over a name in use — a link to a temporary followed by a rename — and `-v`
# reports the replacement as `removed 'd/b'`, printed from inside gnulib's
# `force_linkat` after the first operand's line rather than before the second's.
TREE='mkdir d; printf old > d/a; printf old > d/b'
FAR='printf hello > a; ln a b'
run_case -v @FAR@/a @FAR@/b d

# The same shape entirely on **one** filesystem, where no copy happens at all
# and the answer is still not two renames. GNU consults the table before the
# rename that is allowed to replace, so the second operand is linked to the
# first result and then unlinked: `renamed 'a' -> 'd/a'` / `removed 'd/b'` /
# `removed 'b'`, where two renames would have said `renamed` twice. The tree is
# identical either way — this case exists for the transcript.
TREE='mkdir d; printf old > d/a; printf old > d/b; printf hello > a; ln a b'
run_case -v a b d

# And the same again with the destinations *free*, where the speculative rename
# succeeds and nothing is ever recorded. Both operands are renamed, both lines
# say so, and the pair stays linked because a rename kept them so. The case pins
# the `rename_errno == 0` arm — a table consulted unconditionally would turn the
# second `renamed` into a `removed`.
TREE='mkdir d; printf hello > a; ln a b'
run_case -v a b d

# `--update`, which records into the table even though it is skipping. Here
# rather than in §17 because what it measures is the table, not the option: the
# `-u` cases up there all have a single source and could never see it.
#
# Both destinations are newer, so both operands are skipped and both sources
# survive — and yet `d/b` does not, because the second skip finds the first in
# the table and links over it (`copy.c:2380`, whose own comment admits it
# "replace[s] DST_NAME unconditionally, even if it was a newer separate file").
# The bytes of `d/b` are the case; `-v` says only `removed 'd/b'`.
TREE='mkdir d; printf newer > d/a; printf newer2 > d/b; touch -d "2030-01-01" d/a d/b; printf hello > a; ln a b'
run_case -uv a b d

# The same, but with `d/b` *older*, so the second operand is not skipped and
# reaches the ordinary `earlier_file` block instead. It links to `d/a` there —
# a destination this command never wrote, since the first operand was skipped —
# and then removes its source, which the skipped one did not. Two adjacent
# operands, two routes into one table, two different answers about whether the
# source lives.
TREE='mkdir d; printf newer > d/a; touch -d "2030-01-01" d/a; printf old > d/b; touch -d "2001-01-01" d/b; printf hello > a; ln a b'
run_case -uv a b d

# The same inode, but only one of its names moved. Here the *right* answer is
# two separate files, and the far side must keep `b` with its bytes: a fallback
# that unlinked the inode rather than the named link would lose it.
TREE=''
FAR='printf hello > a; ln a b'
run_case -v @FAR@/a g

# A directory. GNU recurses; this `mv` refuses, and the refusal is deliberate
# rather than an oversight — see `mv.rs`'s cross-device fallback. The setgid
# bit and the subdirectory are on the fixture so that the case keeps measuring
# something after the refusal is lifted.
TREE=''
FAR='mkdir -p d/sub; printf hello > d/f; chmod 2750 d'
xfail_case "B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED" \
  -v @FAR@/d g

# `-u` across the boundary. The comparison is made before the fallback is
# chosen, so the option decides whether the copy happens at all. Older
# destination: it does.
TREE='printf XXX > g; touch -d "1999-01-01" g'
FAR='printf hello > f; chmod 4741 f; touch -d "2001-02-03 04:05:06.123456789" f'
STAMPS=1
run_case -uv @FAR@/f g

# Newer destination: it does not, and no fallback is entered at all. It is the
# pair that separates `-u`'s comparison from the copy it guards: the case above
# exercises both and this one only the comparison, so a regression in either can
# be told from a regression in the other by which of the two goes red.
TREE='printf XXX > g; touch -d "2030-01-01" g'
FAR='printf hello > f; chmod 4741 f; touch -d "2001-02-03 04:05:06.123456789" f'
STAMPS=1
run_case -uv @FAR@/f g

# `-b` across the boundary. The backup is made on the destination's filesystem
# by renaming the destination aside, which is a same-device rename however far
# away the source is — so the backup itself must agree even while the copied
# file does not.
TREE='printf XXX > g'
FAR='printf hello > f; chmod 4741 f; touch -d "2001-02-03 04:05:06.123456789" f'
STAMPS=1
run_case -bv @FAR@/f g

# The other direction: near to far. Everything above reads from the far side;
# this reads from the near one and writes to the far, and a fallback that got
# an argument order backwards would pass the cases above and fail this one.
TREE='printf hello > f; chmod 4741 f; touch -d "2001-02-03 04:05:06.123456789" f'
FAR=':'
STAMPS=1
run_case -v f @FAR@/g

# The control. Both operands are on the far filesystem, so this is a rename and
# must agree exactly — including the verbose line's word, which is `renamed`
# here and `copied` in every case above. If this one ever differs, the section
# is measuring the far directory rather than the boundary.
TREE=''
FAR='printf hello > f; chmod 4741 f; touch -d "2001-02-03 04:05:06.123456789" f'
STAMPS=1
run_case -v @FAR@/f @FAR@/g

# One command line, two sources, one on each side, into a near directory. The
# per-operand decision is what is being measured: `n` is a rename and `f` is a
# copy, in the same run, and the verbose lines say which was which. No pinned
# times and no special bits on the fixture, deliberately — this case is about
# the routing and agrees today; the copy's own losses have their own cases and
# would only make this one differ for a reason it is not asking about.
TREE='mkdir d; printf near > n'
FAR='printf far > f'
run_case -v n @FAR@/f d

# A far source that cannot be read. The fallback has to open it, so this fails
# where a rename would have succeeded — the one place where being on another
# filesystem changes whether a move is *possible* rather than only how it is
# done. Both sides fail, with the same errno, after the same `copied` line, and
# now with the same sentence: `cannot open 'f' for reading`, which names the
# step rather than the operation. This case is why the fallback opens the source
# itself instead of handing both names to one library call — the call returns
# one error for the read end and the write end alike, and the case below is that
# same errno at the other end.
TREE=''
FAR='printf hello > f; chmod 000 f'
run_case -v @FAR@/f g

# A destination directory that cannot be written. The failure is on the near
# side and before any bytes move, so the far source must still be there
# afterwards: a fallback that unlinked the source before confirming the write
# would have destroyed it. Both sides leave it, and this is the second half of
# the pair above — the same errno at the *other* end of the copy, and the two
# sentences have to disagree with each other: `cannot create regular file 'd/g'`
# here against `cannot open 'f' for reading` there.
TREE='mkdir d; chmod 555 d'
FAR='printf hello > f'
run_case -v @FAR@/f d/g

# `-n` and `-i` across the boundary, to certify that the refusal happens before
# the fallback is chosen rather than after the copy has already been made.
TREE='printf XXX > g'
FAR='printf hello > f'
run_case -nv @FAR@/f g
TREE='printf XXX > g'
FAR='printf hello > f'
ANSWERS=$'n\n'
run_case -iv @FAR@/f g

# `-u` across the boundary, which is a *different comparison* from §17's, not
# just the same one in a new place. GNU passes `utimecmp` a "truncate the
# source" flag whenever the move crosses a filesystem (`copy.c:2359`:
# `preserve_timestamps && !(move_mode && same st_dev)`, and `mv` sets both of
# those unconditionally), because the source's time is about to be *written*
# onto the far side rather than carried by a rename — so what the destination
# will end up holding is the source's time as that filesystem can store it, and
# comparing against the untruncated one makes a repeated `mv -u` never settle.
#
# The five cases below are the comparison's shapes: equal, outside the
# two-second quick exit in each direction, and inside it differing only below
# the second, both ways round.
#
# What this cannot measure, and it is most of it: both filesystems here keep
# nanoseconds. The deduced resolution is therefore one nanosecond, truncation
# is the identity, and — because upstream skips the whole deduction when the
# resolution is already the syscall's — control never reaches the part of
# `utimecmp` that the flag exists for. Breaking that part outright leaves all
# five of these passing; only the unit tests in `utimecmp.rs` catch it.
# Measuring it here would need a destination that stores whole seconds — a FAT
# image — which the harness cannot mount without root. What these five do pin
# is that turning the flag on changed nothing on a filesystem that loses
# nothing, which is a real regression risk: the deduction reads the
# destination's stamps and writes a probe onto it, and `STAMPS=1` here would
# catch a probe left behind or a destination disturbed by a comparison.
TREE='printf old > g; touch -d "2009-09-09 09:09:09.987654321" g'
FAR='printf hello > f; touch -d "2004-04-04 04:04:04.123456789" f'
STAMPS=1
run_case -uv @FAR@/f g
TREE='printf old > g; touch -d "1999-09-09 09:09:09.987654321" g'
FAR='printf hello > f; touch -d "2004-04-04 04:04:04.123456789" f'
STAMPS=1
run_case -uv @FAR@/f g
# Same instant on both sides: equal is not older, so nothing moves.
TREE='printf old > g; touch -d "2004-04-04 04:04:04.123456789" g'
FAR='printf hello > f; touch -d "2004-04-04 04:04:04.123456789" f'
STAMPS=1
run_case -uv @FAR@/f g
# Inside the two-second window and differing only below the second, in both
# directions — past the quick exits, so the resolution gets deduced. A
# comparison that rounded to whole seconds would call these equal and stop
# moving the file in the second case.
TREE='printf old > g; touch -d "2004-04-04 04:04:04.500000000" g'
FAR='printf hello > f; touch -d "2004-04-04 04:04:04.400000000" f'
STAMPS=1
run_case -uv @FAR@/f g
TREE='printf old > g; touch -d "2004-04-04 04:04:04.400000000" g'
FAR='printf hello > f; touch -d "2004-04-04 04:04:04.500000000" f'
STAMPS=1
run_case -uv @FAR@/f g

# `--no-copy` is the option that turns this whole section off: across a
# boundary it makes the `EXDEV` an error instead of a fallback, which is the
# only place it has any behaviour at all. This is §20's entry given the one
# fixture that can measure it.
TREE=''
FAR='printf hello > f'
missing --no-copy @FAR@/f g

fi

# =============================================================================
# 23. Extended attributes
# =============================================================================
# The comparison these cases exist to exercise is `xattrs`, added the same day
# as the section; before it, every case below would have passed with the
# attributes thrown away.
#
# Measured, not assumed, and re-measured when the section grew to eight cases:
# with `preserve_xattrs` in `mv.rs` short-circuited to `return` and nothing else
# changed, this run went from 360/0 to 353/7. Six of the seven are the six
# cross-boundary cases below; both same-filesystem ones stayed green, which is
# exactly right, since a `rename(2)` carries attributes whatever our code does.
# Six of eight is the honest ceiling for this section until a directory can
# cross.
#
# The seventh is in §24 — the case that sets an ACL *and* a `user.tag` on one
# file — and it is worth knowing that the two sections overlap there rather than
# being surprised by it later. That case is the only one in the suite holding
# both attribute classes at once, so it is the only one outside this section
# that a change to the ordinary-attribute path can move.
#
# A second probe, narrower, certifies the last two cases specifically: with only
# `top_up_extra` short-circuited — the repair that re-adds owner-write when the
# umask ate it, everything else including the creation-time grant left alone —
# the run went 360/0 to 358/2, and the two reds were exactly the `[umask: 0222]`
# pair. The `chmod 444` case without a umask stayed green, so the grant and the
# repair are each carrying their own case and neither is decoration.
#
# A same-filesystem `mv` is a `rename(2)`: it moves a directory entry and never
# touches the inode, so the attributes cannot be lost and the first case is a
# control rather than a test. Every case that can actually fail is a §22-shaped
# one, where the fallback has to read each attribute off the source and write it
# onto a new inode — which is why this section needs the far filesystem too and
# is skipped with it.
#
# What is deliberately not here: an attribute on a *symlink*. Linux refuses
# `user.*` on anything but a regular file or a directory (the permission model
# for the namespace is the file's own mode, and a symlink has none), so such a
# case would be testing the kernel's refusal on both sides and nothing about
# `mv`. `security.*` on a link is allowed but needs privilege to set.
if [ -z "$far_root" ] || [ -z "$DIFF_XATTR" ] || [ "$DIFF_XATTR_REF" != yes ]; then
  echo "SKIP section 23: needs a second device, a working setxattr, and a"
  echo "                reference built with USE_XATTR (see diff-wsl.sh's libattr"
  echo "                block -- without it GNU's copy_attr is an empty stub that"
  echo "                drops every attribute, so every case here would 'differ')"
else

# The control: one filesystem, so this is a rename and the attribute survives
# because nothing was copied. If this one ever fails, the section is measuring
# the fixture rather than the move.
#
# It is worth being explicit that this case is the *weak* kind: if `TREE`'s
# `diff_setxattr` silently did nothing — `eval "$TREE"` discards its stderr —
# both sides would hold no attributes and it would pass anyway. That is checked
# separately rather than inferred: the temporary root the trees are built under
# was confirmed to accept and return a `user.*` attribute, and `DIFF_XATTR`'s
# own probe above sets one there on every run before naming a Python at all.
TREE='printf hello > f; diff_setxattr f user.tag v1'
FAR=':'
run_case -v f g

# The same file across the boundary. Now the fallback runs, and the attribute
# has to be carried by hand.
TREE=''
FAR='printf hello > f; diff_setxattr f user.tag v1'
run_case -v @FAR@/f g

# Several attributes, one of them not printable ASCII. Two things at once, and
# deliberately: the count, since a fallback that carried only the first would
# pass the case above, and the value, since one that carried the *names* and
# re-read the values as text would mangle this one.
TREE=''
FAR='printf hello > f
      diff_setxattr f user.a one
      diff_setxattr f user.b two
      diff_setxattr f user.c "héllo"'
run_case -v @FAR@/f g

# A destination that already exists and carries its own attribute. A rename
# replaces the inode entirely, so `user.old` must be *gone* afterwards and not
# merged with the source's. This is the attribute half of §22's second case, and
# it is the one a fallback that opened the destination in place would fail: the
# attributes of a file that was written through rather than replaced are its own
# plus the source's.
TREE='printf XXXXXXXXXX > g; diff_setxattr g user.old keepme'
FAR='printf hello > f; diff_setxattr f user.new v'
run_case -v @FAR@/f g

# A *read-only* source, whose copy is a file no `setxattr` can reach unless the
# move goes out of its way. Linux's `xattr_permission` (`fs/xattr.c`) demands
# write access to the inode before it will set an attribute on it, so a copy
# created at the source's `0444` cannot be given the source's attributes — and
# `mv` would report each refusal, turning a silent move into a screenful.
#
# GNU's answer is to create the destination with `S_IWUSR` added (`copy.c:1451`)
# and let the closing `copy_acl` take it off with the rest of the temporary
# mode. This case is the control for that: the bit is asked for, it arrives, and
# the attribute crosses.
TREE=''
FAR='printf hello > f; diff_setxattr f user.tag v1; chmod 444 f'
run_case -v @FAR@/f g

# And the same move under a umask that strips the very bit being asked for.
# `umask 0222` takes owner-write off everything the kernel creates, so the mode
# handed to `open` is not the mode that arrives, and asking is not having. GNU
# re-adds it with an explicit chmod after the open and gives up gracefully if
# even that fails (`copy.c:1539`); this is the only case in the suite that
# reaches that repair.
UMASK=0222 TREE=''
FAR='printf hello > f; diff_setxattr f user.tag v1; chmod 444 f'
run_case -v @FAR@/f g
# `0400`: no group or other bits at all, so there is nothing for the withholding
# to withhold and the extra bit is the whole difference between the creation
# mode and the final one.
UMASK=0222 TREE=''
FAR='printf hello > f; diff_setxattr f user.tag v1; chmod 400 f'
run_case -v @FAR@/f g

# An attribute on a directory rather than a file, moved within one filesystem.
# A directory cannot cross the boundary at all yet
# (`B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-REFUSED`), so this is the only shape
# a directory attribute can be tested in until that lands — and when it does,
# the far version of this case is the one to add.
TREE='mkdir d; printf x > d/f; diff_setxattr d user.dir v'
FAR=':'
run_case -v d e

fi

# =============================================================================
# 24. Access-control lists
# =============================================================================
# An ACL is a permission list finer than the nine mode bits — "user 0 may write
# this as well as the owner". It is compared by the same `xattrs` the section
# above added, because on Linux an ACL *is* the extended attribute
# `system.posix_acl_access`; what it needs of its own is a way to make one, and
# a reference that keeps one.
#
# It is a separate section from §23 rather than more cases in it because the two
# gates are independent and fail differently. §23 needs `USE_XATTR`, without
# which the reference *refuses* `--preserve=xattr` outright; this needs
# `USE_ACL`, without which the reference copies happily and merely drops the
# entries while carrying the bits. A single gate would skip cases that could
# have run, and a single skip message could not say which fact was missing.
#
# `mv` has no options here — it preserves everything it can, always — so unlike
# `cp` there is no negative case to write. What varies is only whether the move
# is a `rename(2)` or the copy-and-unlink fallback, which is the same split §22
# and §23 are built around.
#
# Measured, not assumed: with the `copy_xattrs(.., Xattrs::Permissions)` call in
# `fsattr::copy_permissions` short-circuited — leaving the chmod and the
# clearing, which is exactly a gnulib built without libacl, carrying the bits
# and dropping the entries — this run goes from 357/0 to 353/4. The four are the
# four that cross the boundary; the control and the directory below it stay
# green, which is right, since a `rename(2)` never touches the inode and so
# carries the list whatever our code does. Four of six is the honest ceiling
# here for the same reason §23's is three of five.
if [ -z "$far_root" ] || [ -z "$DIFF_SETFACL" ] || [ "$DIFF_ACL_REF" != yes ]; then
  echo "SKIP section 24: needs a second device, a working setfacl, and a"
  echo "                reference built with USE_ACL (without it gnulib compiles"
  echo "                copy_acl down to a plain chmod, so it carries the mode"
  echo "                bits and silently drops the entries)"
else

# The control, as in §23: one filesystem, so this is a rename, the inode is
# never touched and the list survives whatever `mv` does. If it fails, the
# fixture is what is broken.
TREE='printf hello > f; diff_setfacl f u:0:rwx'
FAR=':'
run_case -v f g

# Across the boundary, where the fallback has to read the list off the source
# and write it onto a new inode.
TREE=''
FAR='printf hello > f; diff_setfacl f u:0:rwx'
run_case -v @FAR@/f g

# Two entries, so that a fallback carrying only the first is caught, and a
# group entry alongside the user one — adding a named entry makes `setfacl`
# synthesise a mask, which has to arrive as it was and not be recomputed from
# the mode at the far end.
TREE=''
FAR='printf hello > f
      diff_setfacl f u:0:rwx
      diff_setfacl f g:0:r-x'
run_case -v @FAR@/f g

# A destination that already exists and carries its own list. A rename replaces
# the inode, so the destination's entry must be *gone* and not merged with the
# source's — the ACL half of §22's second case, and the one a fallback that
# opened the destination in place would fail.
TREE='printf XXXXXXXXXX > g; diff_setfacl g g:0:r-x'
FAR='printf hello > f; diff_setfacl f u:0:rwx'
run_case -v @FAR@/f g

# A list and an ordinary attribute on one file. These are two entries in one
# attribute list carried by two different pieces of GNU's code, so a fallback
# that handled either alone passes one of the cases above and fails this.
TREE=''
FAR='printf hello > f
      diff_setfacl f u:0:rwx
      diff_setxattr f user.tag v1'
run_case -v @FAR@/f g

# A directory's *default* ACL — `system.posix_acl_default`, a second attribute
# that new children inherit — within one filesystem, which is the only shape a
# directory can be moved in until `B-MVS-CROSS-DEVICE-DIRECTORY-MOVES-ARE-
# REFUSED` lands. As with §23's last case, the far version is the one to add
# when it does.
TREE='mkdir d; printf x > d/f; diff_setfacl d u:0:rwx; diff_setfacl d d:u:0:rwx'
FAR=':'
run_case -v d e

fi

# =============================================================================
echo
printf '%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
[ "$xpass" -gt 0 ] && printf ', %d NO LONGER DIFFER (XPASS)' "$xpass"
echo
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
