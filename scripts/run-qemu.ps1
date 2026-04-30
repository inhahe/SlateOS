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
    [int]$Memory = 128
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$KernelBin   = "$ProjectRoot\target\x86_64-unknown-none\debug\kernel"
$EspDir      = "$ProjectRoot\build\esp"
$QemuExe     = "C:\Program Files\qemu\qemu-system-x86_64.exe"
$OvmfFw      = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"

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
$qemuArgs = @(
    "-drive", "if=pflash,format=raw,readonly=on,file=$OvmfFw",
    "-drive", "format=raw,file=fat:rw:$EspDir",
    "-m", "${Memory}M",
    "-machine", "q35",
    "-no-reboot"
)

if ($Test) {
    # Boot test mode: capture serial to file, check for BOOT_OK
    $serialFile = "$ProjectRoot\build\serial-test.txt"
    $qemuArgs += @("-serial", "file:$serialFile", "-display", "none")

    Write-Host "Running boot test (timeout: ${Timeout}s)..." -ForegroundColor Cyan
    $proc = Start-Process -FilePath $QemuExe -ArgumentList $qemuArgs -PassThru -NoNewWindow
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
    # Interactive mode: serial to console
    $qemuArgs += @("-serial", "stdio")
    Write-Host "Starting QEMU (Ctrl+C to exit)..." -ForegroundColor Cyan
    & $QemuExe @qemuArgs
}
