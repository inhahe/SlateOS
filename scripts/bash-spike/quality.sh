#!/bin/bash
# Linking is not working: how many symbols bash actually uses are real
# implementations, and how many are stub-only / ENOSYS?
SYSROOT="/mnt/d/visual studio projects/os/toolchain/sysroot/lib"
POSIX="/mnt/d/visual studio projects/os/posix/src"

# Symbols bash needs that are provided ONLY by libstubs.a (not by libc.a).
comm -12 /tmp/bash_needs.txt /tmp/stub_syms.txt > /tmp/bash_stubonly_all.txt
comm -23 /tmp/bash_stubonly_all.txt /tmp/libc_syms.txt > /tmp/bash_stubonly.txt
echo "=== bash symbols served ONLY by libstubs.a: $(wc -l < /tmp/bash_stubonly.txt) ==="
head -40 /tmp/bash_stubonly.txt

echo
echo "=== ENOSYS sites in the posix crate ==="
grep -rn "ENOSYS" "$POSIX" 2>/dev/null | wc -l
echo "--- which functions report ENOSYS (file:fn context) ---"
grep -rn "ENOSYS" "$POSIX" 2>/dev/null | sed 's|.*/posix/src/||' | head -30
