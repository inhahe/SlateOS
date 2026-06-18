#!/bin/bash
# Extract tcc + crt + libtcc1.a from rootfs.ext4 and strace a hosted compile.
set -u
R="/mnt/d/visual studio projects/os/rootfs.ext4"
STRACE=/tmp/straceroot/usr/bin/strace
WORK=/tmp/tccx
rm -rf "$WORK"
mkdir -p "$WORK/usr/lib/x86_64-linux-gnu" "$WORK/lib" "$WORK/lib64" "$WORK/install/lib/tcc"

echo "=== rootfs: $R ==="
ls -la "$R" || { echo "NO ROOTFS"; exit 1; }

dump() { # src-in-image dst-on-host
    debugfs -R "dump \"$1\" \"$2\"" "$R" 2>/dev/null
    if [ -s "$2" ]; then echo "  dumped $1 -> $2 ($(stat -c%s "$2") bytes)"; else echo "  MISSING $1"; fi
}

echo "=== dumping files ==="
dump /bin/tcc "$WORK/tcc"
dump /usr/lib/x86_64-linux-gnu/crt1.o "$WORK/usr/lib/x86_64-linux-gnu/crt1.o"
dump /usr/lib/x86_64-linux-gnu/crti.o "$WORK/usr/lib/x86_64-linux-gnu/crti.o"
dump /usr/lib/x86_64-linux-gnu/crtn.o "$WORK/usr/lib/x86_64-linux-gnu/crtn.o"
dump /usr/lib/x86_64-linux-gnu/libc.so "$WORK/usr/lib/x86_64-linux-gnu/libc.so"
dump /usr/lib/x86_64-linux-gnu/libc_nonshared.a "$WORK/usr/lib/x86_64-linux-gnu/libc_nonshared.a"
chmod +x "$WORK/tcc" 2>/dev/null

# find libtcc1.a path inside image: search the staged tree listing isn't available; use known /tmp/tccinstall
dump /tmp/tccinstall/lib/tcc/libtcc1.a "$WORK/install/lib/tcc/libtcc1.a"

echo "=== ehdr of crt1.o (host xxd) ==="
xxd "$WORK/usr/lib/x86_64-linux-gnu/crt1.o" 2>/dev/null | head -4

echo "=== run tcc under strace (host glibc, using image's tcc) ==="
printf 'extern int puts(const char *s);\nint main(void){puts("SLATE_TCC_HOSTED_OK");return 0;}\n' > /tmp/hosted.c
cd "$WORK"
# Run the image tcc with explicit search paths matching SlateOS layout
"$STRACE" -f -e trace=openat,open,read,pread64,lseek,mmap,fstat,close,write \
    "$WORK/tcc" -o /tmp/hosted /tmp/hosted.c 2> /tmp/strace.out
echo "tcc exit=$?"
echo "=== strace lines mentioning crt1.o / object reads ==="
grep -nE 'crt1.o|crti.o|crtn.o|libc' /tmp/strace.out | head -40
echo "=== full strace tail ==="
tail -60 /tmp/strace.out
