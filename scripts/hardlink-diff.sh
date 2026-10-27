#!/usr/bin/env bash
# Differential test: our `hardlink` against Ubuntu's, util-linux 2.39.3's.
#
# What hardlink does shows in three places: what it prints, its status, and
# the tree it leaves -- which names became links to which. The order it
# links in follows inode numbers and the order the directory lists its
# entries, so each case builds its tree afresh in a tmpfs of its own, in a
# user and mount namespace (`unshare -Urm`, no root), at the same path for
# both sides: a fresh tmpfs numbers its inodes from the same start and
# lists a directory in the same order, so both programs see the same tree.
# After the run every entry's type, inode, link count, size and mode is
# recorded and compared, and a file's mtime, which the setup fixes (a
# directory's moves with each link made in it, a wall-clock time).
#
# Masked: the `Duration:` line's time, and the device number `-vvv` prints
# (one tmpfs may be given another number than the last). Both programs are
# run by the name `hardlink`, found on PATH, so a message that names the
# program by argv[0] -- getopt's -- names it alike.
#
# A case `nocaps` runs the program without capabilities, so that a
# directory it may not read is one it cannot: as the namespace's root it
# could read anything.
set -u

DIFF_PROG='hardlink'
DIFF_PKG='hardlink'
DIFF_REF='/usr/bin/hardlink'
DIFF_NEED='timeout unshare mount setpriv python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

pass=0; fail=0; broken=0

# inner.sh BINDIR DIR OUT ERR STATE SETUP MODE ARGV...: one side of one
# case, in the namespace.
cat > "$DIFF_TMP/inner.sh" <<'EOF'
set -u
bindir=$1 dir=$2 out=$3 err=$4 state=$5 setup=$6 mode=$7; shift 7
mount -t tmpfs tmpfs "$dir" || exit 90
cd "$dir" || exit 91
# mk PATH CONTENT [EPOCH]: a file holding CONTENT, modified at EPOCH.
mk() {
  mkdir -p "$(dirname "$1")" && printf '%s' "$2" > "$1" && touch -d "@${3:-1700000000}" "$1"
}
# big PATH SIZE [OFFSET BYTE] [EPOCH]: SIZE bytes of `a`, BYTE at OFFSET.
big() {
  mkdir -p "$(dirname "$1")" && head -c "$2" /dev/zero | tr '\0' a > "$1" || return 1
  if [ -n "${3:-}" ]; then printf '%s' "$4" | dd of="$1" bs=1 seek="$3" conv=notrunc status=none; fi
  touch -d "@${5:-1700000000}" "$1"
}
# xa PATH NAME VALUE: an extended attribute.
xa() { python3 -c 'import os,sys; os.setxattr(sys.argv[1], sys.argv[2], sys.argv[3].encode())' "$@"; }
eval "$setup" || exit 92
export PATH="$bindir:$PATH"
run() { timeout -k 1 20 hardlink "$@"; }
case $mode in
  plain) run "$@" > "$out" 2> "$err" ;;
  full) run "$@" > /dev/full 2> "$err" ;;
  nocaps) setpriv --bounding-set -all --inh-caps -all timeout -k 1 20 hardlink "$@" > "$out" 2> "$err" ;;
esac
rc=$?
python3 "$(dirname "$0")/state.py" > "$state"
exit "$rc"
EOF

# state.py: every entry below the current directory, a line each in byte
# order -- its path, type, inode, link count, size and mode, and a regular
# file's mtime. Each is read relative to its directory's descriptor, so an
# entry past PATH_MAX is recorded like any other.
cat > "$DIFF_TMP/state.py" <<'EOF'
import os, stat, sys
lines = []
for top, dirs, files, fd in os.fwalk(b".", follow_symlinks=False):
    for name in dirs + files:
        st = os.stat(name, dir_fd=fd, follow_symlinks=False)
        line = b"%s %o %d %d %d %o" % (top + b"/" + name, stat.S_IFMT(st.st_mode), st.st_ino,
                                       st.st_nlink, st.st_size, stat.S_IMODE(st.st_mode))
        if stat.S_ISREG(st.st_mode):
            line += b" %d" % int(st.st_mtime)
        lines.append(line + b"\n")
sys.stdout.buffer.write(b"".join(sorted(lines)))
EOF

# side SIDE SETUP MODE ARGV...: one side's out, err, state and status.
side() {
  local s=$1 setup=$2 mode=$3 o=$DIFF_TMP/$1; shift 3
  diff_run env -i PATH=/usr/bin:/bin LC_ALL=C.UTF-8 timeout -k 1 40 \
    unshare -Urm bash "$DIFF_TMP/inner.sh" "$bindir/$s" "$DIFF_TMP/root" \
    "$o.out" "$o.err" "$o.state" "$setup" "$mode" "$@"
}

mask() {
  sed -E -i -e 's/^(Duration: +)[0-9]+\.[0-9]{6} seconds$/\1T seconds/' \
    -e 's/^( +[0-9]+: \[)[0-9]+\//\1D\//' "$@"
}

mkdir -p "$DIFF_TMP/root"

# case_ SETUP MODE ARGV...
case_() {
  local setup=$1 mode=$2 o_rc g_rc label; shift 2
  rm -f "$DIFF_TMP"/ours.* "$DIFF_TMP"/gnu.*
  side ours "$setup" "$mode" "$@"; o_rc=$?
  side gnu "$setup" "$mode" "$@"; g_rc=$?
  # inner.sh writes the state last, after the program: a side without it
  # never ran -- `unshare` refused, or the tmpfs or the setup failed (90-92)
  # -- and compared nothing.
  local ran=yes
  [ -s "$DIFF_TMP/ours.state" ] && [ -s "$DIFF_TMP/gnu.state" ] || ran=no
  for s in ours gnu; do for f in out err state; do [ -f "$DIFF_TMP/$s.$f" ] || : > "$DIFF_TMP/$s.$f"; done; done
  mask "$DIFF_TMP/ours.out" "$DIFF_TMP/gnu.out"
  label="[${CASE_NAME:-?} $mode] hardlink $(printf '%q ' "$@")"
  if [ "$ran" = no ] || [ "$g_rc" = 126 ] || [ "$g_rc" = 127 ] \
     || [ "$o_rc" = 126 ] || [ "$o_rc" = 127 ] || [ "$o_rc" = 124 ] || [ "$g_rc" = 124 ]; then
    broken=$((broken + 1)); printf 'BROKEN %s (rc %s/%s)\n' "$label" "$o_rc" "$g_rc"
  elif cmp -s "$DIFF_TMP/ours.out" "$DIFF_TMP/gnu.out" && cmp -s "$DIFF_TMP/ours.err" "$DIFF_TMP/gnu.err" \
     && cmp -s "$DIFF_TMP/ours.state" "$DIFF_TMP/gnu.state" && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1)); [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$label"
  else
    fail=$((fail + 1))
    printf 'DIFF %s (rc %s/%s)\n' "$label" "$o_rc" "$g_rc"
    for f in out err state; do
      cmp -s "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" ||
        diff -u --label "ours.$f" --label "gnu.$f" "$DIFF_TMP/ours.$f" "$DIFF_TMP/gnu.$f" | head -40
    done
    if [ -n "${DIFF_FAIL_FAST:-}" ]; then exit 1; fi
  fi
  return 0
}

# Each setup: shell run in the fresh tmpfs, with mk, big and xa.
S_PAIR='mk a/x hello; mk a/y hello'
S_MIX='mk a/1 abc; mk a/2 abc; mk a/3 abd; mk a/4 abc; mk b/1 abc; mk a/e ""; mk a/e2 ""
  mk a/f "longer content here"; mk b/f "longer content here"; mk b/g "longer content herE"
  ln -s 1 a/l; mkfifo a/p; mkdir -p a/deep/er; mk a/deep/er/z abc'
S_TIMES='mk a/x hello 1000; mk a/y hello 2000; mk a/z hello 2000; mk a/w hello 500'
S_MODES='mk a/x hello; mk a/y hello; mk a/z hello; chmod 600 a/y; chmod 755 a/z'
S_NLINK='mk a/x hello; ln a/x a/x2; mk a/y hello; mk a/z hello; ln a/z a/z2; ln a/z a/z3; mk a/v hello'
S_NAMES='mk d1/f hello; mk d2/f hello; mk d1/g hello; mk d2/h hello; mk d3/f hello'
S_DIRS='mk r1/sub/f hello; mk r2/sub/f hello; mk r1/other/f hello; mk r2/f hello; mk r1/f hello'
S_BIG='big a/b1 100000 99999 X; big a/b2 100000 99999 X; big a/b3 100000 99999 Y
  big a/b4 100000 10 Z; big a/b5 100000 40000 Q; big a/b6 100000 99999 X; big a/s1 40; big a/s2 40 39 B'
S_SIZES='mk a/1 1; mk a/1b 1; mk a/2 22; mk a/2b 22; mk a/5 55555; mk a/5b 55555
  mk a/10 0123456789; mk a/10b 0123456789; mk a/e ""; mk a/e2 ""'
S_REGEX='mk a/keep.txt hello; mk a/keep2.txt hello; mk a/skip.log hello; mk a/skip2.log hello
  mk a/sub.txt/inner hello; mk a/other hello'
S_XATTR='mk a/x hello; mk a/y hello; mk a/z hello; mk a/w hello; mk a/v hello; mk a/u hello
  xa a/x user.k 1; xa a/y user.k 1; xa a/z user.k 2; xa a/w user.j 1
  xa a/v user.a 1; xa a/v user.b 2; xa a/u user.b 2; xa a/u user.a 1'
S_MANY='for i in $(seq 1 30); do mk m/f$i "content $((i % 4))"; done
  for i in $(seq 1 12); do mk n/g$i "other $((i % 3)) longer"; done
  for i in $(seq 1 9); do mk m/h$i "$((i % 2))"; done'
S_DENIED='mk a/x hello; mk a/y hello; mk a/locked/f hello; chmod 000 a/locked
  mk a/noexec/f hello; chmod 444 a/noexec; mk a/unread hello; chmod 000 a/unread'
# b is a bind mount of a, the same files by a second name, across which no
# link can be made; c is a tmpfs of its own, another device.
S_BIND='mk a/x hello; mk a/y hello; mk a/sub/z hello; mkdir b && mount --bind a b
  mkdir c && mount -t tmpfs tmpfs c && mk c/w hello'
# Files past PATH_MAX: a/ then sixteen directories of 250 bytes' name is
# 4018 bytes, g in the last; f is in a seventeenth, at 4271. (Bash cannot
# `cd` past PATH_MAX, so the last level is made from its parent.)
S_LONG='mk a/x hello; n=$(printf "d%.0s" $(seq 1 250))
  ( cd a && for _ in $(seq 1 16); do mkdir "$n" && cd "$n" || exit 1; done && mkdir "$n" \
    && printf hello > g && printf hello > "$n/f" && touch -d @1700000000 g "$n/f" )'

CASE_NAME=pair
for args in '' '-n' '-v' '-vv' '-vvv' '-q' '-c' '-n -v' '-y memcmp -v' '-y sha1 -v' '-y crc32 -v' \
            '-y sha256 -v' '-y md5' '-y md5 -v' '-y SHA256 -vv' '-b 1 -v' '-b 1 -y memcmp -v' '-r 0 -v' \
            '--reflink=always -v' '--reflink -v' '--reflink=never -v' '--skip-reflinks -vv' \
            '--reflink=auto -vvv' '-X -v' '-m -v' '-M -v' '-O -v' '-o -t -p -v' '-f -v' '-d -v' \
            '--content --dry-run --verbose' '--io-size=4K --cache-size=1M -v'; do
  # shellcheck disable=SC2086
  case_ "$S_PAIR" plain $args a
done
case_ "$S_PAIR" plain -vv a/x a/y
case_ "$S_PAIR" plain -vv a a
case_ "$S_PAIR" plain -vv a/x a/x
case_ "$S_PAIR" plain -vv a a/x .
case_ "$S_PAIR" plain -v a/x
case_ "$S_PAIR" plain -v a nonexistent a/y
case_ "$S_PAIR" plain -v -- a
case_ "$S_PAIR" full -v a
case_ "$S_PAIR" full -q a

CASE_NAME=mix
for args in '' '-v' '-vv' '-vvv' '-n -vvv' '-s 0 -vv' '-c -v' '-S 3 -vv' '-y memcmp -b 2 -vv' '-f -vv'; do
  # shellcheck disable=SC2086
  case_ "$S_MIX" plain $args a b
done
case_ "$S_MIX" plain -vv b a
case_ "$S_MIX" plain -vv .

CASE_NAME='times'
for args in '-v' '-vv' '-t -v' '-t -O -v' '-O -v' '-t -m -v' '-t -M -v' '-t -vvv'; do
  # shellcheck disable=SC2086
  case_ "$S_TIMES" plain $args a
done

CASE_NAME=modes
for args in '-v' '-vv' '-p -v' '-p -n -v' '-c -v'; do
  # shellcheck disable=SC2086
  case_ "$S_MODES" plain $args a
done

CASE_NAME=nlink
for args in '-v' '-vvv' '-m -v' '-M -v' '-m -M -v' '-O -v' '-n -m -vv'; do
  # shellcheck disable=SC2086
  case_ "$S_NLINK" plain $args a
done

CASE_NAME=names
for args in '-v' '-f -v' '-f -vvv' '-f -n -v'; do
  # shellcheck disable=SC2086
  case_ "$S_NAMES" plain $args d1 d2 d3
done

CASE_NAME=dirs
for args in '-v' '-d -v' '-d -vvv' '-d -f -v'; do
  # shellcheck disable=SC2086
  case_ "$S_DIRS" plain $args r1 r2
done
case_ "$S_DIRS" plain -d -v r1/f r2/f
case_ "$S_DIRS" plain -d -v r1 r2/f

CASE_NAME=big
for args in '-v' '-vv' '-y memcmp -vv' '-y memcmp -b 16 -vv' '-y sha1 -b 1000 -vv' '-y crc32 -b 7 -vv' \
            '-b 64K -r 1 -vv' '-y sha256 -b 1 -r 64 -vv' '-y memcmp -b 99999 -vv'; do
  # shellcheck disable=SC2086
  case_ "$S_BIG" plain $args a
done

CASE_NAME=sizes
for args in '-v' '-vv' '-s 3 -vv' '-s 2 -S 10 -vv' '-S 1 -vv' '-s 0 -vv' '-s 1K -v' '-S 1K -vv' \
            '--minimum-size=0x10 -vv' '--maximum-size 0 -vv'; do
  # shellcheck disable=SC2086
  case_ "$S_SIZES" plain $args a
done

CASE_NAME=regex
for args in '-v' "-i txt -v" "-x log -v" "-i txt -x keep2 -v" "-x txt -i keep -v" \
            "-i ^/ -v" "-x . -v" "-x . -i other -vv" "-i (keep|skip)2 -v" "-x sub -v" \
            "--include=nomatch -vv" "--exclude=nomatch -vv"; do
  # shellcheck disable=SC2086
  case_ "$S_REGEX" plain $args a
done

CASE_NAME=xattr
for args in '-v' '-X -v' '-X -vv' '-X -c -v' '-X -n -vv'; do
  # shellcheck disable=SC2086
  case_ "$S_XATTR" plain $args a
done

CASE_NAME=many
for args in '' '-v' '-vvv' '-y memcmp -v' '-O -v' '-n -v' '-s 2 -v'; do
  # shellcheck disable=SC2086
  case_ "$S_MANY" plain $args m n
done
case_ "$S_MANY" plain -v n m
case_ "$S_MANY" plain -v .

CASE_NAME=denied
case_ "$S_DENIED" nocaps -v a
case_ "$S_DENIED" nocaps -vvv a
case_ "$S_DENIED" nocaps -v a/locked a/x
case_ "$S_DENIED" plain -v a

CASE_NAME=bind
for args in '-v' '-vvv' '-n -vv' '-c -v'; do
  # shellcheck disable=SC2086
  case_ "$S_BIND" plain $args a b c
done
case_ "$S_BIND" plain -vv .
case_ "$S_BIND" plain -vv b a

CASE_NAME=long
for args in '-v' '-vvv' '-n -v' '-y memcmp -v'; do
  # shellcheck disable=SC2086
  case_ "$S_LONG" plain $args a
done
case_ "$S_LONG" plain -v .

CASE_NAME=usage
for args in '--version' '-V' '--help' '-h' '' '-q -v a' '-v -q a' '--quiet --verbose a' '--reflink=bad a' \
            '--reflink= a' '-s 1x a' '-S -1 a' '-r bad a' '-b "" a' "-i '(' a" '-x [ a' '-i a{1 a' \
            '--bogus' '-Z a' '--refl a' '--ref a' '-y' '-h -q -v' '-V --bogus' '--respect' '-nvq a'; do
  eval "set -- $args"
  case_ "$S_PAIR" plain "$@"
done

# An --exclude pattern too costly to decide -- a backreference searched for
# along a path past PATH_MAX -- ours only: glibc's regexec decides it however
# long that takes. Its warning comes once for the pattern, not once a file.
CASE_NAME=costly
rm -f "$DIFF_TMP"/ours.*
side ours "$S_LONG" plain -v -x '(.*)(.*)(.*)\1y' a
n=$(grep -c 'too costly to match' "$DIFF_TMP/ours.err" || true)
if [ "$n" = 1 ]; then
  pass=$((pass + 1))
else
  fail=$((fail + 1))
  printf 'DIFF [costly] %s warnings for one pattern, not 1\n' "$n"
  head -5 "$DIFF_TMP/ours.err"
fi

printf '\nhardlink-diff: %d passed, %d differed, %d broken\n' "$pass" "$fail" "$broken"
[ "$fail" = 0 ] && [ "$broken" = 0 ]
