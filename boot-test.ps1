$fw = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
$cwd = "D:\visual studio projects\os"

Set-Location $cwd
Remove-Item serial.log -ErrorAction SilentlyContinue

# Copy freshly built kernel to ESP so QEMU picks up the latest binary.
$kernelBin = "$cwd\target\x86_64-unknown-none\debug\kernel"
if (Test-Path $kernelBin) {
    Copy-Item $kernelBin "$cwd\esp\boot\kernel" -Force
}

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

Start-Sleep -Seconds 25
Stop-Process -Name "qemu-system-x86_64" -Force -ErrorAction SilentlyContinue

if (Test-Path serial.log) {
    Get-Content serial.log
} else {
    Write-Host "ERROR: serial.log not created"
}
