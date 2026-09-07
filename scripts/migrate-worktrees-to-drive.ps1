# Migrate the SlateOS trees from one drive to another.
#
# Usage, with ALL Claude sessions stopped and no boot test running:
#     powershell -NoProfile -ExecutionPolicy Bypass -File D:\tmp\migrate-slateos.ps1 -Src "D:\visual studio projects" -Dst "E:\visual studio projects" -DryRun
#     powershell -NoProfile -ExecutionPolicy Bypass -File D:\tmp\migrate-slateos.ps1 -Src "D:\visual studio projects" -Dst "E:\visual studio projects"
#
# MOVES: os, os-lane-a, os-lane-b, os-lane-c -- the main repo and its three
# worktrees, including each lane's UNCOMMITTED work and the per-worktree state
# under os\.git\worktrees\ (boot-test logs, gate timings, indexes).
#
# DOES NOT MOVE: target\ directories. Cargo bakes absolute paths into its
# fingerprints, so a moved target\ is invalidated and rebuilt anyway. Copying
# it is pure cost and it is the overwhelming majority of the bytes.
#
# WHY A SCRIPT: these are git worktrees. os-lane-a\.git is a FILE holding an
# absolute path to os\.git\worktrees\os-lane-a, and that directory's gitdir
# file holds an absolute path back. A plain folder copy leaves six absolute
# paths aimed at the old drive and every lane's git breaks in a way that looks
# like repository corruption. "git worktree repair" rewrites both directions;
# this script runs it and then proves it worked.

param(
    [Parameter(Mandatory=$true)] [string] $Src,
    [Parameter(Mandatory=$true)] [string] $Dst,
    [string[]] $Trees = @("os", "os-lane-a", "os-lane-b", "os-lane-c"),
    [switch] $DryRun,
    [switch] $SkipProcessCheck
)

$ErrorActionPreference = "Stop"

# Native commands write progress and warnings to stderr, and with
# $ErrorActionPreference = "Stop" PowerShell turns any of that into a
# terminating NativeCommandError. git legitimately warns here (one lane has a
# symlinked directory git cannot traverse on Windows), so every git call goes
# through this, which restores Continue for the duration and hands back only
# stdout.
function Invoke-Git {
    param([Parameter(ValueFromRemainingArguments=$true)][string[]] $GitArgs)
    $old = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    try { $out = & git @GitArgs 2>$null } finally { $ErrorActionPreference = $old }
    return $out
}

function Fail($m) { Write-Host "REFUSING: $m" -ForegroundColor Red; exit 1 }
function Info($m) { Write-Host $m -ForegroundColor Cyan }
function Good($m) { Write-Host "  ok   $m" -ForegroundColor Green }

Info "=== Preflight ==="
if (-not (Test-Path $Src)) { Fail "source '$Src' does not exist" }

# 1. Nothing may be running against the tree. A copy taken while cargo writes
#    produces a torn target\ (excluded anyway) or, far worse, a torn .git index.
if (-not $SkipProcessCheck) {
    $busyNames = @("cargo","rustc","qemu-system-x86_64","git","rustfmt","clippy-driver")
    $busy = Get-Process -ErrorAction SilentlyContinue | Where-Object { $busyNames -contains $_.ProcessName }
    if ($busy) {
        $names = ($busy | Group-Object ProcessName | ForEach-Object { $_.Name + " x" + $_.Count }) -join ", "
        Fail "still running: $names. Stop every Claude session and any boot test first."
    }
    Good "no cargo/rustc/git/qemu/rustfmt running"
}

# 2. A held boot lock means a run is in flight, or died dirty.
$lock = Join-Path $Src "os\.git\slateos-boot-lock"
if (Test-Path $lock) { Fail "a boot lock is held at $lock" }
Good "no boot lock held"

# 3. Record the state the far side must reproduce exactly.
Info "Recording pre-migration state..."
$before = @{}
foreach ($t in $Trees) {
    $p = Join-Path $Src $t
    if (-not (Test-Path $p)) { Fail "tree '$t' not found under '$Src'" }
    $head   = (Invoke-Git -C $p --no-optional-locks rev-parse HEAD)
    $branch = (Invoke-Git -C $p --no-optional-locks branch --show-current)
    $dirty  = @(Invoke-Git -C $p --no-optional-locks status --porcelain).Count
    $before[$t] = [PSCustomObject]@{ Head = $head; Branch = $branch; Dirty = $dirty }
    Write-Host ("    {0,-12} {1} [{2}] dirty={3}" -f $t, $head.Substring(0,9), $branch, $dirty)
}

# 4. Space, measured over what we actually copy (target\ excluded).
Info "Measuring payload (excluding target\)..."
$payload = 0
foreach ($t in $Trees) {
    $p = Join-Path $Src $t
    $bytes = (Get-ChildItem -LiteralPath $p -Recurse -File -Force -ErrorAction SilentlyContinue |
              Where-Object { $_.FullName -notmatch '\target\' } |
              Measure-Object -Property Length -Sum).Sum
    if (-not $bytes) { $bytes = 0 }
    $payload += $bytes
    Write-Host ("    {0,-12} {1,8:N2} GB" -f $t, ($bytes/1GB))
}
$qual = Split-Path $Dst -Qualifier
$free = (Get-PSDrive $qual.TrimEnd(':')).Free
Info ("payload {0:N2} GB, free on {1} {2:N1} GB" -f ($payload/1GB), $qual, ($free/1GB))
if ($free -lt ($payload * 1.25)) { Fail "not enough room on $qual (want 25 percent headroom)" }
Good "destination has room"

if ($DryRun) { Info "DryRun: stopping before any copy."; exit 0 }

Info "=== Copying (robocopy, target\ excluded) ==="
New-Item -ItemType Directory -Force -Path $Dst | Out-Null
foreach ($t in $Trees) {
    $from = Join-Path $Src $t
    $to   = Join-Path $Dst $t
    Info ("  " + $t)
    # /E subdirs incl empty; /XD target skip build output; /XJ do not follow
    # junctions; /COPY:DAT data+attrs+times; /DCOPY:DAT dir times; /R:2 /W:2
    # fail fast on a locked file; /MT:8 threads; /NFL /NDL /NP quiet.
    robocopy $from $to /E /XD target /XJ /COPY:DAT /DCOPY:DAT /R:2 /W:2 /MT:8 /NFL /NDL /NP | Out-Null
    # robocopy: 0-7 success, 8+ real failure.
    if ($LASTEXITCODE -ge 8) { Fail "robocopy failed for $t (exit $LASTEXITCODE)" }
    Good ($t + " copied")
}

Info "=== Repairing worktree links ==="
# Run from the new main repo, naming the new worktree paths. This rewrites both
# sides: each worktree's .git file, and os\.git\worktrees\<name>\gitdir.
$mainRepo = Join-Path $Dst "os"
$wtPaths = @()
foreach ($t in $Trees) { if ($t -ne "os") { $wtPaths += (Join-Path $Dst $t) } }
Invoke-Git -C $mainRepo worktree repair @wtPaths
if ($LASTEXITCODE -ne 0) { Fail "git worktree repair failed" }
Good "worktree links repaired"

Info "=== Verifying (not done until this passes) ==="
$bad = 0
foreach ($t in $Trees) {
    $p = Join-Path $Dst $t
    $head   = (Invoke-Git -C $p --no-optional-locks rev-parse HEAD)
    $branch = (Invoke-Git -C $p --no-optional-locks branch --show-current)
    $dirty  = @(Invoke-Git -C $p --no-optional-locks status --porcelain).Count
    $b = $before[$t]
    if ($head -ne $b.Head) {
        Write-Host ("  FAIL {0} HEAD {1} != {2}" -f $t, $head, $b.Head) -ForegroundColor Red; $bad++
    } elseif ($branch -ne $b.Branch) {
        Write-Host ("  FAIL {0} branch {1} != {2}" -f $t, $branch, $b.Branch) -ForegroundColor Red; $bad++
    } elseif ($dirty -ne $b.Dirty) {
        Write-Host ("  FAIL {0} dirty {1} != {2} -- uncommitted work differs" -f $t, $dirty, $b.Dirty) -ForegroundColor Red; $bad++
    } else {
        Good ("{0,-12} {1} [{2}] dirty={3}" -f $t, $head.Substring(0,9), $branch, $dirty)
    }
}

# Every entry must now name the NEW location. A stale old path here is the
# exact failure this script exists to prevent.
$list = Invoke-Git -C $mainRepo worktree list
$stale = $list | Where-Object { $_ -notmatch [regex]::Escape($Dst) }
if ($stale) {
    Write-Host "  FAIL worktree list still points at the old location:" -ForegroundColor Red
    $stale | ForEach-Object { Write-Host ("        " + $_) -ForegroundColor Red }
    $bad++
} else {
    Good ("every worktree resolves under " + $Dst)
}

if ($bad -gt 0) { Fail "$bad verification failure(s). The SOURCE is untouched -- keep using it and investigate." }

Info ""
Info "Migration verified. The source is untouched; delete it only after a green"
Info "boot test from the new location."
Info ""
Info "Remaining manual steps this script cannot do:"
Info "  1. Restart each Claude session with its cwd under the new path."
Info "  2. Each lane runs one build to repopulate target\ (excluded above)."
Info "  3. Check ~\.claude\CLAUDE.md and os\CLAUDE.md for absolute path references."
