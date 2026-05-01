$fw = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"

Set-Location "D:\visual studio projects\os"
Remove-Item serial.log -ErrorAction SilentlyContinue

$proc = Start-Process -FilePath $qemu -ArgumentList @(
    "-drive", "`"if=pflash,format=raw,readonly=on,file=$fw`"",
    "-drive", "format=raw,file=fat:rw:esp",
    "-m", "256M",
    "-serial", "file:serial.log",
    "-display", "none",
    "-no-reboot",
    "-no-shutdown"
) -NoNewWindow -PassThru

Start-Sleep -Seconds 45
Stop-Process -Name "qemu-system-x86_64" -Force -ErrorAction SilentlyContinue
Start-Sleep 1

if (Test-Path serial.log) {
    Get-Content serial.log | Select-String "syscall|BOOT_OK|Console write"
} else {
    Write-Host "ERROR: serial.log not created"
}
