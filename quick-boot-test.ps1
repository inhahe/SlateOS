$fw = "C:\Program Files\qemu\share\edk2-x86_64-code.fd"
$qemu = "C:\Program Files\qemu\qemu-system-x86_64.exe"
$serial_log = "D:\visual studio projects\os\serial-test.log"

# Clean previous log
if (Test-Path $serial_log) { Remove-Item $serial_log }

# Use 1G RAM — 160MB debug kernel needs more than 256M for Limine to load it
$args_str = "-drive `"if=pflash,format=raw,readonly=on,file=$fw`" -drive `"format=raw,file=fat:rw:esp`" -cpu max -machine q35 -m 1G -serial `"file:$serial_log`" -display none -no-reboot -no-shutdown"

$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $qemu
$psi.Arguments = $args_str
$psi.UseShellExecute = $false
$psi.CreateNoWindow = $true

$proc = [System.Diagnostics.Process]::Start($psi)

# Wait up to 120 seconds, checking for BOOT_OK every 5 seconds
$elapsed = 0
$found = $false
while ($elapsed -lt 120) {
    Start-Sleep -Seconds 5
    $elapsed += 5
    if (Test-Path $serial_log) {
        $content = Get-Content $serial_log -Raw -ErrorAction SilentlyContinue
        if ($content -match "BOOT_OK") {
            $found = $true
            break
        }
        if ($content -match "KERNEL PANIC") {
            Write-Host "FAIL: KERNEL PANIC at ${elapsed}s"
            $content -split "`n" | Select-String "PANIC|panic" | Select-Object -First 10 | ForEach-Object { $_.Line.Trim() }
            Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
            exit 1
        }
    }
    Write-Host "Waiting... ${elapsed}s"
}

Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue

if ($found) {
    Write-Host "PASS: BOOT_OK found at ${elapsed}s"
    $content = Get-Content $serial_log -Raw
    $content -split "`n" | Select-String "self-test|Self-test|BOOT_OK|PASSED|container" | ForEach-Object { $_.Line.Trim() }
} else {
    Write-Host "TIMEOUT: no BOOT_OK within 120s"
    if (Test-Path $serial_log) {
        $content = Get-Content $serial_log -Raw
        $lines = $content -split "`n"
        Write-Host "Total lines: $($lines.Length)"
        if ($lines.Length -gt 20) {
            $lines[-20..-1] | ForEach-Object { Write-Host $_ }
        } else {
            $lines | ForEach-Object { Write-Host $_ }
        }
    } else {
        Write-Host "No serial log file created"
    }
}
