//! `<linux/vt.h>` — Linux virtual-terminal (`/dev/tty*`) ioctls.
//!
//! getty, kbd/console-setup, X.Org, and the kernel's own dm-crypt
//! `ask-password` flow drive the text-mode VT subsystem with these
//! ioctls: query mode, switch VT, allocate, deallocate, set keyboard
//! LED state, and so on.

// ---------------------------------------------------------------------------
// VT count
// ---------------------------------------------------------------------------

/// Maximum number of virtual terminals (`tty1`..`tty63`).
pub const MAX_NR_CONSOLES: u32 = 63;
/// Maximum number of user keymaps (counts U_GETKEYS).
pub const MAX_NR_USER_CONSOLES: u32 = 63;

// ---------------------------------------------------------------------------
// VT ioctls — group letter is implicit (high-byte 'V' for KD subset).
// ---------------------------------------------------------------------------

/// `VT_OPENQRY` — find a free VT (returns its number in *arg).
pub const VT_OPENQRY: u32 = 0x5600;
/// `VT_GETMODE` — read the current vt_mode struct.
pub const VT_GETMODE: u32 = 0x5601;
/// `VT_SETMODE` — set the vt_mode struct (auto vs process).
pub const VT_SETMODE: u32 = 0x5602;
/// `VT_GETSTATE` — read vt_stat (active+signal mask).
pub const VT_GETSTATE: u32 = 0x5603;
/// `VT_SENDSIG` — send a signal to all processes on a VT.
pub const VT_SENDSIG: u32 = 0x5604;
/// `VT_RELDISP` — release/acquire display (process mode).
pub const VT_RELDISP: u32 = 0x5605;
/// `VT_ACTIVATE` — switch to the given VT.
pub const VT_ACTIVATE: u32 = 0x5606;
/// `VT_WAITACTIVE` — wait until the given VT becomes active.
pub const VT_WAITACTIVE: u32 = 0x5607;
/// `VT_DISALLOCATE` — free a VT.
pub const VT_DISALLOCATE: u32 = 0x5608;
/// `VT_RESIZE` — resize the VT (cols/rows).
pub const VT_RESIZE: u32 = 0x5609;
/// `VT_RESIZEX` — resize with explicit pixel dims.
pub const VT_RESIZEX: u32 = 0x560a;
/// `VT_LOCKSWITCH` — disable VT switching.
pub const VT_LOCKSWITCH: u32 = 0x560b;
/// `VT_UNLOCKSWITCH` — re-enable VT switching.
pub const VT_UNLOCKSWITCH: u32 = 0x560c;
/// `VT_GETHIFONTMASK` — query high-font mask.
pub const VT_GETHIFONTMASK: u32 = 0x560d;
/// `VT_WAITEVENT` — wait for a VT event (struct vt_event).
pub const VT_WAITEVENT: u32 = 0x560e;
/// `VT_SETACTIVATE` — switch + claim atomically.
pub const VT_SETACTIVATE: u32 = 0x560f;

// ---------------------------------------------------------------------------
// vt_mode.mode
// ---------------------------------------------------------------------------

/// Kernel handles VT switching automatically.
pub const VT_AUTO: u8 = 0x00;
/// Owner process must Ack switches via VT_RELDISP.
pub const VT_PROCESS: u8 = 0x01;
/// Ack mode (used internally by the driver).
pub const VT_ACKACQ: u8 = 0x02;

// ---------------------------------------------------------------------------
// VT_RELDISP arguments
// ---------------------------------------------------------------------------

/// VT_RELDISP arg: refuse switch (stay on this VT).
pub const VT_RELDISP_REFUSE: u32 = 0;
/// VT_RELDISP arg: ack the switch-away.
pub const VT_RELDISP_ACK_AWAY: u32 = 1;
/// VT_RELDISP arg: ack the switch-back.
pub const VT_RELDISP_ACK_BACK: u32 = 2;

// ---------------------------------------------------------------------------
// vt_event.event bits
// ---------------------------------------------------------------------------

/// Switch event.
pub const VT_EVENT_SWITCH: u32 = 0x0001;
/// New console allocated.
pub const VT_EVENT_BLANK: u32 = 0x0002;
/// Unblank event.
pub const VT_EVENT_UNBLANK: u32 = 0x0004;
/// VT resized.
pub const VT_EVENT_RESIZE: u32 = 0x0008;
/// Mask of every defined event bit.
pub const VT_EVENT_MAX: u32 = 0x000f;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_max_consoles() {
        // Historical max is 63 (vt1..vt63; vt0 is the current VT alias).
        assert_eq!(MAX_NR_CONSOLES, 63);
        assert_eq!(MAX_NR_USER_CONSOLES, 63);
    }

    #[test]
    fn test_ioctls_distinct_and_use_v_group() {
        let v = [
            VT_OPENQRY,
            VT_GETMODE,
            VT_SETMODE,
            VT_GETSTATE,
            VT_SENDSIG,
            VT_RELDISP,
            VT_ACTIVATE,
            VT_WAITACTIVE,
            VT_DISALLOCATE,
            VT_RESIZE,
            VT_RESIZEX,
            VT_LOCKSWITCH,
            VT_UNLOCKSWITCH,
            VT_GETHIFONTMASK,
            VT_WAITEVENT,
            VT_SETACTIVATE,
        ];
        for i in 0..v.len() {
            for j in (i + 1)..v.len() {
                assert_ne!(v[i], v[j]);
            }
            // VT ioctls use the 0x5600 prefix (the historical "no-IOC"
            // form — they predate _IO/_IOR/_IOW).
            assert_eq!(v[i] & 0xff00, 0x5600);
        }
    }

    #[test]
    fn test_mode_dense() {
        assert_eq!(VT_AUTO, 0);
        assert_eq!(VT_PROCESS, 1);
        assert_eq!(VT_ACKACQ, 2);
    }

    #[test]
    fn test_reldisp_args_dense() {
        assert_eq!(VT_RELDISP_REFUSE, 0);
        assert_eq!(VT_RELDISP_ACK_AWAY, 1);
        assert_eq!(VT_RELDISP_ACK_BACK, 2);
    }

    #[test]
    fn test_event_bits_pow2_distinct() {
        let e = [
            VT_EVENT_SWITCH,
            VT_EVENT_BLANK,
            VT_EVENT_UNBLANK,
            VT_EVENT_RESIZE,
        ];
        for &b in &e {
            assert!(b.is_power_of_two());
        }
        for i in 0..e.len() {
            for j in (i + 1)..e.len() {
                assert_ne!(e[i], e[j]);
            }
        }
        // EVENT_MAX is the OR of every defined bit.
        assert_eq!(
            VT_EVENT_MAX,
            VT_EVENT_SWITCH | VT_EVENT_BLANK | VT_EVENT_UNBLANK | VT_EVENT_RESIZE
        );
    }
}
