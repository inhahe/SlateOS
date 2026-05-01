$fw = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"

$cwd = "D:\visual studio projects\os"
Set-Location $cwd
Remove-Item serial.log -ErrorAction SilentlyContinue

# Copy freshly built kernel to ESP so QEMU picks up the latest binary.
# Prefer release build (much faster in QEMU, 12x smaller), fall back to debug.
$kernelBin = "$cwd\target\x86_64-unknown-none\release\kernel"
if (-not (Test-Path $kernelBin)) {
    $kernelBin = "$cwd\target\x86_64-unknown-none\debug\kernel"
}
if (Test-Path $kernelBin) {
    Copy-Item $kernelBin "$cwd\esp\boot\kernel" -Force
}

$proc = Start-Process -FilePath $qemu -ArgumentList @(
    "-drive", "`"if=pflash,format=raw,readonly=on,file=$fw`"",
    "-drive", "format=raw,file=fat:rw:esp",
    "-m", "256M",
    "-smp", "2",
    "-cpu", "qemu64,+rdtscp",
    "-serial", "file:serial.log",
    "-display", "none",
    "-no-reboot",
    "-no-shutdown"
) -NoNewWindow -PassThru

# Benchmarks + ISR measurement take ~10-15s, plus boot self-tests.
# Release build with all tests: ~20s. Give 60s margin for TCG slowness.
Start-Sleep -Seconds 60
Stop-Process -Name "qemu-system-x86_64" -Force -ErrorAction SilentlyContinue
Start-Sleep 2

if (Test-Path serial.log) {
    Get-Content serial.log | Select-String "cow|dma|smp|tlb|io_sched|bench|BOOT_OK"
} else {
    Write-Host "ERROR: serial.log not created"
}
