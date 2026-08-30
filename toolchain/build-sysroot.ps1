# build-sysroot.ps1 — Build the sysroot for Rust std userspace programs.
#
# This script:
# 1. Builds the posix crate as a staticlib with code-model=large
# 2. Builds the stubs crate (symbols std needs that posix doesn't provide)
# 3. Assembles the sysroot directory (libc.a, libstubs.a, libunwind.a)
#
# After running this, userspace programs can be built with:
#   cd userspace/<program>
#   $env:CARGO_UNSTABLE_JSON_TARGET_SPEC = "true"
#   cargo +nightly build -Zbuild-std=core,alloc,std,panic_abort --release

$ErrorActionPreference = "Stop"

$root = Split-Path $PSScriptRoot -Parent
$sysroot = Join-Path $PSScriptRoot "sysroot\lib"

# The sysroot is compiled for `x86_64-unknown-none` but *linked into*
# `x86_64-slateos` programs, so its codegen options must match that target's
# ABI (toolchain/x86_64-slateos.json), not unknown-none's defaults.  Setting
# $env:RUSTFLAGS replaces the `[target.x86_64-unknown-none]` rustflags in
# .cargo/config.toml wholesale, which is what we want here — those are tuned
# for the kernel and the bare-metal services, which really are soft-float.
#
#   code-model=large        unknown-none defaults to `kernel`; our programs
#                           are loaded high, so they need `large`.
#   relocation-model=static unknown-none is PIE-by-default, so libc.a came
#                           out PIC and only linked because lld happened to
#                           relax the GOT/TLS accesses at static-link time.
#                           x86_64-slateos is `relocation-model: static`
#                           with PIE off; match it rather than rely on that.
#   +sse,+sse2,-soft-float  THE IMPORTANT ONE.  `x86_64-unknown-none` is
#                           `-sse,+soft-float`, so every libc function with a
#                           float in its signature was compiled to the
#                           soft-float ABI: `strtod`/`strtof`/`atof`/
#                           `difftime` returned their result in %rax and
#                           printf's `%f` read varargs from the wrong place.
#                           Callers are x86_64-slateos (`+sse,+sse2`) and C
#                           fixtures built by `zig cc` (SSE2 is x86-64
#                           baseline), which all read %xmm0 — so every one of
#                           those calls silently returned garbage.  See
#                           BUG-SYSROOT-SOFT-FLOAT-ABI in known-issues.md.
#   codegen-units=4096      THE OTHER IMPORTANT ONE, and it is about the
#                           *archive*, not the code.  A libc archive is not
#                           just a bag of functions: the linker extracts a
#                           member only if that member defines a symbol still
#                           undefined, so an archive's object granularity is
#                           the granularity at which a program can *decline*
#                           our definition and use its own.  glibc gets one
#                           object per `.c` file for free; rustc's default
#                           `codegen-units = 16` instead packed this whole
#                           crate into 16 objects with unrelated modules
#                           merged, so `getopt` shared a member with
#                           `sem_wait`, `glob` with `printf`, `fnmatch` with
#                           `fopen`, and `error` with `getenv`.  Every one of
#                           those four names is one that gnulib supplies a
#                           replacement for, i.e. one that every GNU package
#                           defines itself — and every one of them rode along
#                           with a symbol no C program can avoid, so the
#                           member was always extracted and the duplicate was
#                           unavoidable.  GNU make 4.4.1 failed to link with
#                           11 duplicate symbols and 0 undefined ones for
#                           exactly this reason; at 4096 the partitioner stops
#                           merging and emits ~one object per module (fnmatch
#                           alone, glob+globfree, the getopt family, the error
#                           family), and the duplicate count went to 0.
#                           See design-decisions.md §339 and
#                           scripts/make-spike/README.md.
$sysrootFlags = "-C code-model=large " +
                "-C relocation-model=static " +
                "-C codegen-units=4096 " +
                "-C target-feature=+sse,+sse2,-soft-float"

Write-Host "=== Building POSIX library ($sysrootFlags) ===" -ForegroundColor Cyan
Push-Location (Join-Path $root "posix")
$env:RUSTFLAGS = $sysrootFlags
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "posix build failed" }
Pop-Location

Write-Host ""
# Same flags: libstubs.a is linked into the same x86_64-slateos programs, so
# it needs the same ABI.  ($env:RUSTFLAGS is still set from above; assigning
# it again makes that deliberate rather than incidental.)
Write-Host "=== Building stubs library ===" -ForegroundColor Cyan
Push-Location (Join-Path $PSScriptRoot "stubs")
$env:RUSTFLAGS = $sysrootFlags
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "stubs build failed" }
Pop-Location

Write-Host ""
Write-Host "=== Assembling sysroot ===" -ForegroundColor Cyan
New-Item -ItemType Directory -Force -Path $sysroot | Out-Null

# Both `posix` and the stubs crate are workspace members, so Cargo writes
# their artifacts to the WORKSPACE-ROOT target dir, not a per-crate
# `posix\target` / `toolchain\stubs\target`.  (This changed when the crates
# were folded into the root workspace; the old per-crate paths silently
# copied a stale libc.a.)  Anchor both copies at the root target dir.
$rootTarget = Join-Path $root "target\x86_64-unknown-none\release"

# libc.a = posix staticlib (provides all POSIX/libc functions)
Copy-Item (Join-Path $rootTarget "libposix.a") `
          (Join-Path $sysroot "libc.a") -Force

# libstubs.a = symbols std needs that posix doesn't have yet
Copy-Item (Join-Path $rootTarget "libslateos_stubs.a") `
          (Join-Path $sysroot "libstubs.a") -Force

# libunwind.a = empty archive (unwind symbols are in libstubs.a,
# but std links -lunwind so the archive must exist)
$llvm_ar = Get-ChildItem -Path "$env:USERPROFILE\.rustup" -Recurse -Filter "llvm-ar.exe" |
           Where-Object { $_.FullName -match "stable" } |
           Select-Object -First 1 -ExpandProperty FullName
if ($llvm_ar) {
    & $llvm_ar rc (Join-Path $sysroot "libunwind.a")
} else {
    # Fallback: minimal valid archive header
    [System.IO.File]::WriteAllText((Join-Path $sysroot "libunwind.a"), "!<arch>`n")
}

Write-Host ""
# Assert the archive's SHAPE, not just that it built.
#
# `$sysrootFlags` above carries `-C codegen-units=4096` for a reason that is
# invisible in the output it produces: it controls how many object files the
# crate is split into, and therefore which functions share an archive member.
# A member is extracted whole -- so two functions in one member cannot be
# taken separately, and a program that brings its own `getopt` cannot decline
# ours if ours rides along with a symbol the program does need. That is an ABI
# property of libc.a, and it is exactly the sort of property that decays
# silently: nothing fails to build, no test goes red, and the damage only
# appears the next time someone ports a GNU package, weeks away from whatever
# change caused it. GNU make 4.4.1 found it for us, at the cost of a day.
#
# So the check runs HERE, in the script that produces the artifact, rather than
# living in scripts/ for someone to remember. It parses the archive's own
# symbol index (no `nm`, which does not exist on this host outside WSL) and
# costs well under a second.
#
# It is FATAL. A warning would make this the third thing in this project that
# was written, shipped, and then never actually run.
Write-Host "=== Checking libc.a archive shape ===" -ForegroundColor Cyan
$pyShape = Get-Command python -ErrorAction SilentlyContinue
if (-not $pyShape) { $pyShape = Get-Command python3 -ErrorAction SilentlyContinue }
if ($pyShape) {
    & $pyShape.Source (Join-Path $root "scripts\check-libc-shape.py") (Join-Path $sysroot "libc.a")
    if ($LASTEXITCODE -ne 0) { throw "libc.a shape check failed (see above) - the sysroot is built but not safe to port GNU packages against" }
} else {
    Write-Host "  WARNING: no python on PATH - archive shape NOT checked." -ForegroundColor Yellow
    Write-Host "  Run scripts/check-libc-shape.py by hand before trusting this sysroot." -ForegroundColor Yellow
}

Write-Host ""
# Record what this sysroot was built from, by content.
#
# Without this, the only way to ask "is libc.a behind its sources?" was to
# compare mtimes -- and that check was unsatisfiable by its own advice. This
# script assembles with `Copy-Item`, which PRESERVES the source timestamp, so
# libc.a's mtime is whenever cargo last *linked* libposix.a. If posix has not
# changed, cargo does not relink, the mtime does not move, and re-running this
# script -- the remedy the checker prints -- changes nothing. Meanwhile git
# writes mtimes on files it has not edited (`checkout`, `merge`, `stash`), and
# CLAUDE.md requires a merge from origin/main at the start of every task. So the
# gate got wedged by routine work and could not be cleared by following its own
# instructions. See known-issues.md
# A-SYSROOT-STALENESS-GATE-IS-WEDGED-BY-GIT-TOUCHING-A-FILE-IT-WATCHES.
#
# The stamp is rewritten on every run whether or not cargo relinked, which is
# what makes the gate satisfiable again. The hashing lives in the Python
# checker rather than here on purpose: a stamp written by one implementation
# and verified by another is a second place to get the CRLF-folding rule wrong,
# which is the bug that took the fixture stamps to version 2.
Write-Host "=== Stamping sysroot inputs ===" -ForegroundColor Cyan
$py = Get-Command python -ErrorAction SilentlyContinue
if (-not $py) { $py = Get-Command python3 -ErrorAction SilentlyContinue }
if ($py) {
    & $py.Source (Join-Path $root "scripts\ctest-fixtures.py") sysroot-stamp
    if ($LASTEXITCODE -ne 0) { throw "sysroot stamp failed" }
} else {
    # Not fatal: the sysroot itself is built and usable, and the checker falls
    # back to the mtime test when the stamp is absent. Loud, though, because
    # that fallback is the weaker check this stamp exists to replace.
    Write-Host "  WARNING: no python on PATH - no .sysroot.stamp written." -ForegroundColor Yellow
    Write-Host "  Staleness checks will fall back to comparing mtimes, which git can trip." -ForegroundColor Yellow
}

Write-Host ""
Write-Host "=== Sysroot ready ===" -ForegroundColor Green
Get-ChildItem $sysroot | ForEach-Object {
    Write-Host ("  {0,-20} {1,12:N0} bytes" -f $_.Name, $_.Length)
}
Write-Host ""
Write-Host "To build a userspace program with Rust std:" -ForegroundColor Yellow
Write-Host '  $env:CARGO_UNSTABLE_JSON_TARGET_SPEC = "true"'
Write-Host '  cargo +nightly build -Zbuild-std=core,alloc,std,panic_abort --release'
