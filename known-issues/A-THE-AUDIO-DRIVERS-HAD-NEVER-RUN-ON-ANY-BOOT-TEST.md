## A-THE-AUDIO-DRIVERS-HAD-NEVER-RUN-ON-ANY-BOOT-TEST (lane A, 2026-08-17) - **fixed**

**In short:** the boot test starts a virtual PC with a disk, a network card and
a graphics card attached, and no sound card of any kind. Three sound drivers
(`ac97.rs`, `hda.rs`, `virtio/sound.rs`) had therefore never executed a single
line past "is there a device?" - not once, on any of the twenty boot tests
before this one. They were being changed and "verified" purely by the compiler.
Attaching the sound hardware cost **2 seconds** of boot time (345 s vs 343 s)
and immediately surfaced two real bugs, both filed separately below.

### What was untested

`scripts/boot-test.sh` builds the QEMU command line. It deliberately attaches
`-device ati-vga,model=rv100` with a comment explaining why:

> It is here so the ATI driver's register offsets are checked against a device
> on every boot instead of being trusted.

That reasoning applies verbatim to audio, and audio had never had it. The Q24
lock-discipline batch restructured `play_test_tone` in all three drivers, and
the restructured code could not have been executed by the test suite even in
principle - every one of them takes an early `return` at device discovery.

### The fix

`scripts/boot-test.sh` now attaches all three:

```
-audiodev none,id=snd0 \
-device AC97,audiodev=snd0 \
-device intel-hda \
-device hda-duplex,audiodev=snd0 \
-device virtio-sound-pci,audiodev=snd0,streams=2 \
```

Three details that are easy to get wrong and are recorded in the script's
comment block:

- **`audiodev=none` is the right backend, not a cop-out.** It discards the
  samples but leaves the *device model* - the register file, the DMA engine,
  the descriptor walk - completely intact. That model is the thing under test;
  the host's speakers are not.
- **`hda-duplex` is mandatory, not decoration.** `intel-hda` alone is a
  controller with no codec on the link, so `STATESTS` reads 0, the driver
  correctly concludes there is nothing to talk to, and the entire CORB/RIRB
  verb round-trip - the part most likely to be wrong - stays untested.
- **These are multimedia-class devices**, so unlike `ati-vga` they suppress
  nothing else on the bus.

### What it bought immediately

First-ever execution of: the HDA codec probe, the AFG (audio function group)
node walk, output-stream configuration, and the AC97 tone path. The AC97 path
reported "Playback complete" rather than "ended early", which is a real
validation of the Q24 `play_gen` ABA guard - the generation counter that stops
a stale timer callback from cancelling a *newer* tone.

### Generalisation - the rule this is an instance of

A driver whose device is absent from the boot test is not "lightly tested", it
is **untested**, and no amount of code review substitutes. Before restructuring
any driver, check whether the boot test attaches its hardware; if it does not,
attach it first and let the boot test tell you what is broken. The cost here
was two seconds and the yield was two bugs.
