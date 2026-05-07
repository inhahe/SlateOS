$fw = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"

Set-Location "D:\visual studio projects\os"

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $qemu

$diskImg = "D:\visual studio projects\os\disk.img"

$qemuArgs = "-drive `"if=pflash,format=raw,readonly=on,file=$fw`""
$qemuArgs += " -drive `"format=raw,file=fat:rw:esp`""
$qemuArgs += " -cpu max -machine q35 -m 256M"
$qemuArgs += " -serial stdio -display none -no-reboot -no-shutdown"
if (Test-Path $diskImg) {
    $qemuArgs += " -drive `"file=$diskImg,if=virtio,format=raw`""
}

$psi.Arguments = $qemuArgs
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true

$proc = [System.Diagnostics.Process]::Start($psi)

Start-Sleep -Seconds 60

$output = $proc.StandardOutput.ReadToEnd()
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue

$output | Out-File -FilePath "serial-stdio.log" -Encoding UTF8

if ($output -match "BOOT_OK") {
    Write-Host "PASS: BOOT_OK found"
    $output -split "`n" | Select-String "iommu"
    exit 0
} elseif ($output -match "KERNEL PANIC") {
    Write-Host "FAIL: KERNEL PANIC"
    $output -split "`n" | Select-String "PANIC|panic"
    exit 1
} else {
    Write-Host "TIMEOUT: no BOOT_OK within 60s"
    $lines = $output -split "`n"
    $lines[-20..-1]
    exit 2
}
