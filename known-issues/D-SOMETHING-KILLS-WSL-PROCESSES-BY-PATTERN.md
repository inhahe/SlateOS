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

**What would close it:** finding the sender -- most likely a session or
script that cleans up "its" WSL processes with `pkill -f <pattern>` or
similar, which the operator's rules forbid ("kill only the exact process you
started, by PID"). Until then, a long WSL step should survive one kill:
lane D's pipeline script (`build/pipeline.sh`, local and not in the
tree) runs the CMake step a second time when the first dies.
