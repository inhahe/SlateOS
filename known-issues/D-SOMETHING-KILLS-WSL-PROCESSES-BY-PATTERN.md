## D-SOMETHING-KILLS-WSL-PROCESSES-BY-PATTERN — something outside the tree sends SIGTERM to WSL processes by a pattern, and a lane's build dies with them (lane D, 2026-10-05)

**Status:** OPEN — the sender is not found; lane D's pipeline retries its CMake step meanwhile.

**In short:** twice on 2026-10-05 a long build running inside WSL -- lane D's
cross build of CMake, part of its pipeline before a boot test -- died
mid-compile with every compiler "Terminated" (SIGTERM, exit 143), while WSL
itself kept running. The second time, the system journal shows two unrelated
Python web servers in the same distribution stopped in the same second, by
no request of systemd's. Something on this machine kills WSL processes by a
pattern broad enough to take a build with it. It is not in this tree.

**What was measured:**

- 00:46: the CMake build's compiles "Error 143"; the WSL kernel had been up
  since 00:39:51 (`/proc/uptime`), so no shutdown. The journal from then has
  since rotated away.
- 07:08:53: the compiles "Terminated" (`cmake-spike/build.log`); WSL up since
  06:55. `journalctl --since 07:08:45 --until 07:09:00`: `uvicorn[234]:
  Shutting down` (stereogram.service) and ratemyidea.service's gunicorn
  exiting, in that second, with no "Stopping ..." line from systemd before
  either; systemd restarted ratemyidea three seconds later. Lane F's six
  libFuzzer processes, running since 07:03, were not touched (lane F
  measured; its tree does neither of these things).
- In this tree, nothing runs `wsl --shutdown` or `--terminate`, nothing
  kills by name, `scripts/pgrep-diff.sh` runs its `pkill` cases in their own
  user, mount and PID namespaces, and `run-timeout.py` says in its docs that
  its tree-kill stops at the WSL boundary.

**Not the cause, though it also kills builds:** the distribution shuts down
when no `wsl.exe` is attached (lane F counted 20 shutdowns from 03:04 to
07:15, `journalctl --list-boots`), which SIGTERMs whatever is left -- a
build backgrounded with `nohup` or `&` after its `wsl.exe` exits. Neither
kill above sat on a boot boundary, and lane D's pipeline keeps its `wsl.exe`
attached for the whole step. `instanceIdleTimeout=-1` in `.wslconfig` would
end that kind; it is the operator's file.

**2026-10-07, again -- three workloads in one second.** At 16:03:49 the
same SIGTERM ended, in one second: lane D's cross GCC build (`make
all-target-libstdc++-v3`, inside libstdc++'s `configure`, exit 143), lane
D's round-200 pipeline's CMake build (`gmake ... Terminated`; its retry ran
it again), and `ratemyidea.service`'s gunicorn, which systemd restarted
four seconds later. The bash scripts running those builds -- `cross.sh`,
`cmake-spike/run.sh`, and the runner above them -- were *not* killed: the
sender picks processes by what they are, not by user or session, and not
with `kill -1`. WSL had been up since 14:32 (`/proc/uptime`), 30 GB of 32
were free with swap untouched, and no out-of-memory daemon runs (earlyoom,
systemd-oomd, nohang: all inactive), so it is neither a shutdown nor memory
pressure. The journal shows the web services stopped the same way at 14:19,
14:20, 14:22, 14:23 (WSL restarted after those), 15:21:50 (`stereogram` and
`ratemyidea` together) and 16:03:49. No systemd timer or crontab entry
matches those times.

**17:06:56, and it comes in bursts.** Again `ratemyidea`'s gunicorn and lane
D's native GCC build (`make all-gcc`, inside MPFR's `configure`) -- and this
time the build's immediate retry, started within the second, and the next
two `make`s after it (`all-target-libgcc`, `all-target-libstdc++-v3`), each
with its own immediate retry, were all killed too: six runs ended in the same
second. The sender keeps killing for at least a second or two, so a retry has
to wait it out. The interval since the last is about an hour (15:21:50,
16:03:49, 17:06:56), which suggests something scheduled outside WSL, where
`systemctl list-timers` and the crontabs cannot see it. `scripts/pgrep-diff.sh`
was checked again and is not it: each of its `pkill` cases runs with a
synthetic `/proc` bound over the real one, in its own PID and user namespaces,
so neither what it sees nor what it signals is outside.

**What would close it:** finding the sender -- most likely a session or
script that cleans up "its" WSL processes with `pkill -f <pattern>` or
similar, which the operator's rules forbid ("kill only the exact process you
started, by PID"). The way to name it is the kernel's own record of every
SIGTERM sent, which needs root in WSL (`sudo` asks for a password, so it is
the operator's step): `sudo apt install auditd`, then `sudo auditctl -a
always,exit -F arch=b64 -S kill,tkill,tgkill -F a1=15 -k sigterm`; after the
next occurrence, `sudo ausearch -k sigterm -i` gives the sending process,
its parent and its command line.

**Meanwhile, long WSL steps survive it:** lane D's pipeline script
(`build/pipeline.sh`, local and not in the tree) runs the CMake step a
second time when the first dies, and since 2026-10-07 the GCC spike runs
each `make` through `slate_resume_if_killed` (`scripts/lib/worktree.sh`):
after an outside SIGTERM it waits 30 seconds for the burst to pass and runs
the step again, three times at most. make deletes a target a fatal signal
interrupted, so each run redoes only what was lost.
