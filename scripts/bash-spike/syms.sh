#!/bin/bash
# Spike helper: what does SlateOS's libc.a define, and what does bash need?
SYSROOT="/mnt/d/visual studio projects/os/toolchain/sysroot/lib"
BASHDIR="/mnt/d/visual studio projects/os/build/spike/bash-5.2"

cd "$SYSROOT" || exit 1
# Defined symbols (text/data/weak/indirect) provided by the SlateOS libc + stubs.
nm --defined-only libc.a 2>/dev/null | awk '$2 ~ /^[TWDiRB]$/ {print $3}' | sort -u > /tmp/libc_syms.txt
nm --defined-only libstubs.a 2>/dev/null | awk '$2 ~ /^[TWDiRB]$/ {print $3}' | sort -u > /tmp/stub_syms.txt
cat /tmp/libc_syms.txt /tmp/stub_syms.txt | sort -u > /tmp/provided.txt
echo "libc.a defines:    $(wc -l < /tmp/libc_syms.txt)"
echo "libstubs.a defines:$(wc -l < /tmp/stub_syms.txt)"
echo "provided total:    $(wc -l < /tmp/provided.txt)"

# Undefined symbols referenced by every bash object file.
cd "$BASHDIR" || exit 1
find . -name '*.o' -print0 | xargs -0 nm --undefined-only 2>/dev/null \
  | awk '$1 == "U" {print $2}' | sed 's/@.*//' | sort -u > /tmp/bash_needs.txt
echo "bash references:   $(wc -l < /tmp/bash_needs.txt)"

# What bash needs that SlateOS does not provide (ignore bash-internal symbols,
# which resolve among bash's own objects).
comm -23 /tmp/bash_needs.txt /tmp/provided.txt > /tmp/missing_all.txt
echo "unresolved (incl. bash-internal): $(wc -l < /tmp/missing_all.txt)"

# Drop symbols bash itself defines somewhere in its own objects.
find . -name '*.o' -print0 | xargs -0 nm --defined-only 2>/dev/null \
  | awk '$2 ~ /^[TWDiRBCS]$/ {print $3}' | sort -u > /tmp/bash_defines.txt
comm -23 /tmp/missing_all.txt /tmp/bash_defines.txt > /tmp/missing_libc.txt
echo "=== MISSING FROM SLATEOS LIBC: $(wc -l < /tmp/missing_libc.txt) ==="
cat /tmp/missing_libc.txt
