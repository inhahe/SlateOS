# Build the userspace init binary.
#
# Must be run from outside the OS workspace tree to avoid inheriting
# the kernel's rustflags (code-model=kernel, kernel linker script).
#
# Usage: powershell -File build-init.ps1
#        ./build-init.ps1

$ErrorActionPreference = "Stop"

$initDir = "D:\visual studio projects\os\userspace\init"
$linkerScript = "$initDir\linker.ld"
$manifest = "$initDir\Cargo.toml"
$targetDir = "$initDir\target"

# Copy linker script to a path without spaces (Cargo RUSTFLAGS
# env var splits on spaces, breaking paths like "visual studio").
$tempLinker = "$env:USERPROFILE\linker.ld"
Copy-Item $linkerScript $tempLinker -Force

# Build from the user profile directory (outside the workspace tree)
# so Cargo doesn't walk up to .cargo/config.toml and inherit kernel flags.
$origDir = Get-Location

try {
    Set-Location $env:USERPROFILE

    $env:CARGO_BUILD_TARGET = "x86_64-unknown-none"
    $env:CARGO_TARGET_X86_64_UNKNOWN_NONE_LINKER = "rust-lld"
    $env:CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS = "-C link-arg=-T$tempLinker -C relocation-model=static -C code-model=large -C link-arg=--no-pie"
    $env:CARGO_TARGET_DIR = $targetDir

    cargo build --manifest-path $manifest --release
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Init build failed"
        exit 1
    }

    $binary = "$targetDir\x86_64-unknown-none\release\init"
    if (Test-Path $binary) {
        $size = (Get-Item $binary).Length
        Write-Host "OK: $binary ($size bytes)"
    } else {
        Write-Error "Binary not found at $binary"
        exit 1
    }
} finally {
    Set-Location $origDir
    Remove-Item Env:\CARGO_BUILD_TARGET -ErrorAction SilentlyContinue
    Remove-Item Env:\CARGO_TARGET_X86_64_UNKNOWN_NONE_LINKER -ErrorAction SilentlyContinue
    Remove-Item Env:\CARGO_TARGET_X86_64_UNKNOWN_NONE_RUSTFLAGS -ErrorAction SilentlyContinue
    Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    Remove-Item $tempLinker -ErrorAction SilentlyContinue
}
