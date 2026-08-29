#!/usr/bin/env bash
# Differential test: our tar against GNU tar.
#
# tar is not a filter, so unlike every other harness here the thing to compare
# is rarely stdout. It is the *archive* one side produced and the *tree* the
# other side unpacked, and both of those carry metadata — a mode, an owner, a
# timestamp — that never appears on any stream. A harness that compared only
# stdout would have reported this tar as perfect while it was resetting every
# extracted file to mode 644 and every directory to 1970.
#
# So there are four kinds of case, and each answers a different question:
#
#   create_case   do we write the same bytes GNU writes?
#   interop_case  can GNU read what we wrote, and does it read the same names?
#   list_case     do we print the same listing for an archive GNU wrote?
#   extract_case  does the tree we unpack have the same metadata as GNU's?
#
# `create_case` is the strictest and `extract_case` is the one that catches
# what the others cannot see: two archives can hold the same names and sizes
# and still restore different permissions.
#
# ## Normalising GNU, and why each flag is fair
#
# GNU tar's defaults produce its own extended format, which ours does not claim
# to write. Three flags bring it to the format ours does claim:
#
#   --format=ustar        ustar is what `tar.rs` implements and says it does.
#   --blocking-factor=1   GNU pads the archive to a 10 KiB record; ours ends
#                         after the two zero blocks that ustar requires. The
#                         padding is a tape-drive artefact, not a format rule.
#   --numeric-owner       on create this leaves `uname`/`gname` empty, which is
#                         what ours writes. Filling them needs a passwd lookup
#                         (see the xfail at the bottom), so normalising it here
#                         keeps that one known gap from masking every other
#                         difference in the same 512 bytes.
#   --sort=name           GNU walks a directory in whatever order `readdir`
#                         returns; ours sorts, so that archiving the same tree
#                         twice gives the same bytes. ustar imposes no order at
#                         all, so neither is wrong — but a reference whose
#                         output order is not reproducible cannot be compared
#                         against anything, so the reference is the one that
#                         gets pinned.
#
# Nothing else is normalised. In particular the mode, uid, gid and mtime fields
# are compared as written, because those are the fields a backup exists to
# preserve.
#
# Run `OURS=/usr/bin/tar ./scripts/tar-diff.sh` to confirm the harness still
# discriminates: it should report every xfail as XPASS and nothing else. (The
# create cases then compare GNU's defaults against normalised GNU, so a couple
# of those legitimately still differ; the xfail line for each says so.)
set -u

DIFF_PROG='tar'
DIFF_NEED='find stat cmp od sha256sum touch ln readlink mkfifo'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; xfail=0; xpass=0

# GNU's format normalisation. See the header.
GNUFMT="--format=ustar --blocking-factor=1 --numeric-owner --sort=name"

work=$DIFF_TMP/work
mkdir -p "$work"
cd "$work" >/dev/null || exit 1

# ---------------------------------------------------------------------------
# The fixture tree.
#
# Every mode and every timestamp is set explicitly, because those are the
# fields under test and a fixture that inherited them from the checkout would
# make the comparison depend on how the repository was cloned. The modes are
# chosen to be distinguishable after a umask of 022: 0755 and 0600 survive it
# unchanged, 0777 does not, and a harness that only used 0644 could not tell a
# tar that restores modes from one that does not.
# ---------------------------------------------------------------------------
NONUTF8=$(printf 'caf\351.txt')

build_tree() {
  rm -rf tree
  mkdir -p tree/sub tree/empty-dir
  printf 'hello\n'        > tree/a.txt
  printf 'x'              > tree/sub/b.bin
  printf 'z\n'            > "tree/$NONUTF8"
  : > tree/zero.txt
  printf '\0\1\377\376'   > tree/binary.dat
  chmod 755 tree/a.txt
  chmod 600 tree/sub/b.bin
  chmod 700 tree/sub
  chmod 644 "tree/$NONUTF8"
  # A fixed mtime, in the past, distinct from "now" so that a tar which stamps
  # the extraction time instead of the archived time is visible.
  touch -d '2020-01-02 03:04:05' \
    tree/a.txt tree/sub/b.bin "tree/$NONUTF8" tree/zero.txt tree/binary.dat \
    tree/sub tree/empty-dir tree
}
build_tree

# A second fixture tree, for the types the first one has none of: a symlink, a
# hard link, a fifo, and the two symlink targets an extractor has to treat as
# hostile. It is separate rather than folded into `tree` so that the cases over
# `tree` stay a comparison of the plain-file path alone — a symlink there would
# make every one of them pass or fail for a reason unrelated to its name.
#
# `up` and `abs` are the point of the tree. A symlink whose target is absolute
# or climbs out is the one archive member that can make a *later* member land
# outside the destination, so GNU withholds it until the archive is finished.
# Nothing else here can tell whether that machinery exists.
build_special() {
  rm -rf special
  mkdir -p special/d
  printf 'hello\n'      > special/f
  ln    special/f         special/hard
  ln -s f                 special/rel
  ln -s /no/such/where    special/dangling
  ln -s ../outside        special/up
  ln -s /etc/passwd       special/abs
  mkfifo special/pipe
  printf 'x'            > special/d/inner
  # 0777 does not survive a umask of 022, so it distinguishes a tar that
  # restores the archived mode from one that lets the umask decide; 0700 on a
  # directory catches the same thing for the deferred directory pass.
  chmod 0777 special/f
  chmod 0700 special/d
  touch -d '2020-01-02 03:04:05' \
    special/f special/hard special/pipe special/d special/d/inner special
  # A symlink's own mtime is restored separately, without following it, so it
  # is given a different stamp from everything else.
  touch -h -d '2019-05-06 07:08:09' \
    special/rel special/dangling special/up special/abs
}
build_special

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

xreport() {
  local label="$1" reason="$2"
  if [ "$AGREED" = no ]; then
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'XFAIL %s  (%s)\n' "$label" "$reason"
  else
    xpass=$((xpass+1))
    printf 'XPASS %s\n  now agrees with GNU, so this reason is stale: %s\n' \
      "$label" "$reason"
  fi
  return 0
}

# Compare two (rc, stdout, stderr) triples that the caller has already
# produced into $DIFF_TMP/{o,g}.{out,err} and passed the statuses of.
#
# stdout is hex-dumped: a member name need not be text, and `$(...)` would eat
# a trailing newline and any NUL. stderr is compared as text, because its whole
# content is a diagnostic and the wording is what is being checked.
settle() {
  local o_rc=$1 g_rc=$2 extra_o="${3:-}" extra_g="${4:-}"
  local o_out g_out o_err g_err
  o_out=$(od -An -tx1 <"$DIFF_TMP/o.out"); g_out=$(od -An -tx1 <"$DIFF_TMP/g.out")
  o_err=$(cat "$DIFF_TMP/o.err");          g_err=$(cat "$DIFF_TMP/g.err")
  if [ "$o_rc" = "$g_rc" ] && [ "$o_out" = "$g_out" ] && [ "$o_err" = "$g_err" ] \
     && [ "$extra_o" = "$extra_g" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(printf '  ours (rc=%s): out{%s} err{%s}%s\n  gnu  (rc=%s): out{%s} err{%s}%s' \
    "$o_rc" "$(printf '%s' "$o_out" | tr -s ' \n' ' ')" "$(printf '%s' "$o_err" | tr '\n' '|')" \
    "$([ -n "$extra_o" ] && printf ' %s' "$extra_o")" \
    "$g_rc" "$(printf '%s' "$g_out" | tr -s ' \n' ' ')" "$(printf '%s' "$g_err" | tr '\n' '|')" \
    "$([ -n "$extra_g" ] && printf ' %s' "$extra_g")")
}

# ---------------------------------------------------------------------------
# create — the archive bytes themselves
# ---------------------------------------------------------------------------
# The archive is dumped as a hex summary rather than compared with `cmp` alone,
# so that a difference names the offset and the two byte values instead of
# saying only "they differ". The first differing 512-byte block is what a tar
# bug is almost always confined to, and the header layout is fixed, so an
# offset within it identifies the field.
archive_delta() {
  local a=$1 b=$2
  if cmp -s "$a" "$b"; then printf 'same'; return; fi
  local off
  off=$(cmp "$a" "$b" 2>/dev/null | sed 's/.*byte \([0-9]*\),.*/\1/')
  printf 'differ at byte %s (block %s, offset %s: %s)' \
    "$off" "$(( (off-1) / 512 ))" "$(( (off-1) % 512 ))" \
    "$(header_field $(( (off-1) % 512 )))"
}

# Which ustar header field a byte offset lands in. Prose, so a failure line is
# readable without the format spec open beside it.
header_field() {
  local o=$1
  if   [ "$o" -lt 100 ]; then printf 'name'
  elif [ "$o" -lt 108 ]; then printf 'mode'
  elif [ "$o" -lt 116 ]; then printf 'uid'
  elif [ "$o" -lt 124 ]; then printf 'gid'
  elif [ "$o" -lt 136 ]; then printf 'size'
  elif [ "$o" -lt 148 ]; then printf 'mtime'
  elif [ "$o" -lt 156 ]; then printf 'checksum'
  elif [ "$o" -lt 157 ]; then printf 'typeflag'
  elif [ "$o" -lt 257 ]; then printf 'linkname'
  elif [ "$o" -lt 263 ]; then printf 'magic'
  elif [ "$o" -lt 265 ]; then printf 'version'
  elif [ "$o" -lt 297 ]; then printf 'uname'
  elif [ "$o" -lt 329 ]; then printf 'gname'
  elif [ "$o" -lt 337 ]; then printf 'devmajor'
  elif [ "$o" -lt 345 ]; then printf 'devminor'
  elif [ "$o" -lt 500 ]; then printf 'prefix'
  else printf 'padding or file data'
  fi
}

# create_case LABEL ARGS...  — `-cf X.tar ARGS` on both sides.
create_case() {
  local label="$1"; shift
  local o_rc g_rc
  rm -f o.tar g.tar
  diff_run env PATH="$bindir/ours" tar -cf o.tar "$@" \
    </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  # shellcheck disable=SC2086  # GNUFMT is three separate words on purpose.
  diff_run env PATH="$bindir/gnu" tar $GNUFMT -cf g.tar "$@" \
    </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  settle "$o_rc" "$g_rc" "archive:$(archive_delta o.tar g.tar)" 'archive:same'
  report "create: tar -cf X $label"
}

# ---------------------------------------------------------------------------
# interop — GNU reading our archive
# ---------------------------------------------------------------------------
# Byte equality is the strong claim; this is the weaker one that still has to
# hold if the archive is to be useful at all. It is kept separate because it
# fails for different reasons: a name our tar truncated at 99 bytes produces a
# perfectly well-formed archive that GNU reads without complaint, and the only
# sign is that the name it reads back is not the name that went in.
interop_case() {
  local label="$1"; shift
  local o_rc g_rc
  rm -f o.tar g.tar
  diff_run env PATH="$bindir/ours" tar -cf o.tar "$@" </dev/null >/dev/null 2>&1
  # shellcheck disable=SC2086
  diff_run env PATH="$bindir/gnu" tar $GNUFMT -cf g.tar "$@" </dev/null >/dev/null 2>&1
  # Both archives are read by the *same* tar — GNU's — so any difference in the
  # listing is a difference in the archives and not in the two readers.
  diff_run "$gnu_real" -tvf o.tar >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"; o_rc=$?
  diff_run "$gnu_real" -tvf g.tar >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"; g_rc=$?
  settle "$o_rc" "$g_rc"
  report "interop: GNU -tvf reads ours the same as its own, for $label"
}

# ---------------------------------------------------------------------------
# list — reading an archive GNU wrote
# ---------------------------------------------------------------------------
list_case() {
  local label="$1"; shift
  local o_rc g_rc
  diff_run env PATH="$bindir/ours" tar "$@" </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"
  o_rc=$?
  diff_run env PATH="$bindir/gnu" tar "$@" </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"
  g_rc=$?
  settle "$o_rc" "$g_rc"
  report "list: tar $* ($label)"
}

# ---------------------------------------------------------------------------
# extract — the unpacked tree, metadata included
# ---------------------------------------------------------------------------
# The manifest is the point of this harness. `%a` is the permission bits, `%Y`
# the mtime, `%s` the size, and the content is hashed; between them they cover
# everything a restore is supposed to bring back and nothing that varies
# between two runs. Ownership is deliberately absent: as a non-root user
# neither side can restore it, so comparing it would compare nothing.
#
# One mtime cannot be compared literally: a directory the extraction *invented*
# (or merely wrote into) is stamped with the wall clock, so the two runs disagree
# whenever they straddle a second boundary — which they did, intermittently, on
# the destination root itself. Rounding or ignoring mtimes wholesale would throw
# away the check that matters, since restoring the archive's stamps is most of
# what a manifest is for. Instead any stamp within five minutes of now is printed
# as the token `NOW`: every fixture's stored mtimes are 2018-2020 dates, so a
# recent stamp means "assigned by the extraction", and *that* both sides must
# still agree about — a tar that failed to invent a directory has no line at all.
#
# A symlink *target* gets one substitution for the same reason. The two runs
# unpack into `od` and `gd`, so a fixture that plants a link naming its own
# destination — which the ancestor cases must, since pointing back inside by an
# absolute or climbing path is the behaviour under test — records two different
# strings for identical behaviour. The destination's own name becomes `@D` and
# the work directory above it `@W`, which leaves every other byte of the target
# compared literally.
manifest() {
  ( cd "$1" 2>/dev/null || return 0
    dest="$1"
    now=$(date +%s)
    # mt EPOCH — the stamp, or `NOW` if the extraction just made it up.
    mt() {
      d=$(( now - $1 )); [ "$d" -lt 0 ] && d=$(( -d ))
      if [ "$d" -le 300 ]; then printf 'NOW'; else printf '%s' "$1"; fi
    }
    find . | LC_ALL=C sort | while IFS= read -r p; do
      # `-h` is tested first because every other test follows the link: a
      # symlink to a file would otherwise be reported as that file, and a
      # dangling one would fall through to a `stat` that fails. The target is
      # part of the comparison — restoring a symlink pointing somewhere else is
      # precisely the failure the delayed-link machinery exists to prevent, and
      # it is invisible in mode, size and mtime.
      if [ -h "$p" ]; then
        read -r mode mtime <<EOF
$(stat -c '%a %Y' -- "$p")
EOF
        tgt=$(readlink -- "$p")
        tgt=${tgt//"$work/$dest"/@W/@D}
        tgt=${tgt//"../$dest"/../@D}
        tgt=${tgt//"$work"/@W}
        printf 'l %s %s %s -> %s\n' "$p" "$mode" "$(mt "$mtime")" "$tgt"
      elif [ -d "$p" ]; then
        read -r mode mtime <<EOF
$(stat -c '%a %Y' -- "$p")
EOF
        printf 'd %s %s %s %s\n' "$p" "$mode" "$(mt "$mtime")" -
      elif [ -f "$p" ]; then
        # `%h` is the link count. A tar that writes a hard link as a second
        # copy produces identical bytes, an identical mode and an identical
        # mtime; the count is the only field that differs.
        read -r mode mtime size links <<EOF
$(stat -c '%a %Y %s %h' -- "$p")
EOF
        printf 'f %s %s %s %s %s %s\n' "$p" "$mode" "$(mt "$mtime")" "$size" "$links" \
          "$(sha256sum <"$p" | cut -c1-16)"
      else
        # A fifo, a socket, a device. `%F` names which.
        read -r mode mtime <<EOF
$(stat -c '%a %Y' -- "$p")
EOF
        printf '? %s %s %s %s\n' "$p" "$mode" "$(mt "$mtime")" \
          "$(stat -c '%F' -- "$p" | tr ' ' '-')"
      fi
    done
    # And *which* files are the same file. The inode numbers themselves differ
    # between the two runs by construction, so what can be compared is the
    # grouping: a hard link made to the wrong member has the right count and
    # the wrong company.
    find . -type f -links +1 -printf '%i %p\n' 2>/dev/null | LC_ALL=C sort \
      | awk '{ g[$1] = g[$1] " " $2 } END { for (k in g) print "=" g[k] }' \
      | LC_ALL=C sort )
}

# extract_case LABEL ARCHIVE [EXTRA-ARGS...]
#
# Honours `$PREP`: the name of a function run inside each destination before
# the extraction. Replacing an entry that already exists is a different code
# path from creating one — GNU attempts the creation, unlinks on EEXIST and
# retries, and the error the caller sees is the *first* one — so the two are
# worth separate cases.
extract_case() {
  local label="$1" archive="$2"; shift 2
  local o_rc g_rc o_man g_man
  rm -rf od gd; mkdir od gd
  if [ -n "${PREP:-}" ]; then ( cd od && "$PREP" ); ( cd gd && "$PREP" ); fi
  ( cd od && diff_run env PATH="$bindir/ours" tar -xf "../$archive" "$@" \
      </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err" ); o_rc=$?
  ( cd gd && diff_run env PATH="$bindir/gnu" tar -xf "../$archive" "$@" \
      </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err" ); g_rc=$?
  o_man=$(manifest od); g_man=$(manifest gd)
  settle "$o_rc" "$g_rc" "tree{$(printf '%s' "$o_man" | tr '\n' '|')}" \
                         "tree{$(printf '%s' "$g_man" | tr '\n' '|')}"
  report "extract: $label"
}

# The same, but the divergence is known and recorded rather than a failure.
extract_xcase() {
  local reason="$1"; shift
  local label="$1"
  extract_case_quiet "$@"
  xreport "extract: $label" "$reason"
}
extract_case_quiet() {
  local label="$1" archive="$2"; shift 2
  local o_rc g_rc o_man g_man
  rm -rf od gd; mkdir od gd
  ( cd od && diff_run env PATH="$bindir/ours" tar -xf "../$archive" "$@" \
      </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err" ); o_rc=$?
  ( cd gd && diff_run env PATH="$bindir/gnu" tar -xf "../$archive" "$@" \
      </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err" ); g_rc=$?
  o_man=$(manifest od); g_man=$(manifest gd)
  settle "$o_rc" "$g_rc" "tree{$(printf '%s' "$o_man" | tr '\n' '|')}" \
                         "tree{$(printf '%s' "$g_man" | tr '\n' '|')}"
}

# ---------------------------------------------------------------------------
# plain — a straight argv comparison, for the cases that only produce messages
# ---------------------------------------------------------------------------
plain_case() {
  local label="$1"; shift
  local o_rc g_rc
  diff_run env PATH="$bindir/ours" tar "$@" </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"
  o_rc=$?
  diff_run env PATH="$bindir/gnu" tar "$@" </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"
  g_rc=$?
  settle "$o_rc" "$g_rc"
  report "$label: tar $*"
}

plain_xcase() {
  local reason="$1" label="$2"; shift 2
  local o_rc g_rc
  diff_run env PATH="$bindir/ours" tar "$@" </dev/null >"$DIFF_TMP/o.out" 2>"$DIFF_TMP/o.err"
  o_rc=$?
  diff_run env PATH="$bindir/gnu" tar "$@" </dev/null >"$DIFF_TMP/g.out" 2>"$DIFF_TMP/g.err"
  g_rc=$?
  settle "$o_rc" "$g_rc"
  xreport "$label: tar $*" "$reason"
}

echo "tar-diff:"
echo "  ours: $OURS"
echo "  gnu:  $gnu_real ($("$gnu_real" --version 2>&1 | head -1))"

# ===========================================================================
# 1. creating an archive
# ===========================================================================
create_case 'tree'                       tree
create_case 'one file'                   tree/a.txt
create_case 'a file with no bytes'       tree/zero.txt
create_case 'a file that is not text'    tree/binary.dat
create_case 'a name that is not UTF-8'   "tree/$NONUTF8"
create_case 'an empty directory'         tree/empty-dir
create_case 'several operands'           tree/a.txt tree/zero.txt tree/sub
create_case 'symlinks, a hard link and a fifo' special
create_case 'a symlink on its own'       special/rel
create_case 'a dangling symlink'         special/dangling
create_case 'a fifo'                     special/pipe

# A member name longer than the 100-byte `name` field. ustar splits it at a `/`
# into `prefix` + `name`; a tar that only fills `name` truncates it, produces a
# well-formed archive holding the wrong name, and exits 0.
LONGDIR=$(printf 'd%.0s' $(seq 1 60))
LONGFILE=$(printf 'f%.0s' $(seq 1 50))
mkdir -p "long/$LONGDIR"
printf 'q\n' > "long/$LONGDIR/$LONGFILE"
touch -d '2020-01-02 03:04:05' "long/$LONGDIR/$LONGFILE" "long/$LONGDIR" long
create_case 'a name too long for the name field' long
interop_case 'a name too long for the name field' long

# ===========================================================================
# 2. GNU reading what we wrote
# ===========================================================================
interop_case 'tree'                     tree
interop_case 'a name that is not UTF-8' "tree/$NONUTF8"
interop_case 'an empty directory'       tree/empty-dir
# The typeflag, linkname and devmajor/devminor fields are only exercised here.
# A tar that stored a symlink as an empty regular file writes an archive GNU
# reads without a murmur; the listing is the only place it shows.
interop_case 'symlinks, a hard link and a fifo' special

# ===========================================================================
# 3. listing an archive GNU wrote
# ===========================================================================
# shellcheck disable=SC2086
"$gnu_real" $GNUFMT -cf ref.tar tree long
list_case 'a normal archive'  -tf ref.tar
list_case 'the same, with -v' -tvf ref.tar

# An archive that is not one. A reader that trusts the header can be walked off
# the end of the file by a size field, so a truncated and a corrupt archive are
# both worth a case.
printf 'this is not a tar file at all, not even close\n' > junk.tar
list_case 'not an archive'   -tf junk.tar
head -c 700 ref.tar > truncated.tar
list_case 'truncated mid-member' -tf truncated.tar
head -c 300 ref.tar > partial-header.tar
list_case 'truncated mid-header' -tf partial-header.tar

# ===========================================================================
# 4. extracting
# ===========================================================================
extract_case 'a tree GNU wrote'   ref.tar
extract_case 'one member by name' ref.tar tree/a.txt

# Archived from *inside* `special`, so the members are `./f`, `./rel` and a
# leading `.` for the directory itself. That is what `tar -cf - .` produces and
# it is the common shape in the wild; it also means the destination's own mode
# and mtime are restored from the `.` member, which is a case a `special/...`
# archive would not reach.
# shellcheck disable=SC2086
( cd special && "$gnu_real" $GNUFMT -cf ../spec.tar . )
extract_case 'symlinks, a hard link and a fifo' spec.tar
extract_case 'a symlink alone, by name'         spec.tar ./rel
extract_case 'a fifo alone, by name'            spec.tar ./pipe
extract_case 'with -p'                          spec.tar -p
# Saved and restored rather than run in a subshell: `report` increments the
# counters, and a subshell would throw its tally away.
saved_umask=$(umask); umask 077
extract_case 'under a umask of 077'             spec.tar
umask "$saved_umask"

# Extracting over entries that are already there, of deliberately mismatched
# types: a file where a symlink goes, a directory where a fifo goes, a
# *non-empty* directory where a symlink goes. The last is the one that
# separates `File exists` from `Directory not empty` — GNU discards the failed
# removal and reports the original EEXIST.
prep_existing() {
  printf 'old\n' > rel; mkdir -p pipe; printf 'z\n' > hard
  mkdir -p dangling; : > dangling/keep
  # A fixed stamp: whatever tar does not replace keeps its mtime, and a
  # wall-clock one differs between the two runs by construction.
  touch -d '2018-03-04 05:06:07' rel pipe hard dangling dangling/keep
}
PREP=prep_existing extract_case 'over existing entries of the wrong type' spec.tar

# `escape/` is where the traversal cases aim; the check that it stayed empty is
# further down, but the obstacle-course cases below need it to exist already.
mkdir -p escape

# Extracting a regular member over a file that has *another name*. An `O_TRUNC`
# open rewrites the shared inode, so `other` — which is not in the archive at
# all — silently changes too. GNU's `O_EXCL` + unlink + retry breaks the link
# instead, leaving `other` alone. `%h` and the `=`-grouping lines in `manifest`
# are what make the difference visible.
prep_hardlinked() {
  printf 'old-and-longer\n' > f; ln f other
  touch -d '2018-03-04 05:06:07' f other
}
PREP=prep_hardlinked extract_case 'over a file with another hard link' spec.tar ./f

# The same open, but the obstacle is a symlink pointing *outside* the
# destination. `O_TRUNC` follows it and writes at the far end — a traversal that
# none of the other defences cover, because the symlink came from the
# filesystem rather than from the archive. Both tars must replace the link.
prep_symlink_out() {
  ln -s ../escape/loot f
  ln -s ../escape/loot2 rel
}
PREP=prep_symlink_out extract_case 'over a symlink pointing outside' spec.tar ./f ./rel

# A *directory* member over a symlink that already points at a directory. The
# trap is that `is_dir()` follows the link, sees a directory, and reports the
# mkdir a success — leaving the link in place for `d/inner` to be written
# through. Both tars must end with a real directory at `d` and nothing in
# `escape/`.
prep_dir_symlink() {
  ln -s ../escape d
}
PREP=prep_dir_symlink extract_case 'a directory member over a symlink to one' spec.tar

# A directory member over a plain file: replaced, not refused.
prep_dir_over_file() {
  printf 'in the way\n' > d
  touch -d '2018-03-04 05:06:07' d
}
PREP=prep_dir_over_file extract_case 'a directory member over a plain file' spec.tar

# ---------------------------------------------------------------------------
# A symlink that is an *ancestor* of the member, already in the destination.
#
# The cases above all put the obstacle at a member's *own* path, where the
# creation itself meets it. This is the other shape and it is the dangerous
# one: `d/x` is a symlink pointing outside, the archive holds `x/pwned`, and an
# extractor that resolves the name normally writes at the far end. Neither the
# `..` refusal nor the delayed-symlink placeholder covers it — the link came
# from the filesystem, not the archive — and two archives are enough to arrange
# it even in an empty destination (the `twostep` case below).
#
# GNU's rule, measured member by member, is `openat2(RESOLVE_BENEATH)`: each
# component is judged as the walk reaches it, and a walk that would step above
# the destination is refused *there*, even if a later component would have come
# back. That is not the same as canonicalising and checking the prefix, and the
# difference is exactly the `absolute, inside` and `up and straight back in`
# cases below — a prefix check allows both; GNU refuses both. Every refusal is
# `EXDEV`, printed as `Invalid cross-device link`.
mkdir -p ancsrc/x
printf 'p\n' > ancsrc/x/pwned
ln -s elsewhere ancsrc/x/sl
mkfifo ancsrc/x/p
mkdir -p ancsrc/x/dir
( cd ancsrc && "$gnu_real" $GNUFMT -cf ../anc.tar x/pwned x/sl x/p x/dir )

# Allowed: the link stays inside, however indirectly.
prep_anc_inside()    { mkdir -p sub;      ln -s sub x; }
prep_anc_updown()    { mkdir -p sub deep; ln -s deep/../sub x; }
prep_anc_toroot()    { mkdir -p deep/er;  ln -s deep/er/../.. x; }
prep_anc_chain_in()  { mkdir -p sub y2;   ln -s y2/on x; ln -s ../sub y2/on; }
PREP=prep_anc_inside   extract_case 'an ancestor symlink pointing inside'      anc.tar
PREP=prep_anc_updown   extract_case 'an ancestor symlink whose .. comes back'  anc.tar
PREP=prep_anc_toroot   extract_case 'an ancestor symlink back to the root'     anc.tar
PREP=prep_anc_chain_in extract_case 'a chain of ancestor symlinks, all inside' anc.tar

# Refused: absolute at all, or a step above the root at any point. The two
# absolute ones point *into* the destination and are still refused, which is
# the fact that rules out a canonicalise-and-compare implementation.
prep_anc_absin()     { mkdir -p sub;      ln -s "$PWD/sub" x; }
prep_anc_absroot()   {                    ln -s "$PWD" x; }
prep_anc_backin()    { mkdir -p sub;      ln -s "../$(basename "$PWD")/sub" x; }
prep_anc_out()       {                    ln -s ../escape x; }
prep_anc_absout()    {                    ln -s "$work/escape" x; }
prep_anc_chain_out() { mkdir -p y2;       ln -s y2/on x; ln -s ../../escape y2/on; }
PREP=prep_anc_absin     extract_case 'an ancestor symlink, absolute but inside'  anc.tar
PREP=prep_anc_absroot   extract_case 'an ancestor symlink, absolute to the root' anc.tar
PREP=prep_anc_backin    extract_case 'an ancestor symlink up and straight back'  anc.tar
PREP=prep_anc_out       extract_case 'an ancestor symlink pointing outside'      anc.tar
PREP=prep_anc_absout    extract_case 'an ancestor symlink, absolute and outside' anc.tar
PREP=prep_anc_chain_out extract_case 'a chain whose second hop escapes'          anc.tar

# The two-step attack, which is what makes this reachable with no help from the
# filesystem: the first archive holds only the symlink member `x -> ../escape`,
# which both tars withhold as a placeholder and then create at the end of that
# run; the second archive holds `x/pwned` and meets a real symlink. Each binary
# plants its own link, so the first stage is under test as well as the second.
mkdir -p anclinksrc
ln -s ../escape anclinksrc/x
( cd anclinksrc && "$gnu_real" $GNUFMT -cf ../anclink.tar x )
prep_anc_twostep() {
  local which=gnu
  [ "$(basename "$PWD")" = od ] && which=ours
  env PATH="$bindir/$which" tar -xf ../anclink.tar >/dev/null 2>&1
}
PREP=prep_anc_twostep extract_case 'a symlink planted by an earlier archive' anc.tar

# The other end of a *hard link* member is a name in the archive too, and it is
# resolved beneath the destination on the same terms. Without that, an archive
# linking to `x/secret` through an escaping `x` hands the caller a second name
# for a file outside the tree — and write access to it.
mkdir -p anchsrc/x
printf 's\n' > anchsrc/x/secret
ln anchsrc/x/secret anchsrc/h
( cd anchsrc && "$gnu_real" $GNUFMT -cf ../anch.tar x/secret h )
prep_anc_hard() { ln -s ../escape x; printf 'loot\n' > ../escape/secret; }
PREP=prep_anc_hard extract_case 'a hard link target reached through an escape' anch.tar h
rm -f escape/secret

# Ancestors that are not stored in the archive. `tar -cf deep.tar a/b/c` stores
# the one member and neither `a/` nor `a/b/`, so extracting it is the case that
# forces the extractor to invent them.
mkdir -p deepsrc/a/b/c
printf 'hi\n' > deepsrc/a/b/c/d
printf 'hi\n' > deepsrc/a/b/cc
( cd deepsrc && "$gnu_real" $GNUFMT -cf ../deep.tar a/b/cc )
( cd deepsrc && "$gnu_real" $GNUFMT -cf ../deeper.tar a/b/c/d )
extract_case 'ancestors invented on demand' deep.tar

# And the failures, which are the reason the ancestor walk cannot just be
# `mkdir -p`. GNU keeps walking past an ancestor it could not make and reports
# the *last* one it tried, so an unwritable destination two levels short of the
# member reports `a/b: Cannot mkdir: No such file or directory` — not `a`, and
# not `Permission denied`. `chmod 555 .` is undone by the next case's `rm -rf`,
# which only needs the parent to be writable.
prep_unwritable() { chmod 555 .; }
PREP=prep_unwritable extract_case 'ancestors that cannot be made at all' deep.tar
PREP=prep_unwritable extract_case 'ancestors, three levels short' deeper.tar

# The other shape: the first ancestor is there but will not accept children, so
# the walk gets one level further before it fails.
prep_unwritable_a() { mkdir a; chmod 555 a; }
PREP=prep_unwritable_a extract_case 'an ancestor that rejects children' deep.tar
PREP=prep_unwritable_a extract_case 'the same, one level deeper' deeper.tar
chmod -R u+w od gd 2>/dev/null

# The tar-slip family. GNU strips a leading `/` with a warning and refuses
# `..`; ours must not write outside the destination either way. The comparison
# is of the resulting tree, so "refused" and "refused" agree even if the
# wording does not — the wording is checked by the stderr half of `settle`.
mkdir -p slip
printf 'evil\n' > slip/payload
# `-P` keeps the leading `/` in the stored name, which is the whole point: the
# extractor is what has to remove it. `$work` is already absolute, so it is
# spelled without a second slash — GNU reports the exact prefix it stripped
# (`Removing leading '//' ...` for a doubled one), and a fixture that made the
# path `//tmp/...` would be testing the harness's own typo.
# shellcheck disable=SC2086
"$gnu_real" $GNUFMT -cf abs.tar -P "$work/slip/payload" 2>/dev/null
extract_case 'a member with an absolute path' abs.tar

# The other half of the tar-slip family: a name that climbs out with `..`.
# There is no GNU flag that stores one, so the archive is forged by patching
# the name field of a normal one — 100 bytes at offset 0 of the first header,
# and the checksum recomputed by GNU's own reader being willing to read it.
python3 - "$work" <<'PY' 2>/dev/null || echo "note: no python3; the '..' case did not run" >&2
import sys, pathlib
w = pathlib.Path(sys.argv[1])
blocks = bytearray((w / "abs.tar").read_bytes())
name = b"../escaped.txt"
blocks[0:100] = name + b"\0" * (100 - len(name))
blocks[148:156] = b" " * 8
chk = sum(blocks[0:512])
blocks[148:156] = (b"%06o\0 " % chk)
(w / "dotdot.tar").write_bytes(bytes(blocks))
PY
if [ -f dotdot.tar ]; then
  extract_case 'a member that climbs out with ..' dotdot.tar
fi

# ===========================================================================
# 4b. headers no archiver will write for you
# ===========================================================================
# The cases that matter most to an extractor are exactly the ones a
# well-behaved archiver never produces, so they are built a block at a time.
# `escape/` is the directory the traversal cases aim at; it must still be empty
# afterwards, and that is checked separately from the tree comparison because
# both sides can agree on a tree while both are wrong.
mkdir -p escape
python3 - "$work" <<'PY' 2>/dev/null || echo "note: no python3; the forged-header cases did not run" >&2
import pathlib, sys
w = pathlib.Path(sys.argv[1])

def hdr(name, typeflag=ord('0'), mode=0o644, size=0, link=b'', major=None, minor=None):
    b = bytearray(512)
    def put(off, val): b[off:off + len(val)] = val
    put(0, name); put(100, b'%07o\0' % mode)
    put(108, b'0000000\0'); put(116, b'0000000\0')
    put(124, b'%011o\0' % size); put(136, b'%011o\0' % 1577934245)
    b[156] = typeflag; put(157, link)
    put(257, b'ustar\0'); put(263, b'00')
    if major is not None: put(329, b'%07o\0' % major)
    if minor is not None: put(337, b'%07o\0' % minor)
    # The checksum is computed over the block with the field itself blanked,
    # which is the format's own rule and the reason it can be filled in last.
    b[148:156] = b' ' * 8
    b[148:156] = b'%06o\0 ' % sum(b)
    return bytes(b)

def pad(data): return data + b'\0' * (-len(data) % 512)

esc = str(w / 'escape').encode()
cases = {
  # A char and a block device. As a non-root user both must fail, and the
  # question is whether they fail with the same words: `mknod` returns EPERM,
  # which ErrorKind folds in with EACCES.
  'dev':        hdr(b'zero', ord('3'), 0o666, major=1, minor=5)
                + hdr(b'loopy', ord('4'), 0o660, major=7, minor=0),
  # A hard link to a member that is not in the archive, one to an absolute
  # path, one that climbs out. GNU strips the last two and links what remains.
  'orphan':     hdr(b'orphan', ord('1'), link=b'nowhere'),
  'absl':       hdr(b'esc', ord('1'), link=b'/etc/passwd'),
  'uplink':     hdr(b'esc2', ord('1'), link=b'../outside'),
  # The prefix notice is once per *distinct* prefix, with separate sets for
  # names and for link targets. A repeat, and a repeat of a shorter one after a
  # longer one, must both stay silent.
  'prefixes':   hdr(b'a', ord('1'), link=b'/x') + hdr(b'b', ord('1'), link=b'//x')
                + hdr(b'c', ord('1'), link=b'/x') + hdr(b'd', ord('1'), link=b'../x')
                + hdr(b'e', ord('1'), link=b'/x') + hdr(b'g', ord('1'), link=b'a/../x'),
  # Names in every shape the stripping rule has to answer for, including the
  # two that strip to nothing and the one that is refused outright.
  'dotdot':     b''.join(hdr(n) for n in
                [b'../x', b'a/../b', b'./c', b'd/..', b'..', b'/a']),
  # A typeflag from no standard, with data. The reader must skip its blocks or
  # every following header is misaligned.
  'unknown':    hdr(b'weird', ord('Z'), size=5) + pad(b'ZZZZZ')
                + hdr(b'after', ord('0'), size=6) + pad(b'after\n'),
  # An empty symlink target, an empty hard link target, an empty *name*. GNU
  # substitutes `.` for the latter two and says so, every time.
  'emptysym':   hdr(b'sl', ord('2'), 0o777, link=b''),
  'emptytgt':   hdr(b'lnk', ord('1'), link=b''),
  'emptyname':  hdr(b'', ord('0')) + hdr(b'', ord('0')),
  # A directory member stored without the trailing slash that usually marks one.
  'dirnoslash': hdr(b'plain', ord('5'), 0o755),
  # The traversal itself, both ways round: a symlink out of the tree followed
  # by a member underneath it. If the link is created when it is read, the
  # member lands outside; GNU withholds it until the archive is finished, so
  # the member finds a mode-0 placeholder file and fails.
  'traverse':   hdr(b'd', ord('2'), 0o777, link=esc)
                + hdr(b'd/pwned', ord('0'), size=6) + pad(b'pwned\n'),
  # `..` counted from the destination, which is one level under the work
  # directory, so this names the same `escape/` the absolute one does.
  'traverse2':  hdr(b'd', ord('2'), 0o777, link=b'../escape')
                + hdr(b'd/pwned', ord('0'), size=6) + pad(b'pwned\n'),
}
for name, blocks in cases.items():
    (w / ('h-%s.tar' % name)).write_bytes(blocks + b'\0' * 1024)
PY

for c in dev orphan absl uplink prefixes dotdot unknown emptysym emptytgt \
         emptyname dirnoslash traverse traverse2; do
  [ -f "h-$c.tar" ] || continue
  extract_case "forged header: $c" "h-$c.tar"
  # `-tv` has to agree too. The prefix notices and the `link to` suffix come
  # out of the same stripping the extractor uses, so a listing that disagrees
  # with GNU means the two drivers have drifted apart from each other as well.
  list_case "forged header: $c" -tvf "h-$c.tar"
done

# The traversal cases aimed at `escape/`. A tree comparison cannot see this:
# both sides leave `d` a symlink either way, and the difference is whether
# anything was written through it.
if [ -d escape ]; then
  leaked=$(find escape -mindepth 1 2>/dev/null)
  if [ -z "$leaked" ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' 'traversal: nothing was written outside the destination'
  else
    fail=$((fail+1))
    printf 'DIFF %s\n  wrote outside the destination: %s\n' \
      'traversal: nothing was written outside the destination' "$(printf '%s' "$leaked" | tr '\n' ' ')"
  fi
fi

# ===========================================================================
# 5. verbose
# ===========================================================================
# Which *stream* the member list goes to is the question here, not just its
# text: `tar -cvf a.tar . > list` is a real idiom, and it only works if the
# names are on stdout.
create_case 'verbose create'  -v tree
plain_case  'verbose extract' -xvf ref.tar -C od

# ===========================================================================
# 6. diagnostics
# ===========================================================================
plain_case 'archive does not exist'      -tf nosuch.tar
plain_case 'archive does not exist (-x)' -xf nosuch.tar
plain_case 'archive is a directory'      -tf tree
plain_case 'archive cannot be created'   -cf /nonexistent-dir/x.tar tree
plain_case 'member does not exist'       -cf o.tar tree/nosuch
plain_case 'no mode given'
plain_case '-f with no argument'         -cf
plain_case '-C with no argument'         -xC

# ===========================================================================
# 7. the known divergences
# ===========================================================================
plain_xcase \
  "GNU's -Z is compression, which this tar does not implement; the message is a refusal either way, and the wording of a refusal for an option we do not have is not something to copy" \
  'unknown option' -cZf o.tar tree

plain_xcase \
  "GNU accepts long options; this tar has none, so --help is an operand to it. Adding them is a separate task -- see the getopt-ambiguity gate, which reports 'tar has no LONG_OPTIONS table'" \
  'a long option' --help

# `uname`/`gname` are left empty by ours, so an archive moved to a machine with
# different numeric ids restores to the wrong owner. Filling them needs a
# passwd lookup, which is why this is recorded rather than fixed in passing.
# `--numeric-owner` is dropped here so the case is exactly that difference.
rm -f o.tar gn.tar
diff_run env PATH="$bindir/ours" tar -cf o.tar tree >/dev/null 2>&1
"$gnu_real" --format=ustar --blocking-factor=1 -cf gn.tar tree >/dev/null 2>&1
if cmp -s o.tar gn.tar; then AGREED=yes; else AGREED=no; fi
REPORT="  (uname/gname)"
xreport 'create: uname/gname are filled in' \
  'ours leaves uname/gname empty; GNU fills them from the passwd database, so an archive restored on another machine gets the wrong owner name. Needs a passwd lookup in tar. known-issues.md -> B-tar'

printf '\ntar: %d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ' (%d of which no longer do)' "$xpass"
fi
printf '\n'
[ "$fail" -eq 0 ] && [ "$xpass" -eq 0 ]
