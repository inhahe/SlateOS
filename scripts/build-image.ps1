# build-image.ps1 — Create a bootable UEFI disk image for QEMU.
#
# Creates a FAT32-formatted disk image containing:
#   - EFI/BOOT/BOOTX64.EFI  (Limine UEFI bootloader)
#   - boot/kernel            (our kernel ELF)
#   - boot/limine/limine.conf (bootloader config)
#
# Uses diskpart to create and format a VHD, then copies files.
# Must be run as Administrator (diskpart requires elevation).

param(
    [string]$ProjectRoot = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
    [string]$OutImage = ""
)

# If ProjectRoot wasn't resolved properly, use script location
if (-not (Test-Path "$ProjectRoot\kernel\Cargo.toml")) {
    $ProjectRoot = Split-Path -Parent $PSCommandPath | Split-Path -Parent
}

$ErrorActionPreference = "Stop"

# Paths
$KernelBin   = "$ProjectRoot\target\x86_64-unknown-none\debug\kernel"
$LimineDir   = "$ProjectRoot\limine"
$LimineConf  = "$ProjectRoot\limine.conf"
$BootX64     = "$LimineDir\BOOTX64.EFI"

if (-not $OutImage) {
    $OutImage = "$ProjectRoot\build\boot.img"
}

# Verify prerequisites
if (-not (Test-Path $KernelBin)) {
    Write-Error "Kernel binary not found at $KernelBin. Run 'cargo build' first."
    exit 1
}
if (-not (Test-Path $BootX64)) {
    Write-Error "BOOTX64.EFI not found at $BootX64. Ensure Limine is downloaded."
    exit 1
}
if (-not (Test-Path $LimineConf)) {
    Write-Error "limine.conf not found at $LimineConf."
    exit 1
}

# Create output directory
$BuildDir = Split-Path -Parent $OutImage
if (-not (Test-Path $BuildDir)) {
    New-Item -ItemType Directory -Path $BuildDir -Force | Out-Null
}

# Remove old image
if (Test-Path $OutImage) {
    Remove-Item $OutImage -Force
}

Write-Host "Creating 64MB FAT32 disk image..."

# Create a raw disk image and format it using diskpart VHD commands
$VhdPath = [System.IO.Path]::GetFullPath($OutImage)

# Create diskpart script to make a VHD
$dpScript = @"
create vdisk file="$VhdPath" maximum=64 type=fixed
select vdisk file="$VhdPath"
attach vdisk
create partition primary
format fs=fat32 quick label="BOOT"
assign
"@

$dpFile = "$BuildDir\diskpart_create.txt"
$dpScript | Out-File -FilePath $dpFile -Encoding ASCII

Write-Host "Running diskpart to create and format VHD..."
$result = diskpart /s $dpFile 2>&1
Write-Host $result

# Find the drive letter that was assigned
Start-Sleep -Seconds 2
$volumes = Get-Volume | Where-Object { $_.FileSystemLabel -eq "BOOT" -and $_.DriveType -eq "Removable" }
if (-not $volumes) {
    # Try again with different criteria
    $volumes = Get-Volume | Where-Object { $_.FileSystemLabel -eq "BOOT" }
}

if (-not $volumes) {
    Write-Error "Could not find the mounted VHD volume. Check diskpart output above."
    exit 1
}

$driveLetter = $volumes[0].DriveLetter
Write-Host "VHD mounted at drive ${driveLetter}:"

# Copy files
Write-Host "Copying boot files..."

# EFI bootloader
$efiDir = "${driveLetter}:\EFI\BOOT"
New-Item -ItemType Directory -Path $efiDir -Force | Out-Null
Copy-Item $BootX64 "$efiDir\BOOTX64.EFI" -Force

# Kernel
$bootDir = "${driveLetter}:\boot"
New-Item -ItemType Directory -Path $bootDir -Force | Out-Null
Copy-Item $KernelBin "$bootDir\kernel" -Force

# Limine config
$limineDir2 = "${driveLetter}:\boot\limine"
New-Item -ItemType Directory -Path $limineDir2 -Force | Out-Null
Copy-Item $LimineConf "$limineDir2\limine.conf" -Force
# Also copy to root for older Limine versions
Copy-Item $LimineConf "${driveLetter}:\limine.conf" -Force

Write-Host "Files copied successfully."

# Detach the VHD
$dpDetach = @"
select vdisk file="$VhdPath"
detach vdisk
"@

$dpDetachFile = "$BuildDir\diskpart_detach.txt"
$dpDetach | Out-File -FilePath $dpDetachFile -Encoding ASCII
diskpart /s $dpDetachFile 2>&1 | Out-Null

# Clean up temp files
Remove-Item $dpFile -Force -ErrorAction SilentlyContinue
Remove-Item $dpDetachFile -Force -ErrorAction SilentlyContinue

Write-Host ""
Write-Host "Boot image created: $OutImage"
Write-Host "Run with: qemu-system-x86_64 -bios <ovmf-path> -drive file=$OutImage,format=raw"
