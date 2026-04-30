# run-qemu.ps1 — Build the kernel and run it in QEMU.
#
# Usage:
#   .\scripts\run-qemu.ps1           # build + run
#   .\scripts\run-qemu.ps1 -NoBuild  # run without rebuilding
#   .\scripts\run-qemu.ps1 -Test     # boot test: exits after BOOT_OK or timeout

param(
    [switch]$NoBuild,
    [switch]$Test,
    [int]$Timeout = 30,
    [int]$Memory = 256
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$KernelBin   = "$ProjectRoot\target\x86_64-unknown-none\debug\kernel"
$EspDir      = "$ProjectRoot\build\esp"
$QemuExe     = "C:\Program Files\qemu\qemu-system-x86_64.exe"
$OvmfCode    = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
$OvmfVars    = "$ProjectRoot\build\ovmf-vars.fd"
$DiskImg     = "$ProjectRoot\disk.img"

# Step 0: Create OVMF variable store if it doesn't exist.
#
# OVMF needs a separate pflash drive for UEFI variables.  Without one
# it falls back to writing NVVARS files on any writable FAT it finds
# — which corrupts our virtio-blk test disk between boots.
#
# We copy the edk2-i386-vars.fd template (540,672 bytes).  The UEFI
# variable store format is architecture-independent, so the i386 template
# works for x86_64 firmware.
$OvmfVarsTemplate = "C:\Program Files\qemu\share\edk2-i386-vars.fd"
if (-not (Test-Path $OvmfVars)) {
    Write-Host "Creating OVMF variable store ($OvmfVars)..." -ForegroundColor Cyan
    New-Item -ItemType Directory -Path (Split-Path $OvmfVars) -Force | Out-Null
    Copy-Item $OvmfVarsTemplate $OvmfVars -Force
    Write-Host "Copied OVMF vars template." -ForegroundColor Green
}

# Step 1: Build
if (-not $NoBuild) {
    Write-Host "Building kernel..." -ForegroundColor Cyan
    Push-Location $ProjectRoot
    cargo build 2>&1 | ForEach-Object { Write-Host $_ }
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Build failed."
        Pop-Location
        exit 1
    }
    Pop-Location
    Write-Host "Build OK." -ForegroundColor Green
}

# Step 2: Stage boot files
Write-Host "Staging boot files..." -ForegroundColor Cyan
New-Item -ItemType Directory -Path "$EspDir\EFI\BOOT" -Force | Out-Null
New-Item -ItemType Directory -Path "$EspDir\boot" -Force | Out-Null
Copy-Item "$ProjectRoot\limine\BOOTX64.EFI" "$EspDir\EFI\BOOT\BOOTX64.EFI" -Force
Copy-Item $KernelBin "$EspDir\boot\kernel" -Force
Copy-Item "$ProjectRoot\limine.conf" "$EspDir\limine.conf" -Force

# Step 3: Run QEMU
# Note: OVMF and ESP paths may contain spaces (e.g. "C:\Program Files\...").
# Start-Process -ArgumentList joins array elements with spaces, which breaks
# paths that contain spaces.  We build a single argument string with quoted
# paths to avoid this issue.

# OVMF pflash: unit 0 = firmware code (read-only), unit 1 = variable store (read-write).
$pflashArgs = "-drive `"if=pflash,format=raw,readonly=on,file=$OvmfCode`" " +
              "-drive `"if=pflash,format=raw,file=$OvmfVars`""

if ($Test) {
    # Boot test mode: capture serial to file, check for BOOT_OK
    $serialFile = "$ProjectRoot\build\serial-test.txt"

    # Delete stale serial file to prevent false positives if QEMU fails to start.
    if (Test-Path $serialFile) { Remove-Item $serialFile -Force }

    $argString = "$pflashArgs " +
                 "-drive `"format=raw,file=fat:rw:$EspDir`" " +
                 "-m ${Memory}M -machine q35 -no-reboot " +
                 "-serial `"file:$serialFile`" -display none"

    # Add virtio-blk disk if the disk image exists.
    if (Test-Path $DiskImg) {
        $argString += " -drive `"file=$DiskImg,if=virtio,format=raw`""
    }

    # Add virtio-net device with user-mode networking.
    $argString += " -device virtio-net-pci,netdev=net0 -netdev user,id=net0"

    Write-Host "Running boot test (timeout: ${Timeout}s)..." -ForegroundColor Cyan
    $proc = Start-Process -FilePath $QemuExe -ArgumentList $argString -PassThru -NoNewWindow
    $elapsed = 0
    while (-not $proc.HasExited -and $elapsed -lt $Timeout) {
        Start-Sleep -Seconds 1
        $elapsed++
        if (Test-Path $serialFile) {
            $content = Get-Content $serialFile -Raw -ErrorAction SilentlyContinue
            if ($content -match "BOOT_OK") {
                Write-Host "BOOT_OK detected after ${elapsed}s!" -ForegroundColor Green
                Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
                exit 0
            }
        }
    }

    if (-not $proc.HasExited) {
        Stop-Process -Id $proc.Id -Force
    }

    # Check final output
    if (Test-Path $serialFile) {
        $content = Get-Content $serialFile -Raw -ErrorAction SilentlyContinue
        if ($content -match "BOOT_OK") {
            Write-Host "BOOT_OK found." -ForegroundColor Green
            exit 0
        } elseif ($content -match "PANIC") {
            Write-Host "KERNEL PANIC detected!" -ForegroundColor Red
            # Show panic info
            $content -split "`n" | Where-Object { $_ -match "PANIC|FATAL|EXCEPTION" } | ForEach-Object { Write-Host $_ -ForegroundColor Red }
            exit 1
        }
    }

    Write-Host "BOOT_OK not found within ${Timeout}s. Test FAILED." -ForegroundColor Red
    exit 1

} else {
    # Interactive mode: serial to console.
    # Use call operator (&) with splatting — each array element becomes a
    # separate argument, and PowerShell handles quoting correctly.
    $qemuArgs = @(
        "-drive", "if=pflash,format=raw,readonly=on,file=$OvmfCode",
        "-drive", "if=pflash,format=raw,file=$OvmfVars",
        "-drive", "format=raw,file=fat:rw:$EspDir",
        "-m", "${Memory}M",
        "-machine", "q35",
        "-no-reboot",
        "-serial", "stdio"
    )
    # Add virtio-blk disk if the disk image exists.
    if (Test-Path $DiskImg) {
        $qemuArgs += @("-drive", "file=$DiskImg,if=virtio,format=raw")
    }
    # Add virtio-net device with user-mode networking.
    $qemuArgs += @("-device", "virtio-net-pci,netdev=net0", "-netdev", "user,id=net0")
    Write-Host "Starting QEMU (Ctrl+C to exit)..." -ForegroundColor Cyan
    & $QemuExe @qemuArgs
}
