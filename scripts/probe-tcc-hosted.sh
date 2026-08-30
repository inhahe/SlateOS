#!/bin/bash
# Probe: what files does tcc open to compile+link a hosted dynamic glibc program?
export PATH=/tmp/tccinstall/bin:$PATH
cat > /tmp/hosted.c <<'CEOF'
extern int puts(const char *s);
int main(void) { puts("SLATE_TCC_HOSTED_OK"); return 0; }
CEOF
echo "=== tcc -vv link trace ==="
tcc -vv -o /tmp/hosted /tmp/hosted.c 2>&1
echo "exit=$?"
echo "=== file ==="
file /tmp/hosted
echo "=== run ==="
/tmp/hosted
echo "ran=$?"
echo "=== libc.so linker script ==="
cat /usr/lib/x86_64-linux-gnu/libc.so
echo "=== sizes ==="
ls -l /usr/lib/x86_64-linux-gnu/crt1.o /usr/lib/x86_64-linux-gnu/crti.o /usr/lib/x86_64-linux-gnu/crtn.o /usr/lib/x86_64-linux-gnu/libc_nonshared.a /tmp/tccinstall/lib/tcc/libtcc1.a
echo "=== tcc search dirs ==="
tcc -print-search-dirs 2>&1
