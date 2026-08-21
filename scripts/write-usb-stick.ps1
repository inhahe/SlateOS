<#
.SYNOPSIS
    Write build/slateos-usb.img to a USB flash drive, with guards.

.DESCRIPTION
    Writing a raw image to the wrong disk destroys that disk's partition table
    and is not undoable.  It is the one genuinely irreversible step in the
    bare-metal boot path, so this script is built around refusing to be the
    thing that does it:

      * It lists candidate disks and will only ever target BusType 'USB'.
        An internal NVMe/SATA disk cannot be selected at all -- not by disk
        number, not by typo, not by -Force.
      * It refuses a disk that Windows reports as the system or boot disk.
      * It requires you to retype the target's FriendlyName exactly.  A disk
        number is one keystroke from another disk; a model string is not.
      * It refuses a disk larger than -MaxSizeGB (default 128) unless you pass
        -AllowLargeDisk, because a 2 TB "USB" device is far more likely to be
        an external backup drive than a boot stick.

    Nothing here touches the Windows installation.  SlateOS is booted by
    choosing the stick in the firmware's boot menu; no bootloader is installed
    to the internal disk and no boot entry is added.  Recovery from a failed
    boot is: power off, remove the stick, power on.

.PARAMETER Image
    Image to write.  Defaults to build/slateos-usb.img next to this script.

.PARAMETER DiskNumber
    Target disk number.  Omit to list candidates and stop.

.PARAMETER MaxSizeGB
    Reject USB disks larger than this (default 128).

.PARAMETER AllowLargeDisk
    Permit a USB disk larger than -MaxSizeGB.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/write-usb-stick.ps1
    # lists USB disks and exits without writing anything

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/write-usb-stick.ps1 -DiskNumber 2
#>

[CmdletBinding()]
param(
    [string]$Image,
    [int]$DiskNumber = -1,
    [int]$MaxSizeGB = 128,
    [switch]$AllowLargeDisk
)

$ErrorActionPreference = 'Stop'

if (-not $Image) {
    $Image = Join-Path (Split-Path -Parent $PSScriptRoot) 'build\slateos-usb.img'
}

if (-not (Test-Path -LiteralPath $Image)) {
    Write-Host "ERROR: image not found: $Image" -ForegroundColor Red
    Write-Host "       Build it first:  python scripts/build-usb-image.py"
    exit 1
}

$imageInfo = Get-Item -LiteralPath $Image
$imageSize = $imageInfo.Length

# Administrator is required to open \\.\PhysicalDriveN for writing.  Checked up
# front rather than allowed to surface as an "Access is denied" halfway through.
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Host "ERROR: this must run in an elevated PowerShell." -ForegroundColor Red
    Write-Host "       Right-click PowerShell -> Run as administrator, then re-run."
    exit 1
}

$disks = Get-Disk | Sort-Object Number

Write-Host ""
Write-Host "Image:  $Image"
Write-Host "        $([math]::Round($imageSize / 1MB, 1)) MiB, modified $($imageInfo.LastWriteTime)"
Write-Host ""
Write-Host "Disks on this machine:"
Write-Host ""
foreach ($d in $disks) {
    $flag = ''
    if ($d.IsSystem)   { $flag += ' [SYSTEM]' }
    if ($d.IsBoot)     { $flag += ' [BOOT]' }
    if ($d.BusType -ne 'USB') { $flag += ' [not USB - cannot be selected]' }
    '{0,3}  {1,-9} {2,9:N1} GB  {3}{4}' -f `
        $d.Number, $d.BusType, ($d.Size / 1GB), $d.FriendlyName, $flag | Write-Host
}
Write-Host ""

if ($DiskNumber -lt 0) {
    Write-Host "No -DiskNumber given, so nothing was written."
    Write-Host "Re-run with the number of the USB disk you want to overwrite, e.g.:"
    Write-Host "    -DiskNumber 2"
    exit 0
}

$target = $disks | Where-Object { $_.Number -eq $DiskNumber }
if (-not $target) {
    Write-Host "ERROR: no disk number $DiskNumber." -ForegroundColor Red
    exit 1
}

# --- The guards.  None of these is overridable except the size one. ---

if ($target.BusType -ne 'USB') {
    Write-Host "REFUSED: disk $DiskNumber is $($target.BusType), not USB." -ForegroundColor Red
    Write-Host "         This script only ever writes to USB-attached disks."
    exit 1
}
if ($target.IsSystem -or $target.IsBoot) {
    Write-Host "REFUSED: disk $DiskNumber is the system or boot disk." -ForegroundColor Red
    exit 1
}
if ($target.Size -lt $imageSize) {
    Write-Host ("REFUSED: disk $DiskNumber holds {0:N1} GB but the image needs {1:N1} GB." -f `
        ($target.Size / 1GB), ($imageSize / 1GB)) -ForegroundColor Red
    exit 1
}
if (($target.Size / 1GB) -gt $MaxSizeGB -and -not $AllowLargeDisk) {
    Write-Host ("REFUSED: disk $DiskNumber is {0:N0} GB, over the {1} GB limit." -f `
        ($target.Size / 1GB), $MaxSizeGB) -ForegroundColor Red
    Write-Host "         A USB disk this large is more likely an external drive than a"
    Write-Host "         boot stick.  If you are certain, re-run with -AllowLargeDisk."
    exit 1
}

Write-Host "About to ERASE disk $DiskNumber and write the SlateOS image to it:" -ForegroundColor Yellow
Write-Host ""
Write-Host "    $($target.FriendlyName)"
Write-Host ("    {0:N1} GB, bus $($target.BusType), $($target.PartitionStyle) partition table" -f ($target.Size / 1GB))
Write-Host ""
Write-Host "Everything on it will be lost.  To proceed, retype the model name exactly:" -ForegroundColor Yellow
$typed = Read-Host "Model"
if ($typed -ne $target.FriendlyName) {
    Write-Host "Names do not match; nothing was written." -ForegroundColor Green
    exit 1
}

# Take the disk offline first.  A mounted volume means Windows holds cached
# writes that can land AFTER ours and corrupt the filesystem we just wrote.
Write-Host ""
Write-Host "Taking disk $DiskNumber offline..."
Set-Disk -Number $DiskNumber -IsOffline $true
try {
    $handle = [System.IO.File]::Open(
        "\\.\PhysicalDrive$DiskNumber",
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::ReadWrite)
    try {
        $src = [System.IO.File]::OpenRead($Image)
        try {
            # 4 MiB buffer: raw physical-drive writes must be sector-aligned and
            # are dominated by per-request overhead, so a large multiple of the
            # sector size is both required and fastest.
            $buffer = New-Object byte[] (4MB)
            $written = 0
            $sw = [Diagnostics.Stopwatch]::StartNew()
            while ($true) {
                $n = $src.Read($buffer, 0, $buffer.Length)
                if ($n -le 0) { break }
                # Pad the final short read out to a full sector; a raw write of a
                # non-multiple length fails outright.
                if ($n % 512 -ne 0) {
                    $pad = 512 - ($n % 512)
                    [Array]::Clear($buffer, $n, $pad)
                    $n += $pad
                }
                $handle.Write($buffer, 0, $n)
                $written += $n
                Write-Progress -Activity "Writing $Image to disk $DiskNumber" `
                    -Status ("{0:N0} / {1:N0} MiB" -f ($written / 1MB), ($imageSize / 1MB)) `
                    -PercentComplete ([math]::Min(100, 100.0 * $written / $imageSize))
            }
            $handle.Flush()
            $sw.Stop()
            Write-Progress -Activity "Writing" -Completed
            Write-Host ("Wrote {0:N0} MiB in {1:N0}s ({2:N1} MiB/s)." -f `
                ($written / 1MB), $sw.Elapsed.TotalSeconds, ($written / 1MB / $sw.Elapsed.TotalSeconds))
        } finally { $src.Dispose() }
    } finally { $handle.Dispose() }
} finally {
    Write-Host "Bringing disk $DiskNumber back online..."
    Set-Disk -Number $DiskNumber -IsOffline $false
}

Write-Host ""
Write-Host "Done.  Next steps are in bare-metal-boot.md:" -ForegroundColor Green
Write-Host "  1. Turn Secure Boot OFF in firmware (Limine's BOOTX64.EFI is unsigned)."
Write-Host "  2. Boot the PC and pick the stick from the firmware boot menu."
Write-Host "  3. If anything goes wrong: power off, remove the stick, power on."
Write-Host "     Windows is untouched -- nothing was installed to the internal disk."
