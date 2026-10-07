#!/bin/bash
# Cross-compile the Mono runtime (mono-sgen) and link it against SlateOS's libc.a.
#
# The "try the port before you write a line" step from roadmap-detailed.md's
# "Porting vs. Reimplementing" policy, applied to the .NET runtime the
# operator asked for (design-decisions.md 1050: "I want Mono (dotnet support
# for Linux) ported too"; roadmap.md suggests lane D, as a language runtime
# like CPython). Shaped like scripts/gdb-spike/run.sh and
# scripts/make-spike/run.sh, down to the names of the numbers it prints.
#
# WHAT THIS ANSWERS, AND WHAT IT DOES NOT
#
# One question: does the Mono 6.14.1 runtime -- the JIT and virtual machine,
# `mono-sgen`, with its SGen garbage collector -- resolve every symbol it
# needs against `toolchain/sysroot/lib/libc.a`?
#
# Not whether it runs a program. That needs the class libraries
# (mscorlib.dll and the rest), which are .NET assemblies built by Mono's own
# C# compiler, and are the same files on every operating system: a second
# step, once this links. Nor does it answer whether the runtime's demands on
# the kernel are met -- see the README for the two that matter most (how a
# fault becomes a NullReferenceException, and how the collector stops
# threads).
#
# Run it from WSL. The work tree is durable ($SLATE_WORK, not /tmp, which WSL
# empties when the distribution idles).
set -uo pipefail
set -x

. "$(dirname "${BASH_SOURCE[0]}")/../lib/worktree.sh" || exit 1

SYSROOT="$SLATE_SYSROOT"
WORK="$SLATE_WORK/mono-spike"
SPIKE_LIBS="$WORK/libs"
JOBS="${SLATE_SPIKE_JOBS:-8}"
VER="$SLATE_MONO_VERSION"

slate_make_zig_wrappers || exit 1
slate_ensure_mono_src || exit 1

mkdir -p "$WORK" "$SPIKE_LIBS" && cd "$WORK" || exit 1
cp "$SYSROOT/libc.a" "$SYSROOT/libunwind.a" "$SPIKE_LIBS/" || exit 1
slate_make_link_wrappers "$WORK/bin" "$SPIKE_LIBS" || exit 1
export CC="$SLATE_LINK_CC" CXX="$SLATE_LINK_CXX" AR="$SLATE_AR" RANLIB="$SLATE_RANLIB"

rm -rf "mono-$VER"
tar xf "$SLATE_MONO_TARBALL" || exit 1
cd "mono-$VER" || exit 1

# --build as well as --host, as in every spike: a program linked against our
# libc.a cannot run on Linux, so configure must not try.
#
# What is built is the runtime and nothing else, each omission for a reason:
#   --disable-mcs-build, --with-mcs-docs=no: the C# compiler and the class
#       libraries, which are .NET assemblies and need a C# compiler to build;
#       they are the same files on every system, a second step.
#   --disable-support-build: libMonoPosixHelper, Mono.Posix's native half,
#       which Mono loads as a shared module; a static runtime needs it linked
#       in instead, which is its own change.
#   --with-shared_mono=no (and --disable-shared): libmono as a shared
#       library; --with-static_mono=yes, the default, links it into mono-sgen.
#   --disable-boehm: the older of Mono's two collectors; SGen is the one.
#   --disable-btls: BoringSSL, for TLS in the class libraries -- a port of
#       its own, built with cmake.
#   --with-libgdiplus=no, --with-ikvm-native=no: System.Drawing's native
#       library and the Java interop shim.
#   --enable-cooperative-suspend: SGen stops threads for a collection at
#       safepoints the JIT emits, rather than by signalling each thread and
#       having its handler wait in sigsuspend. The signal way needs a mask per
#       thread, and a thread's mask here is the process's (pthread_sigmask is
#       sigprocmask -- known-issues/D-POSIX-LIBC-LACKS-WHAT-GLIBCS-HEADERS-
#       DECLARE.md's last two names wait on the same thing).
#   --disable-nls: message catalogues; the C locale is the only one we have.
./configure --build=x86_64-pc-linux-gnu --host=x86_64-linux-musl \
    --disable-shared --enable-static \
    --with-shared_mono=no --with-static_mono=yes \
    --disable-mcs-build --with-mcs-docs=no --disable-support-build \
    --disable-boehm --disable-btls --disable-nls \
    --with-libgdiplus=no --with-ikvm-native=no \
    --enable-cooperative-suspend \
    >conf.log 2>&1
echo "CONFIGURE_EXIT=$?"
tail -25 conf.log

# What configure decided about the facilities the runtime leans on hardest,
# kept in the log: config.h lives in the work tree and does not outlive it.
echo "MONO_DECISIONS_FROM_CONFIG_H:"
if [ -f config.h ]; then
    grep -E 'define (HAVE_SIGACTION|HAVE_WORKING_SIGALTSTACK|USE_COOP_GC|ENABLE_COOP_SUSPEND|HAVE_KW_THREAD|MONO_KEYWORD_THREAD|HAVE_DL_ITERATE_PHDR|HAVE_PTHREAD_ATTR_GETSTACK|HAVE_GETCONTEXT|HAVE_MPROTECT|HAVE_SYS_MMAN_H)' config.h
else
    echo "ERROR: no config.h -- configure did not finish; the decisions above are UNRECORDED."
fi

# -k: one failing object must not hide the rest.
make -k -j"$JOBS" >make.log 2>&1
echo "MAKE_EXIT=$?"
grep -E ' error: |Error [0-9]' make.log | grep -v 'undefined symbol' | head -30

# The decisive step: mono-sgen's link on its own, so its counts can be read.
link_log="$WORK/mono-link.log"
rm -f mono/mini/mono-sgen
make -C mono/mini mono-sgen >"$link_log" 2>&1
echo "MONO_LINK_EXIT=$?"
# Reached means the linker ran: it made the program, or wrote why not. Not
# "the log names ld.lld" alone -- a link that succeeds prints only make's
# CCLD line, and this test's first version called Mono's clean link
# unreached (scripts/gdb-spike/slatelink.sh learned it the same day).
if [ ! -x mono/mini/mono-sgen ] && ! grep -q "ld\.lld" "$link_log"; then
    # Not reached: an object it needs did not compile. A count of 0 here
    # would read as "nothing is missing", so it is not printed.
    echo "MONO_LINK_NOT_REACHED -- the build of its objects failed:"
    grep -E ' error: |Error [0-9]|No rule to make' "$link_log" | head -10
    echo "NO_SLATE_MONO_BINARY"
    exit 1
fi
grep -oP "undefined symbol: \K.*" "$link_log" | sort -u >"$WORK/mono-missing.txt"
echo "MONO_MISSING_COUNT=$(wc -l <"$WORK/mono-missing.txt")"
cat "$WORK/mono-missing.txt"
# Printed even at zero: a symbol defined twice fails a link as surely as
# one defined nowhere (make-spike's note).
grep -oP "duplicate symbol: \K.*" "$link_log" | sort -u >"$WORK/mono-dupes.txt"
echo "MONO_DUPLICATE_COUNT=$(wc -l <"$WORK/mono-dupes.txt")"
cat "$WORK/mono-dupes.txt"

if [ -x mono/mini/mono-sgen ]; then
    file mono/mini/mono-sgen
    readelf -h mono/mini/mono-sgen | grep -E "Type|Entry"
    # `file` calls it "static-pie": Mono links with -Wl,--export-dynamic,
    # which gives a static executable a PT_DYNAMIC segment. readelf's Type is
    # the fact (EXEC), and the kernel takes a program for dynamic only by its
    # PT_INTERP (kernel/src/proc/elf.rs, interp_path), which this has none of.
    if [ "$(readelf -l mono/mini/mono-sgen | grep -c INTERP)" -ne 0 ]; then
        echo "MONO_HAS_AN_INTERPRETER -- linked dynamically; nothing here can load it"
        echo "NO_SLATE_MONO_BINARY"
        exit 1
    fi
    # The link must be against ours, not musl's: our libc.a is Rust, and
    # brings its panic entry point; musl's would bring __syscall_cp. Counted
    # over the whole symbol table, not `nm | grep -q`, which pipefail turns
    # into a failure on a match (scripts/gdb-spike/slatelink.sh says how).
    syms="$(nm mono/mini/mono-sgen 2>/dev/null)"
    if [ "$(grep -c 'rust_begin_unwind' <<<"$syms")" -eq 0 ] \
        || [ "$(grep -c '__syscall_cp' <<<"$syms")" -ne 0 ]; then
        echo "MONO_NOT_OUR_LIBC -- the link did not take this library's libc.a"
        echo "NO_SLATE_MONO_BINARY"
        exit 1
    fi
    # --strip-debug, as GDB's and CPython's are staged: the symbol table
    # stays, so a backtrace names its functions, and the DWARF goes.
    mkdir -p "$SLATE_SPIKE" || exit 1
    strip --strip-debug -o "$SLATE_SPIKE/mono-sgen-slateos.elf" mono/mini/mono-sgen || exit 1
    ls -l "$SLATE_SPIKE/mono-sgen-slateos.elf"
    echo "SLATE_MONO_BUILT"
else
    echo "NO_SLATE_MONO_BINARY"
    exit 1
fi
