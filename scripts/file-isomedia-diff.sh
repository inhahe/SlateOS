#!/usr/bin/env bash
# Differential test: our `file` against file 5.45's on ISO base media files.
#
# `userspace/file`'s `ftyp` branch is file 5.45's own rules
# (magic/Magdir/animation, generated into src/isomedia_table.rs by
# scripts/file-isomedia-gen.py), evaluated the way libmagic evaluates them. So
# this harness builds one file for every brand rule in that table -- a 24-byte
# `ftyp` box naming the brand, padded with zeros -- and the variants the
# nested rules read: every value of the byte after a three-letter brand
# (3GPP releases, 3GPP2 profiles, the ISO version letter), Sony XAVC's codec
# fields, and files too short for the rules that read far past the brand. Each
# is compared as `file -b FILE`.
#
# Only the description is compared. Our `file -i` prints the MIME type alone
# where GNU's adds "; charset=binary", and ours has no `--mime-type`; the MIME
# types are held to file 5.45's by the crate's unit tests instead
# (userspace/file/src/isomedia/tests.rs).
#
#   bash scripts/file-isomedia-diff.sh          # VERBOSE=1 to list agreements
set -u

DIFF_PROG='file'
DIFF_PKG='file'
DIFF_NEED='python3'
# shellcheck source=diff-wsl.sh
. "$(dirname "$0")/diff-wsl.sh"

here=$(cd "$(dirname "$0")" && pwd)
pass=0; fail=0
fx=$DIFF_TMP/fx
mkdir -p "$fx"

if ! python3 - "$here/../userspace/file/src/isomedia_table.rs" "$fx" <<'PY'
import os
import re
import struct
import sys

table, out = sys.argv[1], sys.argv[2]
src = open(table, encoding="utf-8").read()


def rust_bytes(lit):
    """The bytes of a Rust b"..." literal's body (the generator emits only
    printable ASCII and \\xNN escapes)."""
    return re.sub(rb"\\x([0-9a-fA-F]{2})", lambda m: bytes([int(m.group(1), 16)]),
                  lit.encode("latin-1"))


brands = []
for m in re.finditer(r'offset: 8,\s+test: Test::String(?:W)?\(b"((?:[^"\\]|\\.)*)"\)', src):
    b = rust_bytes(m.group(1))
    if b not in brands:
        brands.append(b)
if len(brands) < 100:
    sys.exit(f"read only {len(brands)} brands from {table}")


def ftyp(brand, pad=200):
    brand = (brand + b"    ")[:4]
    return struct.pack(">I", 24) + b"ftyp" + brand + struct.pack(">I", 0) + brand + b"mif1" + b"\0" * pad


files = {}
for b in brands:
    name = "brand-" + b.hex()
    files[name] = ftyp(b)
    if len(b) == 3:
        # The fourth brand byte is what the nested rules read: every release
        # number and profile letter they name, and some they do not.
        for v in list(range(0, 12)) + [ord(c) for c in "abcdm2456 "]:
            files[f"{name}-{v:02x}"] = ftyp(b + bytes([v]))
for b in [b"xxxx", b"f4v ", b"mp45", b"\0\0\0\0", b"\xff\xfe\xfd\xfc", b"iso\0"]:
    files["unknown-" + b.hex()] = ftyp(b)

xavc = bytearray(ftyp(b"XAVC"))
xavc[96:100] = b"mp4a"
xavc[118:120] = struct.pack(">H", 48000)
xavc[140:144] = b"avc1"
xavc[168:170] = struct.pack(">H", 1920)
xavc[170:172] = struct.pack(">H", 1080)
files["xavc-fields"] = bytes(xavc)
odd = bytearray(ftyp(b"XAVC"))
odd[96:100] = b"\x01ab\xff"
odd[140:142] = b"\x7fZ"
files["xavc-unprintable"] = bytes(odd)
long_name = bytearray(ftyp(b"XAVC"))
long_name[96:104] = b"abcdefgh"
files["xavc-long-string"] = bytes(long_name)
for n in (0, 4, 8, 11, 12, 20, 96, 97, 100, 119, 120, 141, 170, 171, 172):
    files[f"xavc-short-{n:03d}"] = (struct.pack(">I", 24) + b"ftypXAVC" + b"\0" * 300)[:n]

for name, data in files.items():
    with open(os.path.join(out, name), "wb") as f:
        f.write(data)
print(f"file-isomedia-diff: {len(brands)} brands, {len(files)} files", file=sys.stderr)
PY
then
  echo "file-isomedia-diff: could not build the fixtures" >&2
  exit 1
fi

compare() { # file
  local o g o_rc g_rc
  o=$(diff_run env LC_ALL=C.UTF-8 PATH="$bindir/ours" file -b "$1" 2>&1); o_rc=$?
  g=$(diff_run env LC_ALL=C.UTF-8 PATH="$bindir/gnu" file -b "$1" 2>&1); g_rc=$?
  if [ "$o" = "$g" ] && [ "$o_rc" = "$g_rc" ]; then
    pass=$((pass + 1))
    [ -n "${VERBOSE:-}" ] && printf 'OK   %s: %s\n' "${1##*/}" "$o"
  else
    fail=$((fail + 1))
    printf 'DIFF %s\n  ours (rc=%s): %s\n  gnu  (rc=%s): %s\n' "${1##*/}" "$o_rc" "$o" "$g_rc" "$g"
  fi
}

for f in "$fx"/*; do
  compare "$f"
done

echo "file-isomedia-diff: $pass agree, $fail differ"
[ "$fail" = 0 ]
