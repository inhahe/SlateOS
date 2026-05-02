$fw = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"

Set-Location "D:\visual studio projects\os"

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $qemu

$diskImg = "D:\visual studio projects\os\disk.img"
$ext4Img = "D:\visual studio projects\os\build\ext4_test.img"
$swapImg = "D:\visual studio projects\os\build\swap.img"

# Build QEMU argument string.  Use q35 machine and cpu max for
# feature parity with run-qemu.ps1.  Virtio-blk devices for disk
# images, virtio-net for network.
$qemuArgs = "-drive `"if=pflash,format=raw,readonly=on,file=$fw`""
$qemuArgs += " -drive `"format=raw,file=fat:rw:esp`""
$qemuArgs += " -cpu max -machine q35 -m 256M -smp 2"
$qemuArgs += " -serial stdio -display none -no-reboot -no-shutdown"
if (Test-Path $diskImg) {
    $qemuArgs += " -drive `"file=$diskImg,if=virtio,format=raw`""
}
if (Test-Path $ext4Img) {
    $qemuArgs += " -drive `"file=$ext4Img,if=virtio,format=raw`""
}
if (Test-Path $swapImg) {
    $qemuArgs += " -drive `"file=$swapImg,if=virtio,format=raw`""
}

$psi.Arguments = $qemuArgs
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true

$proc = [System.Diagnostics.Process]::Start($psi)

Start-Sleep -Seconds 30

$output = $proc.StandardOutput.ReadToEnd()
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue

$output | Out-File -FilePath "serial-stdio.log" -Encoding UTF8
$output -split "`n" | Select-String "cow|dma|smp|tlb|io_sched|bench|BOOT_OK|ext4|FAIL|PANIC"
