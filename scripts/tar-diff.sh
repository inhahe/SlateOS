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
DIFF_NEED='find stat cmp od sha256sum touch'
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
manifest() {
  ( cd "$1" 2>/dev/null || return 0
    find . | LC_ALL=C sort | while IFS= read -r p; do
      if [ -d "$p" ]; then
        printf 'd %s %s %s\n' "$p" "$(stat -c '%a %Y' "$p")" -
      elif [ -f "$p" ]; then
        printf 'f %s %s %s\n' "$p" "$(stat -c '%a %Y %s' "$p")" \
          "$(sha256sum <"$p" | cut -c1-16)"
      else
        printf '? %s %s -\n' "$p" "$(stat -c '%a %Y' "$p")"
      fi
    done )
}

# extract_case LABEL ARCHIVE [EXTRA-ARGS...]
extract_case() {
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
