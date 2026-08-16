#!/bin/bash
# Cross-compile bash 5.2 -> x86_64-linux-musl (the ABI SlateOS's libc.a targets).
#
# Fixes over cross.sh: the compiler is reached through a wrapper on a path with
# no spaces (configure word-splits $CC, and the repo lives under "visual studio
# projects"), and the whole build happens on the Linux filesystem — /mnt/d is
# both space-laden and slow.
. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

set -x
SPIKE="$SLATE_SPIKE"
BUILD="/tmp/bash-cross"

command -v "$SPIKE/zig/zig" >/dev/null || { echo "NO_ZIG"; exit 1; }
"$SPIKE/zig/zig" version || exit 1

# Wrapper on a spaceless path so `$CC conftest.c` survives word splitting.
cat > /tmp/zigcc <<WRAP
#!/bin/sh
exec "$SPIKE/zig/zig" cc --target=x86_64-linux-musl "\$@"
WRAP
cat > /tmp/zigar <<WRAP
#!/bin/sh
exec "$SPIKE/zig/zig" ar "\$@"
WRAP
cat > /tmp/zigranlib <<WRAP
#!/bin/sh
exec "$SPIKE/zig/zig" ranlib "\$@"
WRAP
chmod +x /tmp/zigcc /tmp/zigar /tmp/zigranlib
/tmp/zigcc --version || exit 1

# Sanity: can the wrapper build a hello-world at all?
echo 'int main(void){return 0;}' > /tmp/t.c
/tmp/zigcc /tmp/t.c -o /tmp/t.out && echo "WRAPPER_LINKS_OK" || { echo "WRAPPER_BROKEN"; exit 1; }

rm -rf "$BUILD"
mkdir -p "$BUILD"
tar xzf "$SPIKE/bash-5.2.tar.gz" -C "$BUILD" --strip-components=1 || exit 1
cd "$BUILD" || exit 1

export CC=/tmp/zigcc AR=/tmp/zigar RANLIB=/tmp/zigranlib
# --disable-readline drops termcap (9 of the 23 unresolved symbols); a spike only
# needs `bash -c` and script execution to prove the port is real.
./configure --host=x86_64-linux-musl --build=x86_64-pc-linux-gnu \
    --without-bash-malloc --disable-nls --disable-readline --without-curses \
    >cross-configure.log 2>&1
echo "CROSS_CONFIGURE_EXIT=$?"
tail -15 cross-configure.log

make -j8 >cross-make.log 2>&1
echo "CROSS_MAKE_EXIT=$?"
tail -25 cross-make.log

if [ -x "$BUILD/bash" ]; then
  file "$BUILD/bash"
  ls -l "$BUILD/bash"
  echo "CROSS_BASH_BUILT"
  cp "$BUILD/bash" "$SPIKE/bash-musl.elf"
else
  echo "NO_CROSS_BINARY"
fi
