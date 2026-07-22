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

Write-Host "=== Building POSIX library (code-model=large) ===" -ForegroundColor Cyan
Push-Location (Join-Path $root "posix")
$env:RUSTFLAGS = "-C code-model=large"
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "posix build failed" }
Pop-Location

Write-Host ""
Write-Host "=== Building stubs library ===" -ForegroundColor Cyan
Push-Location (Join-Path $PSScriptRoot "stubs")
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
Write-Host "=== Sysroot ready ===" -ForegroundColor Green
Get-ChildItem $sysroot | ForEach-Object {
    Write-Host ("  {0,-20} {1,12:N0} bytes" -f $_.Name, $_.Length)
}
Write-Host ""
Write-Host "To build a userspace program with Rust std:" -ForegroundColor Yellow
Write-Host '  $env:CARGO_UNSTABLE_JSON_TARGET_SPEC = "true"'
Write-Host '  cargo +nightly build -Zbuild-std=core,alloc,std,panic_abort --release'
