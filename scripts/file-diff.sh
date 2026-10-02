#!/usr/bin/env bash
# Differential test: our `file` against file 5.45, built from the release.
#
# `userspace/file` is a port of file 5.45 and libmagic, function by function,
# and keeps upstream's behaviour where it is odd -- so the test is the output,
# byte for byte, of the two programs on the same inputs:
#
#   1. upstream's own test files, in seven modes (-b, -i, --mime-type,
#      --mime-encoding, --extension, --apple, -k);
#   2. a sample of this machine's files: binaries, libraries, fonts, images,
#      documentation;
#   3. generated ELF files (scripts/file-gen-elf.py): every note, core file,
#      capability and limit readelf.c reads;
#   4. generated Composite Document Files (scripts/file-gen-cdf.py), and
#      mutations of them;
#   5. generated compressed files (scripts/file-gen-z.py) under -z and -Z:
#      zlib's inflate, its messages, and the external decompressors;
#   6. random mutations of the files in 1 and 2;
#   7. a sample of all of the above through a pipe and a redirect;
#   8. the database: `-l`, and `-C` of the vendored magic directory, whose
#      compiled bytes must match upstream's;
#   9. names that are not ordinary files, and options that are wrong.
#
# The reference is file 5.45 built from the release tarball (verified by its
# SHA-256) with zlib, cached in ~/.cache/slateos-diff-file: a distribution's
# `file` carries patches, and its magic database more. It runs with the
# database it built (`-m`). Ours runs with the database built into it, which
# it reads only when no installed one is at the default path -- so ours runs
# in a mount namespace where /usr/share/misc is empty.
#
# Two differences are deliberate and are not tested here: `-v` names the
# built-in database, and `-S` is accepted (SlateOS has no seccomp sandbox to
# turn off); see the head of userspace/file/src/main.rs.
#
#   bash scripts/file-diff.sh          # SHOW=N to print N differences a part
set -u

DIFF_PROG='file'
DIFF_PKG='file'
DIFF_NEED='python3 curl sha256sum make gcc unshare tar'
# The reference is built below, after the preamble: the preamble re-execs this
# file inside WSL, and nothing may run before it.
DIFF_NO_REF=1
DIFF_NO_BINDIR=1
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

here=$(cd "$(dirname "$0")" && pwd)
SHOW=${SHOW:-10}

FILE_VERSION='5.45'
FILE_SHA256='fc97f51029bb0e2c9f4e3bffefdaf678f0e039ee872b9de5c002a6d09c784d82'
ref_cache=$HOME/.cache/slateos-diff-file
ref_dir=$ref_cache/file-$FILE_VERSION
if [ ! -x "$ref_dir/src/file" ] || [ ! -f "$ref_dir/magic/magic.mgc" ]; then
  mkdir -p "$ref_cache" || exit 1
  tarball=$ref_cache/file-$FILE_VERSION.tar.gz
  if [ ! -f "$tarball" ]; then
    curl -fsSL -o "$tarball.part" "https://astron.com/pub/file/file-$FILE_VERSION.tar.gz" &&
      mv "$tarball.part" "$tarball" ||
      { echo "file-diff: cannot fetch file $FILE_VERSION" >&2; exit 1; }
  fi
  if ! echo "$FILE_SHA256  $tarball" | sha256sum -c --quiet - >/dev/null 2>&1; then
    echo "file-diff: $tarball does not have the release's SHA-256; removed" >&2
    rm -f "$tarball"
    exit 1
  fi
  rm -rf "$ref_dir"
  (cd "$ref_cache" && tar xzf "$tarball" && cd "file-$FILE_VERSION" &&
    ./configure --disable-shared --enable-static >/dev/null &&
    make -j"$(nproc)" >/dev/null 2>&1) ||
    { echo "file-diff: cannot build file $FILE_VERSION in $ref_dir" >&2; exit 1; }
fi
# Without zlib the reference hands gzip to an external `gzip`, as ours never
# does, and every -z case would differ for that reason alone.
if ! grep -q '^#define ZLIBSUPPORT 1' "$ref_dir/config.h"; then
  echo "file-diff: the reference was built without zlib: install zlib1g-dev and remove $ref_dir" >&2
  exit 1
fi
REF=$ref_dir/src/file
MGC=$ref_dir/magic/magic.mgc

# Ours, with no installed database to find. `unshare -r` needs user
# namespaces, which WSL has; a machine without a database there needs none.
if [ -e /usr/share/misc/magic ] || [ -e /usr/share/misc/magic.mgc ]; then
  if ! unshare -rm true 2>/dev/null; then
    echo "file-diff: /usr/share/misc holds a magic database and unshare -rm is refused" >&2
    exit 1
  fi
  ours() { unshare -rm sh -c 'mount -t tmpfs none /usr/share/misc && exec "$0" "$@"' "$OURS" "$@"; }
else
  ours() { "$OURS" "$@"; }
fi
ref() { "$REF" -m "$MGC" "$@"; }

pass=0; fail=0
w=$DIFF_TMP

# Both sides on every name in LIST, in each of the modes after it, as one
# `-f LIST` run a side; each line that differs is a failure.
bulk() {
  local what=$1 list=$2; shift 2
  local m n d
  for m in "$@"; do
    # shellcheck disable=SC2086  # a mode is several words, on purpose
    ref $m -f "$list" >"$w/ref.txt" 2>&1
    # shellcheck disable=SC2086
    ours $m -f "$list" >"$w/ours.txt" 2>&1
    n=$(wc -l <"$w/ref.txt")
    d=$(diff "$w/ref.txt" "$w/ours.txt" | grep -c '^<')
    if [ "$d" -eq 0 ] && [ "$n" -eq "$(wc -l <"$w/ours.txt")" ]; then
      pass=$((pass + n))
      [ -n "${VERBOSE:-}" ] && echo "ok   $what [$m]: $n"
    else
      [ "$d" -eq 0 ] && d=1
      fail=$((fail + d)); pass=$((pass + n - d))
      echo "FAIL $what [$m]: $d of $n differ"
      diff "$w/ref.txt" "$w/ours.txt" | head -"$SHOW"
    fi
  done
}

# One case: the same arguments to both, stdout, stderr and status compared.
one() {
  local what=$1; shift
  local r o
  r=$(ref "$@" 2>&1; echo "rc=$?")
  o=$(ours "$@" 2>&1; echo "rc=$?")
  if [ "$r" = "$o" ]; then
    pass=$((pass + 1))
  else
    fail=$((fail + 1))
    printf 'FAIL %s\n  ref:  %s\n  ours: %s\n' "$what" "$r" "$o"
  fi
}

# One file through standard input, as a pipe and as a redirect.
stdin_case() {
  local f=$1; shift
  local r o
  r=$(ref "$@" - <"$f" 2>&1); o=$(ours "$@" - <"$f" 2>&1)
  if [ "$r" = "$o" ]; then pass=$((pass + 1)); else
    fail=$((fail + 1)); printf 'FAIL redirect %s [%s]\n  ref:  %s\n  ours: %s\n' "$f" "$*" "$r" "$o"; fi
  r=$(ref "$@" - < <(cat "$f") 2>&1); o=$(ours "$@" - < <(cat "$f") 2>&1)
  if [ "$r" = "$o" ]; then pass=$((pass + 1)); else
    fail=$((fail + 1)); printf 'FAIL pipe %s [%s]\n  ref:  %s\n  ours: %s\n' "$f" "$*" "$r" "$o"; fi
}

# 1. Upstream's test files.
ls -d "$ref_dir"/tests/*.testfile >"$w/tests.txt"
bulk tests "$w/tests.txt" "-b" "-bi" "-b --mime-type" "-b --mime-encoding" "-b --extension" "-b --apple" "-bk"

# 2. This machine's files: every 7th, sorted, so the sample is stable here.
find /usr/bin /usr/sbin /usr/lib/x86_64-linux-gnu /usr/share/fonts /usr/share/pixmaps \
  /usr/share/icons /usr/share/doc /usr/share/man /usr/share/zoneinfo /etc \
  -maxdepth 3 \( -type f -o -type l \) -readable 2>/dev/null |
  sort | awk 'NR % 7 == 0' | head -2500 >"$w/system.txt"
bulk system "$w/system.txt" "" "-i" "-k" "-L" "-z"

# 3. ELF.
python3 "$here/file-gen-elf.py" "$w/elf" >/dev/null || exit 1
ls -d "$w"/elf/* >"$w/elf.txt"
bulk elf "$w/elf.txt" "" "-i" "-k" "-e cdf"

# 4. Composite Document Files.
python3 "$here/file-gen-cdf.py" "$w/cdf" 1 400 >/dev/null || exit 1
ls -d "$w"/cdf/* >"$w/cdf.txt"
bulk cdf "$w/cdf.txt" "" "-i" "--mime-type" "-k"

# 5. Compressed files.
python3 "$here/file-gen-z.py" "$w/z" 3 >/dev/null || exit 1
ls -d "$w"/z/* >"$w/z.txt"
bulk compressed "$w/z.txt" "-z" "-zi" "-Z" "-z --mime-type" "-zk" "-z -P bytes=500"

# 6. Mutations of 1 and 2.
mkdir -p "$w/mut"
cat "$w/tests.txt" "$w/system.txt" | python3 -c '
import os, random, sys
out = sys.argv[1]
R = random.Random(11)
datas = []
for name in sys.stdin.read().split("\n"):
    try:
        if name and os.path.isfile(name):
            d = open(name, "rb").read(200000)
            if d:
                datas.append(d)
    except OSError:
        pass
for i in range(2500):
    d = bytearray(R.choice(datas))
    lim = min(len(d), R.choice([16, 64, 512, 4096, len(d)]))
    for _ in range(R.choice([1, 1, 2, 3, 5, 10])):
        p = R.randrange(0, lim)
        r = R.random()
        if r < 0.4:
            d[p] = R.randrange(256)
        elif r < 0.6:
            d[p] ^= 1 << R.randrange(8)
        elif r < 0.8:
            d[p] = R.choice([0, 0xff, 0x7f, 0x80, 0x20, 0x0a])
        else:
            n = R.choice([2, 4, 8])
            if p + n <= len(d):
                d[p:p + n] = R.choice([b"\xff" * n, b"\0" * n, bytes(R.randrange(256) for _ in range(n))])
    if R.random() < 0.15:
        d = d[:R.randrange(1, len(d) + 1)]
    with open(os.path.join(out, "m%05d" % i), "wb") as f:
        f.write(d)
' "$w/mut" || exit 1
ls -d "$w"/mut/* >"$w/mut.txt"
bulk mutations "$w/mut.txt" "" "-i" "-k" "--extension"

# 7. Standard input.
cat "$w/elf.txt" "$w/cdf.txt" "$w/z.txt" "$w/tests.txt" | awk 'NR % 23 == 0' >"$w/stdin.txt"
while IFS= read -r f; do
  stdin_case "$f"
  stdin_case "$f" -z
done <"$w/stdin.txt"

# 8. The database: its listing, and what compiling it writes.
r=$(ref -l 2>&1); o=$(ours -l 2>&1)
if [ "$r" = "$o" ]; then pass=$((pass + 1)); else
  fail=$((fail + 1)); echo "FAIL -l"; diff <(echo "$r") <(echo "$o") | head -"$SHOW"; fi
mkdir -p "$w/cref" "$w/cours"
(cd "$w/cref" && "$REF" -C -m "$root/userspace/file/magic" >/dev/null 2>&1)
(cd "$w/cours" && "$OURS" -C -m "$root/userspace/file/magic" >/dev/null 2>&1)
if cmp -s "$w/cref/magic.mgc" "$w/cours/magic.mgc"; then pass=$((pass + 1)); else
  fail=$((fail + 1)); echo "FAIL -C: the compiled databases differ"; fi

# 9. Names that are not ordinary files, and options that are wrong.
mkdir -p "$w/odd/dir"
: >"$w/odd/empty"
printf 'secret\n' >"$w/odd/unreadable"; chmod 000 "$w/odd/unreadable"
ln -s nowhere "$w/odd/dangling"
ln -s loop "$w/odd/loop"
mkfifo "$w/odd/fifo"
for args in "$w/odd/missing" "$w/odd/dir" "$w/odd/empty" "$w/odd/unreadable" \
  "$w/odd/dangling" "-L $w/odd/dangling" "$w/odd/loop" "-L $w/odd/loop" \
  "$w/odd/fifo" "/dev/null" "-s /dev/null" "/dev/zero" "-s /dev/zero" \
  "-E $w/odd/missing" "-0 $w/odd/empty $w/odd/dir" "-N $w/odd/dir" \
  "-F :: $w/odd/empty" "-p $w/odd/empty" "-r $w/odd/empty" "-n $w/odd/empty" \
  "-Q" "-P bogus=1" "-P indir" "-e bogus" "-m" "--mime-type --brief" \
  "--help" "-" "-f $w/odd/missing" "--exclude-quiet=bogus $w/odd/empty"; do
  # shellcheck disable=SC2086  # each is several words, on purpose
  one "$args" $args </dev/null
done
chmod 600 "$w/odd/unreadable"

echo "file-diff: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
