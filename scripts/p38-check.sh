#!/bin/bash
# Path Z Part 38 — separate compilation validation against the IMAGE's tcc.
#
# Proves the multi-step flow the kernel self-test will exercise:
#   tcc -c a.c -o a.o      (compile-only: produce a relocatable ELF object)
#   tcc -c b.c -o b.o
#   tcc -o prog a.o b.o     (link two objects + crt + glibc -> dynamic exe)
#   ./prog                  (run; must print the expected line)
#
# a.c defines a function; b.c (main) calls it across the TU boundary. This is
# strictly more than Parts 36/37 (single-file compile+link): it requires tcc's
# -c object emission AND tcc-as-linker combining multiple relocatable inputs.
set -u
R="/mnt/d/visual studio projects/os/rootfs.ext4"
W=/tmp/tccx
rm -rf "$W"
mkdir -p "$W/usr/lib/x86_64-linux-gnu" "$W/lib" "$W/lib64" "$W/install/lib/tcc"

dump() { # src-in-image dst-on-host
    debugfs -R "dump \"$1\" \"$2\"" "$R" 2>/dev/null
    if [ -s "$2" ]; then echo "  dumped $1 ($(stat -c%s "$2") bytes)"; else echo "  MISSING $1"; fi
}

echo "=== rootfs: $R ==="
ls -la "$R" || { echo "NO ROOTFS"; exit 1; }

echo "=== dumping tcc + crt + libc support ==="
dump /bin/tcc "$W/tcc"
dump /usr/lib/x86_64-linux-gnu/crt1.o "$W/usr/lib/x86_64-linux-gnu/crt1.o"
dump /usr/lib/x86_64-linux-gnu/crti.o "$W/usr/lib/x86_64-linux-gnu/crti.o"
dump /usr/lib/x86_64-linux-gnu/crtn.o "$W/usr/lib/x86_64-linux-gnu/crtn.o"
dump /usr/lib/x86_64-linux-gnu/libc.so "$W/usr/lib/x86_64-linux-gnu/libc.so"
dump /usr/lib/x86_64-linux-gnu/libc_nonshared.a "$W/usr/lib/x86_64-linux-gnu/libc_nonshared.a"
dump /tmp/tccinstall/lib/tcc/libtcc1.a "$W/install/lib/tcc/libtcc1.a"
chmod +x "$W/tcc" 2>/dev/null

cat > /tmp/p38_a.c <<'EOF'
/* translation unit A: defines a function used by main in unit B */
int slate_add(int a, int b){ return a + b; }
EOF

cat > /tmp/p38_b.c <<'EOF'
/* translation unit B: main, calls across the TU boundary into unit A */
extern int printf(const char *fmt, ...);
extern int slate_add(int a, int b);
int main(void){
  int r = slate_add(40, 2);
  printf("SLATE-SEP-%d\n", r);
  return 0;
}
EOF

echo "=== step 1: tcc -c p38_a.c -o p38_a.o ==="
"$W/tcc" -c /tmp/p38_a.c -o /tmp/p38_a.o && echo "compile A OK" || { echo "COMPILE A FAILED"; exit 1; }
echo "  a.o type: $(xxd /tmp/p38_a.o | head -1)"

echo "=== step 2: tcc -c p38_b.c -o p38_b.o ==="
"$W/tcc" -c /tmp/p38_b.c -o /tmp/p38_b.o && echo "compile B OK" || { echo "COMPILE B FAILED"; exit 1; }

echo "=== step 3: tcc -o p38 p38_a.o p38_b.o (link) ==="
"$W/tcc" -o /tmp/p38 /tmp/p38_a.o /tmp/p38_b.o && echo "link OK" || { echo "LINK FAILED"; exit 1; }

echo "=== readelf interp (must be dynamic) ==="
readelf -l /tmp/p38 2>/dev/null | grep -A1 INTERP

echo "=== run (redirected to file, like SlateOS) ==="
/tmp/p38 > /tmp/p38.out; echo "exit=$?"
echo "=== output bytes ==="
xxd /tmp/p38.out
echo "=== output text ==="
cat /tmp/p38.out
echo "=== EXPECT: SLATE-SEP-42 ==="
