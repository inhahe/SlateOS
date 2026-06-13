$p = Start-Process -FilePath 'C:\Program Files\qemu\qemu-system-x86_64.exe' `
    -ArgumentList '-drive','format=raw,file=target/x86_64-unknown-none/debug/disk.img','-serial','stdio','-display','none','-m','256M','-no-reboot','-no-shutdown' `
    -NoNewWindow -PassThru -RedirectStandardOutput serial_out.txt -RedirectStandardError serial_err.txt
if (-not $p.WaitForExit(30000)) {
    Stop-Process -Id $p.Id -Force
    Write-Host 'TIMEOUT after 30s'
}
Get-Content serial_out.txt
