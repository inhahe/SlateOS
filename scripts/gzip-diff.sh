#!/usr/bin/env bash
# Differential test: our `gzip` (and `gunzip`, `zcat`, `uncompress`) against
# Ubuntu 24.04's -- gzip 1.12-1ubuntu3.2 and its scripts.
#
# ## What a case compares
#
# Everything the program can change: standard output **byte for byte** --
# compressed data included, which is the point: the port is held to produce
# the same `.gz` files, not merely files that decompress to the same bytes --
# standard error, the exit status, and the working directory afterwards:
# every name in it, its type, mode, size, modification time to the
# nanosecond, and its contents' hash. So `gzip f` is checked for removing `f`,
# for the `f.gz` it leaves, and for the mode and time it gives it.
#
# Each side runs in its own fresh copy of the fixtures, under `env -i` with
# `LC_ALL=C.UTF-8` and `TZ=UTC`, and reached through one directory,
# `$DIFF_TMP/cur`, re-pointed at the side being run. That last is for the
# scripts: Ubuntu's `gunzip --help` prints its own path (`$0`), and both
# sides must be found at the same one.
#
# ## The fixtures
#
# Made by `gen.py` below. Plain inputs, and every kind of input gzip reads:
# gzip members with each header flag, crafted (`FEXTRA`, `FNAME`,
# `FCOMMENT`, `FHCRC`, reserved bits, encryption, an unknown method);
# several members in one file; trailing garbage and trailing zeros; damage
# (a bad CRC, a bad length, truncation); pkzip files made by `zip`; `pack`
# files (upstream's own test samples); and `compress` (`.Z`) files from a
# small encoder here -- WSL has no `compress` -- each checked, as it is
# made, to decode with the reference gzip. Upstream's `-lh5-` LZH has no
# encoder anywhere to hand; it is reached through the random cases.
#
# ## Random and mutated streams
#
# The last section feeds each decoder seeded random bytes behind its magic
# number, and valid files with bytes changed, and compares what each side
# makes of them -- partial output, diagnostics, status. The decoders are
# where a port can differ invisibly to a test that only uses good files.
#
# ## Deliberate differences
#
# `userspace/gzip/src/lib.rs` lists them. The one this harness meets is the
# first: a name with an unprintable byte is escaped in a diagnostic.
set -u

DIFF_PROG='gzip'
DIFF_PKG='gzip'
DIFF_NEED='python3 zip timeout'
# Some 900 cases, each on two fresh copies of the fixtures.
DIFF_TIMEOUT=${DIFF_TIMEOUT:-3600}
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

for name in gunzip zcat uncompress; do
  if [ -n "$OURS" ] && [ "$OURS" != "$(command -v gzip)" ]; then
    ln -s "$OURS" "$bindir/ours/$name" || exit 1
  else
    ln -s "/usr/bin/$name" "$bindir/ours/$name" || exit 1
  fi
  ln -s "/usr/bin/$name" "$bindir/gnu/$name" || exit 1
done
cur=$DIFF_TMP/cur

pass=0; fail=0; xfail=0; xpass=0

fixtures=$DIFF_TMP/fixtures
mkdir -p "$fixtures"
gen=$DIFF_TMP/gen.py
cat > "$gen" <<'PY'
"""Make the fixtures: python3 gen.py DIR. Every file gets a fixed time."""
import os, random, struct, subprocess, sys, zlib

d = sys.argv[1]
T = 981173106  # 2001-02-03 04:05:06 UTC
NS = 123456789

def put(name, data, mtime=T, ns=NS, mode=0o644):
    p = os.path.join(d, name)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "wb") as f:
        f.write(data)
    os.chmod(p, mode)
    os.utime(p, ns=(mtime * 10**9 + ns, mtime * 10**9 + ns))

def gzip_ref(data, *opts):
    return subprocess.run(["/usr/bin/gzip", "-c", *opts], input=data,
                          stdout=subprocess.PIPE, check=True).stdout

def member(data, flags=0, mtime=T, xfl=0, os_=3, extra=None, name=None,
           comment=None, hcrc=False, method=8, crc=None, size=None,
           body=None):
    if hcrc:
        flags |= 2
    if extra is not None:
        flags |= 4
    if name is not None:
        flags |= 8
    if comment is not None:
        flags |= 16
    h = bytes([0x1f, 0x8b, method, flags]) + struct.pack("<I", mtime) + bytes([xfl, os_])
    if extra is not None:
        h += struct.pack("<H", len(extra)) + extra
    if name is not None:
        h += name + b"\0"
    if comment is not None:
        h += comment + b"\0"
    if hcrc:
        h += struct.pack("<H", zlib.crc32(h) & 0xffff)
    if body is None:
        c = zlib.compressobj(9, zlib.DEFLATED, -15)
        body = c.compress(data) + c.flush()
    t = struct.pack("<II", zlib.crc32(data) if crc is None else crc,
                    len(data) if size is None else size)
    return h + body + t

def lzw(data, maxbits=16, block=True):
    """compress(1)'s LZW, as ncompress writes it, without CLEAR codes."""
    out = bytearray([0x1f, 0x9d, maxbits | (0x80 if block else 0)])
    bits = 0
    acc = 0
    nacc = 0
    group_start = 0
    n_bits = 9
    maxmaxcode = 1 << maxbits
    maxcode = (1 << n_bits) - 1
    free_ent = 257 if block else 256
    table = {bytes([i]): i for i in range(256)}
    buf = bytearray()

    def emit(code):
        nonlocal acc, nacc, bits
        acc |= code << nacc
        nacc += n_bits
        bits += n_bits
        while nacc >= 8:
            buf.append(acc & 0xff)
            acc >>= 8
            nacc -= 8

    def pad():
        nonlocal acc, nacc, bits, group_start
        group = n_bits * 8
        used = (bits - group_start) % group
        if used:
            emit_zero = group - used
            while emit_zero:
                take = min(emit_zero, 8)
                nacc += take
                bits += take
                emit_zero -= take
                while nacc >= 8:
                    buf.append(acc & 0xff)
                    acc >>= 8
                    nacc -= 8
        group_start = bits

    w = b""
    for b in data:
        wc = w + bytes([b])
        if wc in table:
            w = wc
            continue
        emit(table[w])
        # No `n_bits < maxbits` test: with `-b 9` the table fills at 512 and
        # the codes widen to 10 bits anyway, in compress and in gzip alike.
        if free_ent > maxcode:
            pad()
            n_bits += 1
            maxcode = maxmaxcode if n_bits == maxbits else (1 << n_bits) - 1
        if free_ent < maxmaxcode:
            table[wc] = free_ent
            free_ent += 1
        w = bytes([b])
    if w:
        emit(table[w])
    if nacc:
        buf.append(acc & 0xff)
    return bytes(out + buf)

rng = random.Random(20261016)
text = b"hello, world\n" * 3
fox = (b"The quick brown fox jumps over the lazy dog.\n" * 7000)[:300000]
seq = "".join("%d\n" % i for i in range(1, 30001)).encode()
rand = bytes(rng.getrandbits(8) for _ in range(70000))

put("text", text)
put("fox", fox)
put("seq", seq)
put("rand", rand)
put("empty", b"")
put("one", b"x")
put("two words", text)
put("epoch", text, mtime=0, ns=0)
put("future", text, mtime=2**32 + 5, ns=0)
put("dir/a", text)
put("dir/b", seq)
put("dir/sub/c", fox[:1000])
put("dir/already.gz", gzip_ref(text))
put("Upper.TGZ", gzip_ref(text))
put("arch.tgz", gzip_ref(seq))
put("arch.taz", gzip_ref(text))
put("dash-z", gzip_ref(text))
put("x-gz", gzip_ref(text))
put("under_z", gzip_ref(text))
put("dot.z", gzip_ref(text))
put("dot.Z.Z", gzip_ref(text))

# gzip members.
put("t.gz", gzip_ref(text, "-9"))
put("named.gz", subprocess.run(["/usr/bin/gzip", "-c", "-N", os.path.join(d, "text")],
                               stdout=subprocess.PIPE, check=True).stdout)
put("noname.gz", gzip_ref(text, "-n"))
put("seq.gz", gzip_ref(seq, "-6"))
put("fox.gz", gzip_ref(fox, "-1"))
put("rand.gz", gzip_ref(rand))
put("multi.gz", gzip_ref(text) + gzip_ref(seq))
put("garbage.gz", gzip_ref(text) + b"this is not gzip")
put("zeros.gz", gzip_ref(text) + b"\0\0\0\0")
put("zero1.gz", gzip_ref(text) + b"\0")
put("nul-then-garbage.gz", gzip_ref(text) + b"\0\0x")
put("badcrc.gz", member(text, crc=0x12345678))
put("badlen.gz", member(text, size=99))
put("badboth.gz", member(text, crc=1, size=2))
whole = gzip_ref(seq)
put("trunc.gz", whole[: len(whole) // 2])
put("trunc-header.gz", whole[:6])
put("trunc-trailer.gz", whole[:-3])
put("magic-only.gz", b"\x1f\x8b")
put("empty.gz", b"")
put("notgz.gz", b"plain text, not compressed\n")
put("method.gz", member(text, method=7))
put("encrypted.gz", member(text, flags=0x20))
put("reserved.gz", member(text, flags=0x40))
put("extra.gz", member(text, extra=b"AB\x02\x00hi"))
put("comment.gz", member(text, comment=b"a comment"))
put("stored-name.gz", member(text, name=b"original.txt", mtime=T + 86400))
put("path-name.gz", member(text, name=b"some/dir/inner.txt"))
put("long-name.gz", member(text, name=b"n" * 1100))
put("hcrc.gz", member(text, hcrc=True, name=b"hc.txt", extra=b"xy\x00\x00"))
bad = bytearray(member(text, hcrc=True))
bad[10] ^= 0xff
put("hcrc-bad.gz", bytes(bad))
put("stamp0.gz", member(text, mtime=0))
put("stamp-max.gz", member(text, mtime=0xffffffff))
put("xfl-os.gz", member(text, xfl=2, os_=11))
put("old-magic.gz", b"\x1f\x9e" + member(text)[2:])
put("stored-block.gz", member(b"abc", body=b"\x01\x03\x00\xfc\xffabc"))
put("fixed-block.gz", member(b"a", body=b"\x4b\x04\x00"))
put("bad-blocktype.gz", member(text, body=b"\x07\x00"))

# pack: upstream's samples.
put("banana.z", b"\x1f\x1e\x00\x00\x00\x06\x03\x01\x01\x00\x61\x6e\x62\x16\xc8")
put("pack-bad1.z", bytes(int(x, 8) for x in "037 036 000 000 037 213 010 000 000 000 000 000 002 003 036 000 000 000 002 003 037 213 010 000 000 000 000 000 002 003 355 301 001 015 000 000 000 302 240 037 000 302 240 037 213 010 000 000 000 000 000 002 003 355 301".split()))
put("pack-short.z", b"\x1f\x1e\x00\x00\x00\x06\x03\x01\x01\x00\x61\x6e\x62")
put("pack-len.z", b"\x1f\x1e\x00\x00\x00\x07\x03\x01\x01\x00\x61\x6e\x62\x16\xc8")

# compress (.Z), checked against the reference as they are made.
for name, data, mb, block in [("t.Z", text, 16, True), ("seq.Z", seq, 16, True),
                               ("fox9.Z", fox[:20000], 9, True), ("rand12.Z", rand[:30000], 12, True),
                               ("noblock.Z", seq[:5000], 16, False), ("big.Z", seq + fox, 16, True)]:
    z = lzw(data, mb, block)
    back = subprocess.run(["/usr/bin/gzip", "-dc"], input=z, stdout=subprocess.PIPE).stdout
    assert back == data, ("the test encoder is wrong", name)
    put(name, z)
put("bits17.Z", b"\x1f\x9d\x91" + lzw(text)[3:])
put("flags.Z", b"\x1f\x9d\xf0" + lzw(text)[3:])
z = bytearray(lzw(seq[:3000]))
z[40] = 0xff
z[41] = 0xff
put("corrupt.Z", bytes(z))
put("first-code.Z", b"\x1f\x9d\x90\xff\xff\xff")

# LZH: only the magic number and a little; the random section does more.
put("lzh-empty.lzh", b"\x1f\xa0")
put("lzh-zero.lzh", b"\x1f\xa0\x00\x00\x00\x00")

# pkzip.
for name, opt, members in [("one.zip", "-9", ["text"]), ("stored.zip", "-0", ["text"]),
                           ("two.zip", "-9", ["text", "seq"])]:
    subprocess.run(["zip", "-q", "-X", opt, name, *members], cwd=d, check=True)
    os.utime(os.path.join(d, name), ns=(T * 10**9, T * 10**9))
with open(os.path.join(d, "one.zip"), "rb") as f:
    zipdata = f.read()
put("zip-bad.zip", zipdata[:30])

# Random streams behind each magic number, and good files damaged.
for i in range(int(os.environ.get("GZ_RANDOM", "40"))):
    r = random.Random(1000 + i)
    n = r.randrange(1, 400)
    blob = bytes(r.getrandbits(8) for _ in range(n))
    put("r%02d-gz.gz" % i, member(b"", body=blob)[:-8] + bytes(r.getrandbits(8) for _ in range(8)))
    put("r%02d-z.Z" % i, b"\x1f\x9d" + bytes([r.choice([0x90, 0x8c, 0x89, 0x10])]) + blob)
    put("r%02d-pk.z" % i, b"\x1f\x1e" + blob)
    put("r%02d-lzh.lzh" % i, b"\x1f\xa0" + blob)
    src = r.choice([text, seq, fox[:5000], rand[:3000]])
    good = bytearray(gzip_ref(src, "-%d" % r.randrange(1, 10)))
    for _ in range(r.randrange(1, 4)):
        at = r.randrange(10, len(good))
        good[at] = r.getrandbits(8)
    put("m%02d.gz" % i, bytes(good))
PY
python3 "$gen" "$fixtures" || { echo "gzip-diff: making the fixtures failed" >&2; exit 1; }
( cd "$fixtures" && ln -s text link && cp -p text hardlink1 && ln hardlink1 hardlink2 )
( cd "$fixtures" && cp -p text setuid && chmod u+s setuid && cp -p text sticky && chmod +t sticky )

work=$DIFF_TMP/work

# One side: a fresh copy of the fixtures, the case's setup, the command.
run_side() {
  local side=$1 out=$2 err=$3; shift 3
  [ -d "$work" ] && chmod -R u+rwX "$work" 2>/dev/null
  rm -rf "$work"
  cp -a "$fixtures" "$work"
  ln -sfn "$bindir/$side" "$cur"
  (
    cd "$work" || exit 99
    if [ -n "${SETUP:-}" ]; then eval "$SETUP"; fi
    local input=/dev/null
    if [ -n "${INPUT:-}" ]; then
      printf "$INPUT" > "$DIFF_TMP/input"
      input=$DIFF_TMP/input
    fi
    if [ -n "${INFILE:-}" ]; then input=$INFILE; fi
    # shellcheck disable=SC2086
    set -- env -i PATH="$cur" LC_ALL=C.UTF-8 TZ=UTC ${ENVS:-} /usr/bin/timeout -k 2 30 "$@"
    case ${REDIR:-} in
      '>/dev/full')  diff_run "$@" <"$input" >/dev/full 2>"$err" ;;
      '>&-')         diff_run "$@" <"$input" >&- 2>"$err" ;;
      '2>/dev/full') diff_run "$@" <"$input" >"$out" 2>/dev/full ;;
      '<&-')         diff_run "$@" <&- >"$out" 2>"$err" ;;
      '|head')       { diff_run "$@" <"$input" 2>"$err"; echo $? > "$DIFF_TMP/pipe-rc"; } | head -c 100 >"$out"
                     exit "$(cat "$DIFF_TMP/pipe-rc")" ;;
      *)             diff_run "$@" <"$input" >"$out" 2>"$err" ;;
    esac
  )
}

# The working directory afterwards: one line per name.
snapshot() {
  # A directory's time is when an entry last came or went: the moment of the
  # run, which differs between the sides. Its name and mode are compared.
  ( cd "$work" && { find . -mindepth 1 -type d -printf '%P|d|%m\n'
      find . -mindepth 1 ! -type d -printf '%P|%y|%m|%s|%T@\n'; } | LC_ALL=C sort
    find . -type f -perm -u=r -exec sha256sum {} + | LC_ALL=C sort -k2 | cut -c1-16,65- )
}

compare() {
  local o_out g_out o_err g_err o_rc g_rc o_tree g_tree
  o_out=$DIFF_TMP/o.out; g_out=$DIFF_TMP/g.out; o_err=$DIFF_TMP/o.err; g_err=$DIFF_TMP/g.err
  : > "$o_out"; : > "$g_out"
  run_side ours "$o_out" "$o_err" "$@"; o_rc=$?; o_tree=$(snapshot)
  run_side gnu "$g_out" "$g_err" "$@"; g_rc=$?; g_tree=$(snapshot)
  local o_sum g_sum
  o_sum=$(sha256sum < "$o_out" | cut -c1-16); g_sum=$(sha256sum < "$g_out" | cut -c1-16)
  if [ "$o_sum" = "$g_sum" ] && [ "$o_rc" = "$g_rc" ] && cmp -s "$o_err" "$g_err" && [ "$o_tree" = "$g_tree" ]; then
    AGREED=yes
  else
    AGREED=no
  fi
  REPORT=$(
    printf '  ours rc=%s out=%s (%s bytes) err={%s}\n' "$o_rc" "$o_sum" "$(stat -c%s "$o_out")" "$(head -c 400 "$o_err" | tr '\n' '|')"
    printf '  gnu  rc=%s out=%s (%s bytes) err={%s}\n' "$g_rc" "$g_sum" "$(stat -c%s "$g_out")" "$(head -c 400 "$g_err" | tr '\n' '|')"
    if [ "$o_sum" != "$g_sum" ]; then
      cmp "$o_out" "$g_out" 2>&1 | head -2 | sed 's/^/  /'
    fi
    if [ "$o_tree" != "$g_tree" ]; then
      diff <(printf '%s\n' "$o_tree") <(printf '%s\n' "$g_tree") | head -12 | sed 's/^/  tree: /'
    fi
  )
  SETUP=''; INPUT=''; INFILE=''; ENVS=''; REDIR=''
}

report() {
  if [ "$AGREED" = yes ]; then
    pass=$((pass+1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s\n' "$1"
  else
    fail=$((fail+1))
    printf 'DIFF %s\n%s\n' "$1" "$REPORT"
  fi
  return 0
}

run_case() {
  local label="$* ${REDIR:-} ${ENVS:-} ${SETUP:+[setup: $SETUP]}"
  compare "$@"
  report "$label"
}

xfail_case() {
  local why=$1; shift
  local label="$* ${REDIR:-} ${ENVS:-}"
  compare "$@"
  if [ "$AGREED" = yes ]; then
    xpass=$((xpass+1))
    printf 'XPASS %s -- expected to differ (%s) and did not\n' "$label" "$why"
  else
    xfail=$((xfail+1))
    [ -n "${VERBOSE:-}" ] && printf 'xfail %s (%s)\n%s\n' "$label" "$why" "$REPORT"
  fi
  return 0
}

# --- the command line ----------------------------------------------------------
run_case gzip --help
run_case gzip -h
run_case gzip -H
run_case gzip --version
run_case gzip -V
run_case gzip -L
run_case gzip --license
run_case gzip --he
run_case gzip --ver
run_case gzip -?
run_case gzip -x
run_case gzip --bogus
run_case gzip --s text
run_case gzip --n text
run_case gzip --suffix
run_case gzip -S
run_case gzip -S '' text
run_case gzip -S 1234567890123456789012345678901 text
run_case gzip -S 123456789012345678901234567890 text
run_case gzip -b
run_case gzip -b 12 text
run_case gzip -b 1x text
run_case gzip -b '' text
run_case gzip --bits=9 text
run_case gzip -Z text
run_case gzip --lzw text
run_case gzip -a text
run_case gzip --ascii -q text
run_case gzip -m text
run_case gzip -M text
run_case gzip --help=x
run_case gzip -c -- -x
run_case gzip -vh
run_case gzip -hV
run_case gzip -Vh
run_case gzip --stdout --to-stdout text
run_case gzip --uncompress -c t.gz
run_case gzip --silent --verbose text
run_case gzip ---presume-input-tty
run_case gzip ---pres -d
REDIR='>/dev/full'; run_case gzip --help
REDIR='>&-';        run_case gzip --version
REDIR='>/dev/full'; run_case gzip -L
REDIR='2>/dev/full'; run_case gzip -x

# --- the GZIP variable -----------------------------------------------------------
ENVS='GZIP=-v';               run_case gzip text
ENVS='GZIP=-q';               run_case gzip text
ENVS='GZIP=-9';               run_case gzip -c fox
ENVS='GZIP=--fast';           run_case gzip -c fox
ENVS='GZIP=--best';           run_case gzip -c fox
ENVS='GZIP=--rsyncable';      run_case gzip -c seq
ENVS='GZIP=-n';               run_case gzip text
ENVS='GZIP=-N';               run_case gzip -d named.gz
ENVS='GZIP=-c';               run_case gzip text
ENVS='GZIP=--stdout';         run_case gzip text
ENVS='GZIP=--std';            run_case gzip text
ENVS='GZIP=-h';               run_case gzip
ENVS='GZIP=-S.x';             run_case gzip text
ENVS='GZIP=foo';              run_case gzip text
ENVS='GZIP=foo -v';           run_case gzip text
ENVS='GZIP=-v foo -q';        run_case gzip text
ENVS='GZIP=--';               run_case gzip text
ENVS='GZIP=-v --';            run_case gzip text
ENVS='GZIP=foo --';           run_case gzip text
ENVS='GZIP=-x';               run_case gzip text
ENVS='GZIP=--bogus';          run_case gzip text
ENVS='GZIP= ';                run_case gzip text
ENVS='GZIP=';                 run_case gzip text
ENVS='GZIP=-vq';              run_case gzip text
ENVS='GZIP=-q -v';            run_case gzip text
ENVS='GZIP=-1 -9';            run_case gzip -c fox

# --- compressing ---------------------------------------------------------------
for f in text fox seq rand empty one; do
  run_case gzip "$f"
  run_case gzip -c "$f"
  run_case gzip -k "$f"
  run_case gzip -n "$f"
  run_case gzip -c --rsyncable "$f"
done
for lv in 1 2 3 4 5 6 7 8 9; do
  run_case gzip -c -$lv fox
  run_case gzip -c -$lv seq
  run_case gzip -c -$lv rand
  run_case gzip -c -$lv --rsyncable fox
done
run_case gzip -c fox seq rand text
run_case gzip -c -1 seq rand fox
run_case gzip -c -9 rand text seq
run_case gzip text fox seq
run_case gzip -v text fox
run_case gzip -vk text
run_case gzip -q text
run_case gzip -N text
run_case gzip --no-name text
run_case gzip -S .z text
run_case gzip -S z text
run_case gzip -S _Z text
run_case gzip -S .GZ text
run_case gzip 'two words'
run_case gzip -c epoch
run_case gzip epoch
run_case gzip -n epoch
run_case gzip -c future
run_case gzip t.gz
run_case gzip -f t.gz
run_case gzip -v t.gz
run_case gzip -q t.gz
run_case gzip dash-z x-gz under_z dot.z
run_case gzip Upper.TGZ
run_case gzip dir
run_case gzip -r dir
run_case gzip -rv dir
run_case gzip -rq dir
run_case gzip -rk dir
run_case gzip -r dir/
run_case gzip -c -r dir
run_case gzip link
run_case gzip -f link
run_case gzip -c link
run_case gzip hardlink1
run_case gzip -f hardlink1
run_case gzip setuid
run_case gzip sticky
run_case gzip -f sticky
run_case gzip -c setuid
run_case gzip missing
run_case gzip missing text
run_case gzip /dev/null
run_case gzip -c /dev/null
run_case gzip .
SETUP='chmod 000 text'; run_case gzip text
SETUP='chmod 0555 .';   run_case gzip text
SETUP='chmod 0555 .';   run_case gzip -k text
SETUP='touch -d @981173106 text.gz';  run_case gzip text
SETUP='touch -d @981173106 text.gz';  run_case gzip -f text
SETUP='touch -d @981173106 text.gz';  run_case gzip -q text
SETUP='touch -d @981173106 text.gz';  INPUT='y\n'; run_case gzip ---presume-input-tty text
SETUP='touch -d @981173106 text.gz';  INPUT='n\n'; run_case gzip ---presume-input-tty text
SETUP='touch -d @981173106 text.gz';  INPUT='yes please\n'; run_case gzip ---presume-input-tty text
SETUP='touch -d @981173106 text.gz fox.gz'; INPUT='n\ny\n'; run_case gzip ---presume-input-tty text fox
SETUP='touch -d @981173106 text.gz';  run_case gzip ---presume-input-tty text
SETUP='mkdir text.gz';  run_case gzip -f text
SETUP="touch -d @981173106 $(printf 'n%.0s' $(seq 1 252)).gz 2>/dev/null; cp -p text $(printf 'n%.0s' $(seq 1 252))"
run_case gzip "$(printf 'n%.0s' $(seq 1 252))"
SETUP="cp -p text $(printf 'n%.0s' $(seq 1 254))"
run_case gzip "$(printf 'n%.0s' $(seq 1 254))"
SETUP="cp -p text $(printf 'a%.0s' $(seq 1 250)).tar"
run_case gzip "$(printf 'a%.0s' $(seq 1 250)).tar"
SETUP="cp -p text abc.$(printf 'd%.0s' $(seq 1 250))"
run_case gzip "abc.$(printf 'd%.0s' $(seq 1 250))"
run_case gzip "$(printf 'p%.0s' $(seq 1 1030))"
run_case gzip -
run_case gzip -c -
INFILE=$fixtures/fox; run_case gzip
INFILE=$fixtures/fox; run_case gzip -9
INFILE=$fixtures/fox; run_case gzip -v
INFILE=$fixtures/fox; run_case gzip - text
INFILE=$fixtures/fox; run_case gzip -c text - seq
INFILE=$fixtures/empty; run_case gzip
run_case gzip ---presume-input-tty
run_case gzip ---presume-input-tty -q
run_case gzip -f ---presume-input-tty
REDIR='>/dev/full'; run_case gzip -c text
REDIR='>&-';        run_case gzip -c text
REDIR='>&-';        run_case gzip text
REDIR='<&-';        run_case gzip text
REDIR='>/dev/full'; run_case gzip -c fox seq
REDIR='|head';      run_case gzip -c fox
REDIR='|head';      run_case gzip -c -q fox
REDIR='|head';      INFILE=$fixtures/fox; run_case gzip
run_case gzip --synchronous text
run_case gzip --synchronous -c text

# --- decompressing ---------------------------------------------------------------
for f in t.gz named.gz noname.gz seq.gz fox.gz rand.gz multi.gz; do
  run_case gzip -d "$f"
  run_case gzip -dc "$f"
  run_case gzip -t "$f"
  run_case gzip -tv "$f"
  run_case gzip -l "$f"
  run_case gzip -lv "$f"
  run_case gzip -dN "$f"
  run_case gzip -dv "$f"
done
for f in garbage.gz zeros.gz zero1.gz nul-then-garbage.gz badcrc.gz badlen.gz badboth.gz \
         trunc.gz trunc-header.gz trunc-trailer.gz magic-only.gz empty.gz notgz.gz \
         method.gz encrypted.gz reserved.gz extra.gz comment.gz stored-name.gz \
         path-name.gz long-name.gz hcrc.gz hcrc-bad.gz stamp0.gz stamp-max.gz \
         xfl-os.gz old-magic.gz stored-block.gz fixed-block.gz bad-blocktype.gz; do
  run_case gzip -d "$f"
  run_case gzip -dc "$f"
  run_case gzip -dv "$f"
  run_case gzip -t "$f"
  run_case gzip -l "$f"
  run_case gzip -dN "$f"
done
run_case gzip -df reserved.gz
run_case gzip -dff reserved.gz
run_case gzip -dcff reserved.gz
run_case gzip -dff hcrc-bad.gz
run_case gzip -dcvv stored-name.gz
run_case gzip -dvv stored-name.gz
run_case gzip -dNvv stored-name.gz
run_case gzip -lvN stored-name.gz
run_case gzip -lv t.gz seq.gz
run_case gzip -l t.gz seq.gz fox.gz
run_case gzip -lq t.gz seq.gz
run_case gzip -l t.gz badcrc.gz seq.gz
run_case gzip -l t.gz garbage.gz seq.gz
run_case gzip -l t.Z t.gz
run_case gzip -lv t.Z banana.z one.zip
run_case gzip -t t.gz badcrc.gz seq.gz
run_case gzip -tq badcrc.gz
run_case gzip -d text
run_case gzip -dv text
run_case gzip -dq text
run_case gzip -d text.gz
run_case gzip -d missing
run_case gzip -d t
run_case gzip -d seq
SETUP='mv t.gz t.z';   run_case gzip -d t
SETUP='mv t.gz t-z';   run_case gzip -d t
SETUP='mv t.gz t.Z';   run_case gzip -d t
SETUP='mv t.gz t.x';   run_case gzip -d -S .x t
SETUP='mv t.gz t.x';   run_case gzip -d -S .x t.x
run_case gzip -d Upper.TGZ
run_case gzip -d arch.tgz
run_case gzip -d arch.taz
run_case gzip -d dash-z x-gz under_z dot.z dot.Z.Z
run_case gzip -dr dir
run_case gzip -dr .
run_case gzip -tr dir
run_case gzip -lr dir
run_case gzip -t text
run_case gzip -tv text
run_case gzip -d -c -
INFILE=$fixtures/t.gz;     run_case gzip -d
INFILE=$fixtures/multi.gz; run_case gzip -dc
INFILE=$fixtures/t.gz;     run_case gzip -l
INFILE=$fixtures/t.gz;     run_case gzip -lv
INFILE=$fixtures/t.gz;     run_case gzip -t
INFILE=$fixtures/t.gz;     run_case gzip -tv
INFILE=$fixtures/garbage.gz; run_case gzip -d
INFILE=$fixtures/zeros.gz;   run_case gzip -dv
INFILE=$fixtures/notgz.gz;   run_case gzip -d
INFILE=$fixtures/notgz.gz;   run_case gzip -dcf
INFILE=$fixtures/empty;      run_case gzip -dcf
INFILE=$fixtures/one;        run_case gzip -dcf
INFILE=$fixtures/t.gz;       run_case gzip -d ---presume-input-tty
SETUP='touch -d @981173106 t';  run_case gzip -d t.gz
SETUP='touch -d @981173106 t';  run_case gzip -df t.gz
SETUP='touch -d @981173106 t';  INPUT='y\n'; run_case gzip -d ---presume-input-tty t.gz
run_case gzip -dc text t.gz
run_case gzip -dcf text t.gz
run_case gzip -dcf rand
run_case gzip -dk t.gz
run_case gzip -dkv t.gz
REDIR='>/dev/full'; run_case gzip -dc seq.gz
REDIR='>&-';        run_case gzip -dc t.gz
REDIR='>/dev/full'; run_case gzip -l t.gz
REDIR='>&-';        run_case gzip -l t.gz
REDIR='|head';      run_case gzip -dc seq.gz
REDIR='|head';      run_case gzip -dcq seq.gz

# --- other formats -------------------------------------------------------------
for f in t.Z seq.Z fox9.Z rand12.Z noblock.Z big.Z bits17.Z flags.Z corrupt.Z first-code.Z \
         banana.z pack-bad1.z pack-short.z pack-len.z lzh-empty.lzh lzh-zero.lzh \
         one.zip stored.zip two.zip zip-bad.zip; do
  run_case gzip -dc "$f"
  run_case gzip -t "$f"
  run_case gzip -l "$f"
done
run_case gzip -d t.Z seq.Z
run_case gzip -d corrupt.Z
run_case gzip -d banana.z
run_case gzip -d two.zip
run_case gzip -dc two.zip
run_case gzip -S .zip -d one.zip
run_case gzip -S .zip -d stored.zip

# --- gunzip, zcat, uncompress --------------------------------------------------
for p in gunzip zcat uncompress; do
  run_case "$p" --help
  run_case "$p" --version
  run_case "$p" -h
  run_case "$p" -V
  run_case "$p" t.gz
  run_case "$p" -v t.gz
  run_case "$p" text
  run_case "$p" t.Z
  run_case "$p" -l seq.gz
  run_case "$p" -x
  run_case "$p" -- --help
  run_case "$p" -v --help
  REDIR='>/dev/full'; run_case "$p" --help
  REDIR='>/dev/full'; run_case "$p" --version
  INFILE=$fixtures/t.gz; run_case "$p"
done
run_case zcat -f text
run_case zcat text t.gz
run_case zcat seq
ENVS='GZIP=-v'; run_case gunzip t.gz
ENVS='GZIP=-c'; run_case zcat t.gz

# --- names that are not printable ------------------------------------------------
why_name='a name with an unprintable byte is escaped in a diagnostic (deliberate difference 1)'
SETUP="cp -p text \"\$(printf 'tab\\tname')\" && touch -d @981173106 \"\$(printf 'tab\\tname').gz\""
xfail_case "$why_name" gzip "$(printf 'tab\tname')"
SETUP="cp -p text \"\$(printf 'nl\\nname')\""
run_case gzip -c "$(printf 'nl\nname')"

# --- -k, and an abort after an output is finished ----------------------------------
# Upstream unlinks t, which it finished and was asked to keep, when the empty
# operand after it aborts the run.
xfail_case 'with -k a finished output is kept when a later operand aborts (deliberate difference 4)' \
  gzip -dk t.gz empty.gz
run_case gzip -d t.gz empty.gz
# A decompression that fails (codes wider than 16 bits): under -k both
# remove the broken output; without it upstream deletes the input first.
run_case gzip -dk bits17.Z
xfail_case 'a failed decompression keeps its input (deliberate difference 4)' gzip -d bits17.Z

# --- random and damaged streams ---------------------------------------------------
for f in "$fixtures"/r*-gz.gz "$fixtures"/r*-z.Z "$fixtures"/r*-pk.z "$fixtures"/r*-lzh.lzh "$fixtures"/m*.gz; do
  b=${f##*/}
  run_case gzip -dc "$b"
  run_case gzip -t "$b"
done

printf '\n%d passed, %d differed, %d differ on purpose' "$pass" "$fail" "$xfail"
if [ "$xpass" -gt 0 ]; then
  printf ', %d NO LONGER differ (update the harness)' "$xpass"
fi
printf '\n'
[ "$fail" = 0 ] && [ "$xpass" = 0 ]
