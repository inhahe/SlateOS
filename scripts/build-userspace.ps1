# build-userspace.ps1 — Build userspace binaries (init, hello).
#
# Userspace crates are excluded from the kernel workspace but cargo still
# finds the workspace-level .cargo/config.toml (config hierarchy is CWD-
# based, walking up parent directories).  That config contains kernel-
# specific rustflags (code-model=kernel, linker script) that break
# userspace builds.
#
# This script builds each userspace crate with RUSTFLAGS set to override
# the config-file flags entirely.  Per cargo docs, when RUSTFLAGS is set
# it takes priority over all target.<triple>.rustflags config entries.
#
# Usage:
#   .\scripts\build-userspace.ps1          # build all
#   .\scripts\build-userspace.ps1 init     # build only init
#   .\scripts\build-userspace.ps1 hello    # build only hello

param(
    [string]$Crate = "all"
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $PSScriptRoot

function Build-UserspaceCrate {
    param([string]$Name, [string]$Dir)

    Write-Host "Building $Name..." -ForegroundColor Cyan

    # Each crate has its own .cargo/config.toml with crate-specific flags,
    # but we must override via env var to prevent the parent workspace
    # config's rustflags from being merged in.
    #
    # Read the crate's own linker script and relocation model from its
    # config to replicate them in the env var.
    # Static + large model: 64-bit absolute addresses, no GOT.
    # PIC mode's GOT entries need runtime relocations that the kernel
    # doesn't process, causing NULL-pointer jumps.
    $env:RUSTFLAGS = "-C link-arg=-Tlinker.ld -C relocation-model=static -C code-model=large"

    Push-Location $Dir
    try {
        cargo build --release
        if ($LASTEXITCODE -ne 0) {
            Write-Error "Build of $Name failed."
            exit 1
        }
        Write-Host "$Name built OK." -ForegroundColor Green
    } finally {
        Pop-Location
        Remove-Item Env:\RUSTFLAGS -ErrorAction SilentlyContinue
    }
}

$crates = @{
    "init"  = "$ProjectRoot\userspace\init"
    "hello" = "$ProjectRoot\userspace\hello"
}

if ($Crate -eq "all") {
    foreach ($kv in $crates.GetEnumerator()) {
        if (Test-Path $kv.Value) {
            Build-UserspaceCrate -Name $kv.Key -Dir $kv.Value
        }
    }
} else {
    if (-not $crates.ContainsKey($Crate)) {
        Write-Error "Unknown crate: $Crate. Valid: $($crates.Keys -join ', ')"
        exit 1
    }
    Build-UserspaceCrate -Name $Crate -Dir $crates[$Crate]
}
