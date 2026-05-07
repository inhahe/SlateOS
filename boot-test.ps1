$fw = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
$cwd = "D:\visual studio projects\os"

Set-Location $cwd
Remove-Item serial.log -ErrorAction SilentlyContinue

# Copy freshly built kernel to ESP so QEMU picks up the latest binary.
# Prefer release build (smaller, faster boot, per CLAUDE.md workaround for
# debug binary being too large for Limine at lower memory sizes).
$kernelBin = "$cwd\target\x86_64-unknown-none\release\kernel"
if (-not (Test-Path $kernelBin)) {
    $kernelBin = "$cwd\target\x86_64-unknown-none\debug\kernel"
}
if (Test-Path $kernelBin) {
    Copy-Item $kernelBin "$cwd\esp\boot\kernel" -Force
}

# NOTE: WHPX (Windows Hypervisor Platform) is available on this system
# but causes a kernel hang during early boot (WHPX doesn't fully
# support our use of XSAVE / MSR writes).  Using TCG (software
# emulation) instead.  Benchmark numbers under TCG are ~5-10x higher
# than bare metal due to emulated CLI/STI VM exits — keep this in mind
# when comparing against baselines.toml targets.
$proc = Start-Process -FilePath $qemu -ArgumentList @(
    "-drive", "`"if=pflash,format=raw,readonly=on,file=$fw`"",
    "-drive", "format=raw,file=fat:rw:esp",
    "-drive", "format=raw,if=virtio,file=disk.img",
    "-m", "256M",
    "-serial", "file:serial.log",
    "-display", "none",
    "-no-reboot",
    "-no-shutdown"
) -NoNewWindow -PassThru

Start-Sleep -Seconds 180
Stop-Process -Name "qemu-system-x86_64" -Force -ErrorAction SilentlyContinue

if (Test-Path serial.log) {
    Get-Content serial.log
} else {
    Write-Host "ERROR: serial.log not created"
}
