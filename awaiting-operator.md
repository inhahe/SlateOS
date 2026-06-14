# Awaiting Operator — Participation / Go-Ahead Queue

Things that are **gated on the operator's input, participation, or explicit
go-ahead** but are **not design decisions**. Unlike `open-questions.md`, none of
these ask you to *choose* between options — they're items where I either want
your green light before committing significant effort, or where I literally
cannot proceed without you doing something (running hardware, providing an
asset, granting access, etc.).

This list does **not** necessarily block other work: when something here is
parked, I keep working on unblocked roadmap tasks. It exists so that when you
*do* have attention to spare, you can see what's waiting on you at a glance.

## How this differs from the other queues

| File | Holds |
|------|-------|
| `open-questions.md` | Design **decisions** needing your choice (architectural forks, user-visible policy, tradeoffs with no obvious answer). |
| **`awaiting-operator.md`** (this file) | Non-decision items needing your **go-ahead or participation** (green-light a big initiative, run something on real hardware, supply an asset/credential). |
| `todo.txt` | Deferred work **I can pick up myself** — no operator needed. |
| `known-issues.md` | Bugs and accumulated tech debt. |

When an item here is resolved (you give the go-ahead, or do the thing), I remove
it and either start the work or note where it went.

---

## Open items

### 1. Green-light for large multi-day "port" initiatives (e.g. Chromium)
- **What:** Before I sink days/weeks into a large external port (Chromium, a
  full Mesa GL stack, a JS engine, a large language runtime, etc.), I'd like
  your explicit go-ahead, since these reshape the roadmap and are expensive to
  unwind.
- **Why it needs you:** Not a design decision — more a prioritization/commitment
  call. These crowd out other roadmap work for a long stretch, so it should be a
  deliberate choice by you, not something I drift into autonomously.
- **Status / my read:** **Premature regardless of go-ahead.** Chromium
  specifically is blocked on hard prerequisites we don't have yet: a working
  graphics stack (compositor + GPU or at least a presented framebuffer), font
  rendering, a large slice of the Linux/POSIX ABI, networking with TLS, and a
  self-hosting or cross toolchain capable of building it. We're currently at the
  Linux-ABI-syscall + shell layer. So this is a "much later" item even with your
  blessing — I'm not parked waiting on you for it *right now*; I'm noting it so
  the policy ("ask before starting a giant port") is written down.
- **What unblocks it:** (a) the graphics/toolchain prerequisites land, **and**
  (b) you say "go." Until both, I won't start.

### 2. Real-hardware boot / driver validation
- **What:** Everything is currently validated in QEMU via `scripts/boot-test.sh`.
  Driver work (USB, real GPU, NIC, NVMe/AHCI on physical disks) eventually needs
  validation on actual hardware, which I can't do.
- **Why it needs you:** Requires you to flash an image to a USB stick / disk and
  boot a physical machine, then report what you see on serial/screen.
- **Status / my read:** Not blocking now — QEMU covers the current work. Becomes
  relevant once we have drivers whose behavior diverges between QEMU and real
  silicon. I'll flag specific drivers here when they reach that point.
- **What unblocks it:** You boot a build on real hardware and relay the serial
  log / observed behavior when I ask.
