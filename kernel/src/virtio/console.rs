//! Virtio console device driver (virtio device type 3), in its multiport form:
//! named byte channels between the host and a running guest.
//!
//! QEMU's `virtio-serial-pci`, one `virtserialport` per channel:
//!
//! ```text
//! -device virtio-serial-pci,max_ports=2
//! -chardev socket,id=agent,host=127.0.0.1,port=4555,server=on,wait=off
//! -device virtserialport,chardev=agent,name=org.slateos.agent.0
//! ```
//!
//! It is C-Q11 idea 1's channel (`requests/c-a-two-ways-to-test-a-change-without-a-full-boot.md`):
//! a way for the host into a guest that is already running -- no reboot, and
//! no network, which may be the very thing under test. A port carries bytes
//! both ways; what is said over one is for the layers above.
//!
//! ## The protocol (virtio 1.1 §5.3)
//!
//! With `VIRTIO_CONSOLE_F_MULTIPORT`, queues 2 and 3 carry control messages --
//! an 8-byte `{id, event, value}`, little-endian -- and port `n`'s receive and
//! transmit queues are `2n + 2` and `2n + 3` (port 0's are 0 and 1). The driver
//! says DEVICE_READY; the device announces each port with DEVICE_ADD; the
//! driver answers PORT_READY; the device names the port (PORT_NAME) and says
//! whether its host end is connected (PORT_OPEN); the driver says PORT_OPEN
//! when its own end is.
//!
//! ## What it does, and does not
//!
//! - Ports 1 to [`MAX_PORTS`]. Port 0 is QEMU's reserved console slot, which
//!   nothing here drives; a port past the limit is refused at PORT_READY. The
//!   limit is the descriptor pool's: `ada::MAX_QUEUES` is 16 queues for every
//!   virtio device together, and a port costs two.
//! - Polled, as every virtio driver here is: no interrupt. [`poll`] drains the
//!   control and receive queues, and every call below polls first.
//! - The guest end of a port opens as soon as the port is ready, and what the
//!   host sends is kept here until read, up to [`RX_LIMIT`] bytes a port. Past
//!   that, receive buffers are not given back, so the device holds the host's
//!   writes: flow control, not loss.
//! - A write to a port whose host end is not connected is refused
//!   (`NotConnected`): QEMU would discard it, and answering that it was
//!   written would be false.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::mm::frame::{self, FRAME_SIZE, PhysFrame};
use crate::pci::{self, PciDevice};
use crate::serial_println;
use crate::sync::Mutex;
use crate::virtio::modern::{ModernTransport, STATUS_DRIVER_OK, VIRTIO_VENDOR};
use crate::virtio::queue::{VRING_DESC_F_WRITE, Virtqueue};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Serial-log prefix, handed to the shared transport too.
const LOG_TAG: &str = "[virtio-con]";

/// The modern device ID (`0x1040 + 3`).
const DEVICE_ID_MODERN: u16 = 0x1043;
/// The transitional device ID, which also offers the modern capabilities.
const DEVICE_ID_TRANSITIONAL: u16 = 0x1003;

/// `VIRTIO_CONSOLE_F_MULTIPORT`: control queues and named ports.
const F_MULTIPORT: u32 = 1 << 1;

/// `max_nr_ports` in the device config (after `cols` and `rows`).
const CONFIG_MAX_NR_PORTS: usize = 4;

/// Control events (virtio 1.1 §5.3.6.2).
const EVENT_DEVICE_READY: u16 = 0;
const EVENT_DEVICE_ADD: u16 = 1;
const EVENT_DEVICE_REMOVE: u16 = 2;
const EVENT_PORT_READY: u16 = 3;
const EVENT_CONSOLE_PORT: u16 = 4;
const EVENT_RESIZE: u16 = 5;
const EVENT_PORT_OPEN: u16 = 6;
const EVENT_PORT_NAME: u16 = 7;

/// The control queues' indices.
const CTRL_RX_QUEUE: u16 = 2;
const CTRL_TX_QUEUE: u16 = 3;

/// The highest port this driver drives; ports are 1 to this.
pub const MAX_PORTS: u32 = 2;

/// A control message's header: `{le32 id, le16 event, le16 value}`.
const CONTROL_LEN: usize = 8;
/// Each control receive buffer: room for a PORT_NAME's header and its name.
const CTRL_RX_BUF: usize = 512;
/// Control receive buffers kept with the device.
const CTRL_RX_COUNT: usize = 8;
/// Each port receive buffer: a quarter of a frame.
const RX_BUF: usize = 4096;
/// Port receive buffers kept with the device: one frame's worth.
const RX_COUNT: usize = FRAME_SIZE / RX_BUF;
/// The most received bytes kept for a port until read.
pub const RX_LIMIT: usize = 1 << 20;
/// How long a control message or a write waits for the device to take it.
const SEND_TIMEOUT_NS: u64 = 1_000_000_000;
/// How long [`init`] waits for the device to announce and name its ports.
const SETTLE_NS: u64 = 200_000_000;

// The buffers are slices of single frames.
const _: () = assert!(CTRL_RX_BUF * CTRL_RX_COUNT <= FRAME_SIZE);
const _: () = assert!(RX_BUF * RX_COUNT <= FRAME_SIZE && RX_COUNT > 0);

// ---------------------------------------------------------------------------
// The protocol's pure parts
// ---------------------------------------------------------------------------

/// Port `id`'s receive and transmit queue indices.
#[must_use]
pub fn queue_indices(id: u32) -> Option<(u16, u16)> {
    if id == 0 {
        return Some((0, 1));
    }
    let rx = id.checked_mul(2)?.checked_add(2)?;
    let tx = rx.checked_add(1)?;
    Some((u16::try_from(rx).ok()?, u16::try_from(tx).ok()?))
}

/// A control message: its header, and what follows it (a PORT_NAME's name).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Control {
    id: u32,
    event: u16,
    value: u16,
    payload: Vec<u8>,
}

/// Read a control message the device wrote: `None` when it is shorter than
/// the header.
fn parse_control(bytes: &[u8]) -> Option<Control> {
    let id = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?);
    let event = u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?);
    let value = u16::from_le_bytes(bytes.get(6..8)?.try_into().ok()?);
    Some(Control {
        id,
        event,
        value,
        payload: bytes.get(CONTROL_LEN..).unwrap_or_default().to_vec(),
    })
}

/// A control message's header, for the device.
fn encode_control(id: u32, event: u16, value: u16) -> [u8; CONTROL_LEN] {
    let mut out = [0u8; CONTROL_LEN];
    let (id_part, rest) = out.split_at_mut(4);
    id_part.copy_from_slice(&id.to_le_bytes());
    let (event_part, value_part) = rest.split_at_mut(2);
    event_part.copy_from_slice(&event.to_le_bytes());
    value_part.copy_from_slice(&value.to_le_bytes());
    out
}

/// A PORT_NAME's name: the payload up to its NUL (QEMU sends one), or `None`
/// for an empty one.
fn port_name(payload: &[u8]) -> Option<Vec<u8>> {
    let end = payload
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(payload.len());
    let name = payload.get(..end)?;
    (!name.is_empty()).then(|| name.to_vec())
}

// ---------------------------------------------------------------------------
// Device state
// ---------------------------------------------------------------------------

/// One port the driver set queues up for.
struct Port {
    id: u32,
    rx_index: u16,
    tx_index: u16,
    rxq: Virtqueue,
    txq: Virtqueue,
    rx_frame: PhysFrame,
    tx_frame: PhysFrame,
    /// The descriptor head of each receive buffer the device holds, by slot.
    rx_posted: [Option<u16>; RX_COUNT],
    /// Received and not yet read.
    received: VecDeque<u8>,
    /// The device announced it (DEVICE_ADD) and was told it is ready.
    added: bool,
    /// Its name, from PORT_NAME.
    name: Option<Vec<u8>>,
    /// Whether its host end is connected (the device's PORT_OPEN).
    host_connected: bool,
}

/// The device.
struct Console {
    transport: ModernTransport,
    hhdm: u64,
    ctrl_rxq: Virtqueue,
    ctrl_txq: Virtqueue,
    ctrl_rx_frame: PhysFrame,
    ctrl_tx_frame: PhysFrame,
    ctrl_rx_posted: [Option<u16>; CTRL_RX_COUNT],
    ports: Vec<Port>,
    /// Queues the device has enabled that no port uses -- a port's receive
    /// queue whose transmit queue could not be had. A modern device cannot be
    /// told to forget one queue, so it is kept, never given a buffer, for as
    /// long as the device lives: freed, its memory would be the device's to
    /// read as rings.
    stranded: Vec<Virtqueue>,
}

// SAFETY: the virtqueues hold raw pointers into DMA memory reachable from any
// CPU through the HHDM; every access goes through `CONSOLE`'s lock.
unsafe impl Send for Console {}

/// The one virtio console this kernel drives.
static CONSOLE: Mutex<Option<Console>> = Mutex::named(None, b"VIRTIO_CON");

/// Whether [`init`] found and started one.
static PRESENT: AtomicBool = AtomicBool::new(false);

/// What a caller may know of a port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// Its number on the device.
    pub id: u32,
    /// Its name, once the device has given it one.
    pub name: Option<Vec<u8>>,
    /// Whether its host end is connected.
    pub host_connected: bool,
    /// Bytes received and not yet read.
    pub buffered: usize,
}

// ---------------------------------------------------------------------------
// DMA helpers
// ---------------------------------------------------------------------------

/// Copy `len` bytes out of `frame` at `offset`, or `None` past its end.
fn frame_read(frame: &PhysFrame, hhdm: u64, offset: usize, len: usize) -> Option<Vec<u8>> {
    let end = offset.checked_add(len)?;
    if end > FRAME_SIZE {
        return None;
    }
    let base = frame.addr().checked_add(hhdm)?;
    let mut out = alloc::vec![0u8; len];
    // SAFETY: `frame` is a whole frame this driver owns, mapped by the HHDM at
    // `addr + hhdm`; `offset..end` was checked to lie inside it. The device
    // may write it only while a buffer over it is posted, and the caller
    // reads a buffer only after the device has handed it back.
    unsafe {
        core::ptr::copy_nonoverlapping((base as *const u8).add(offset), out.as_mut_ptr(), len);
    }
    Some(out)
}

/// Copy `data` into `frame` at `offset`; `false` past its end.
fn frame_write(frame: &PhysFrame, hhdm: u64, offset: usize, data: &[u8]) -> bool {
    let Some(end) = offset.checked_add(data.len()) else {
        return false;
    };
    let Some(base) = frame.addr().checked_add(hhdm) else {
        return false;
    };
    if end > FRAME_SIZE {
        return false;
    }
    // SAFETY: as `frame_read`, writing: inside the frame, and no buffer over
    // it is posted while the caller fills it.
    unsafe {
        core::ptr::copy_nonoverlapping(data.as_ptr(), (base as *mut u8).add(offset), data.len());
    }
    true
}

/// Physical address of slot `slot` of `size` bytes in `frame`.
fn slot_phys(frame: &PhysFrame, slot: usize, size: usize) -> Option<u64> {
    let offset = u64::try_from(slot.checked_mul(size)?).ok()?;
    frame.addr().checked_add(offset)
}

/// Wait for the device to hand back descriptor `head` on `queue`, polling,
/// until [`SEND_TIMEOUT_NS`]; free it either way it ends.
fn wait_for(queue: &mut Virtqueue, head: u16) -> KernelResult<u32> {
    let deadline = crate::hrtimer::now_ns().saturating_add(SEND_TIMEOUT_NS);
    loop {
        if let Some((done, len)) = queue.poll_used() {
            queue.free_chain(done);
            if done == head {
                return Ok(len);
            }
            // Another completion on a queue with one buffer in flight at a
            // time: the device answering a descriptor it was not given. Freed
            // above, as the pool allows, and otherwise ignored.
            continue;
        }
        if crate::hrtimer::now_ns() >= deadline {
            serial_println!(
                "{} the device did not take descriptor {} in time",
                LOG_TAG,
                head
            );
            return Err(KernelError::TimedOut);
        }
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// The device, under its lock
// ---------------------------------------------------------------------------

impl Console {
    /// Send a control message and wait for the device to take it.
    fn send_control(&mut self, id: u32, event: u16, value: u16) -> KernelResult<()> {
        let message = encode_control(id, event, value);
        if !frame_write(&self.ctrl_tx_frame, self.hhdm, 0, &message) {
            return Err(KernelError::InternalError);
        }
        #[allow(clippy::cast_possible_truncation)] // eight
        let head = self
            .ctrl_txq
            .submit(&[(self.ctrl_tx_frame.addr(), CONTROL_LEN as u32, 0)])?;
        self.transport.notify_queue(CTRL_TX_QUEUE);
        wait_for(&mut self.ctrl_txq, head).map(|_| ())
    }

    /// Give control receive buffer `slot` to the device.
    fn post_control(&mut self, slot: usize) -> KernelResult<()> {
        let phys =
            slot_phys(&self.ctrl_rx_frame, slot, CTRL_RX_BUF).ok_or(KernelError::InternalError)?;
        #[allow(clippy::cast_possible_truncation)] // 512
        let head = self
            .ctrl_rxq
            .submit(&[(phys, CTRL_RX_BUF as u32, VRING_DESC_F_WRITE)])?;
        if let Some(posted) = self.ctrl_rx_posted.get_mut(slot) {
            *posted = Some(head);
        }
        Ok(())
    }

    /// Drain the control receive queue, acting on each message.
    fn poll_control(&mut self) {
        let mut reposted = false;
        while let Some((head, len)) = self.ctrl_rxq.poll_used() {
            let slot = self.ctrl_rx_posted.iter().position(|p| *p == Some(head));
            self.ctrl_rxq.free_chain(head);
            let Some(slot) = slot else {
                continue;
            };
            if let Some(posted) = self.ctrl_rx_posted.get_mut(slot) {
                *posted = None;
            }
            let len = usize::try_from(len).unwrap_or(CTRL_RX_BUF).min(CTRL_RX_BUF);
            let offset = slot.saturating_mul(CTRL_RX_BUF);
            let message = frame_read(&self.ctrl_rx_frame, self.hhdm, offset, len)
                .and_then(|bytes| parse_control(&bytes));
            if let Some(message) = message {
                self.on_control(&message);
            }
            if self.post_control(slot).is_ok() {
                reposted = true;
            }
        }
        if reposted {
            self.transport.notify_queue(CTRL_RX_QUEUE);
        }
    }

    /// Act on one control message from the device.
    fn on_control(&mut self, message: &Control) {
        let index = self.ports.iter().position(|p| p.id == message.id);
        match message.event {
            EVENT_DEVICE_ADD => {
                let Some(index) = index else {
                    serial_println!(
                        "{} port {} is past this driver's {}; refused",
                        LOG_TAG,
                        message.id,
                        MAX_PORTS
                    );
                    // Best effort: a refusal that does not arrive leaves the
                    // port as unused as one refused.
                    let _ = self.send_control(message.id, EVENT_PORT_READY, 0);
                    return;
                };
                if let Some(port) = self.ports.get_mut(index) {
                    port.added = true;
                }
                // Ready, and the guest end open: what the host sends is kept.
                let ready = self.send_control(message.id, EVENT_PORT_READY, 1);
                let opened = self.send_control(message.id, EVENT_PORT_OPEN, 1);
                if ready.is_err() || opened.is_err() {
                    serial_println!(
                        "{} port {}: the device did not take PORT_READY or PORT_OPEN",
                        LOG_TAG,
                        message.id
                    );
                }
            }
            EVENT_DEVICE_REMOVE => {
                if let Some(port) = index.and_then(|i| self.ports.get_mut(i)) {
                    port.added = false;
                    port.name = None;
                    port.host_connected = false;
                    port.received.clear();
                }
            }
            EVENT_PORT_NAME => {
                if let Some(port) = index.and_then(|i| self.ports.get_mut(i)) {
                    port.name = port_name(&message.payload);
                }
            }
            EVENT_PORT_OPEN => {
                if let Some(port) = index.and_then(|i| self.ports.get_mut(i)) {
                    port.host_connected = message.value != 0;
                }
            }
            // A console port's size, and which port is the console: nothing
            // here draws on one.
            EVENT_CONSOLE_PORT | EVENT_RESIZE => {}
            other => {
                serial_println!(
                    "{} unknown control event {} for port {}",
                    LOG_TAG,
                    other,
                    message.id
                );
            }
        }
    }

    /// Drain every port's receive queue into its buffer, giving back buffers
    /// while there is room.
    fn poll_ports(&mut self) {
        let hhdm = self.hhdm;
        for port in &mut self.ports {
            while let Some((head, len)) = port.rxq.poll_used() {
                let slot = port.rx_posted.iter().position(|p| *p == Some(head));
                port.rxq.free_chain(head);
                let Some(slot) = slot else {
                    continue;
                };
                if let Some(posted) = port.rx_posted.get_mut(slot) {
                    *posted = None;
                }
                let len = usize::try_from(len).unwrap_or(RX_BUF).min(RX_BUF);
                if let Some(bytes) =
                    frame_read(&port.rx_frame, hhdm, slot.saturating_mul(RX_BUF), len)
                {
                    port.received.extend(bytes);
                }
            }
        }
        for index in 0..self.ports.len() {
            self.refill(index);
        }
    }

    /// Give port `index`'s free receive buffers back to the device while it
    /// has room for what they could bring.
    fn refill(&mut self, index: usize) {
        let Some(port) = self.ports.get_mut(index) else {
            return;
        };
        let mut posted_any = false;
        for slot in 0..RX_COUNT {
            if port
                .received
                .len()
                .saturating_add(RX_BUF.saturating_mul(RX_COUNT))
                > RX_LIMIT
            {
                break;
            }
            if port.rx_posted.get(slot).copied().flatten().is_some() {
                continue;
            }
            let Some(phys) = slot_phys(&port.rx_frame, slot, RX_BUF) else {
                continue;
            };
            #[allow(clippy::cast_possible_truncation)] // 4096
            match port
                .rxq
                .submit(&[(phys, RX_BUF as u32, VRING_DESC_F_WRITE)])
            {
                Ok(head) => {
                    if let Some(posted) = port.rx_posted.get_mut(slot) {
                        *posted = Some(head);
                    }
                    posted_any = true;
                }
                // The queue is full: the rest wait for the next pass.
                Err(_) => break,
            }
        }
        if posted_any {
            let rx_index = port.rx_index;
            self.transport.notify_queue(rx_index);
        }
    }

    /// Everything the device has said, acted on.
    fn poll(&mut self) {
        self.poll_control();
        self.poll_ports();
    }

    /// Send `data` on port `index`, a frame at a time.
    fn write(&mut self, index: usize, data: &[u8]) -> KernelResult<usize> {
        let hhdm = self.hhdm;
        let Some(port) = self.ports.get_mut(index) else {
            return Err(KernelError::NoSuchDevice);
        };
        if !port.added {
            return Err(KernelError::NoSuchDevice);
        }
        if !port.host_connected {
            return Err(KernelError::NotConnected);
        }
        let mut written = 0usize;
        for chunk in data.chunks(FRAME_SIZE) {
            if !frame_write(&port.tx_frame, hhdm, 0, chunk) {
                return Err(KernelError::InternalError);
            }
            let len = u32::try_from(chunk.len()).map_err(|_| KernelError::InternalError)?;
            let head = port.txq.submit(&[(port.tx_frame.addr(), len, 0)])?;
            self.transport.notify_queue(port.tx_index);
            wait_for(&mut port.txq, head)?;
            written = written.saturating_add(chunk.len());
        }
        Ok(written)
    }
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Find a virtio console and start it: queues for its control channel and for
/// ports 1 to [`MAX_PORTS`] (as many as it has and the descriptor pool can
/// hold), then wait briefly for it to announce and name them.
///
/// # Errors
///
/// `NoSuchDevice` without one; `NotSupported` for one without
/// `VIRTIO_CONSOLE_F_MULTIPORT` (a bare console, which nothing here drives);
/// the transport's and the frame allocator's.
pub fn init(hhdm_offset: u64) -> KernelResult<()> {
    let dev = find_device()?;
    serial_println!(
        "{} Found device at {:02x}:{:02x}.{} (ID {:04x}:{:04x})",
        LOG_TAG,
        dev.address.bus,
        dev.address.device,
        dev.address.function,
        dev.vendor_id,
        dev.device_id
    );
    pci::enable_bus_master(dev.address);
    let transport = ModernTransport::probe(LOG_TAG, &dev, hhdm_offset)?;
    let accepted = transport.negotiate(F_MULTIPORT)?;
    if accepted & F_MULTIPORT == 0 {
        serial_println!("{} no MULTIPORT: a bare console, not driven here", LOG_TAG);
        transport.reset();
        return Err(KernelError::NotSupported);
    }
    let max_nr_ports = transport.read_device_config32(CONFIG_MAX_NR_PORTS);

    // From the first enabled queue on, a failure resets the device before
    // anything it was given is freed.
    let mut console = match set_up(transport, hhdm_offset, max_nr_ports) {
        Ok(console) => console,
        Err((transport, e)) => {
            transport.reset();
            return Err(e);
        }
    };
    if let Err(e) = console.start() {
        console.transport.reset();
        console.release_frames();
        return Err(e);
    }
    serial_println!(
        "{} DRIVER_OK: {} of the device's {} ports driven{}",
        LOG_TAG,
        console.ports.len(),
        max_nr_ports,
        if console.stranded.is_empty() {
            ""
        } else {
            " (one more port's receive queue enabled and held unused: no transmit queue for it)"
        }
    );
    let driven = console.ports.len();
    *CONSOLE.lock() = Some(console);
    PRESENT.store(true, Ordering::Release);
    pci::bind_driver(dev.address, "virtio-console");

    // The announcements arrive as the device gets to them: wait until every
    // port driven is announced and named, or briefly -- a port the device has
    // room for but no `virtserialport` behind is never announced.
    let deadline = crate::hrtimer::now_ns().saturating_add(SETTLE_NS);
    while driven > 0 && crate::hrtimer::now_ns() < deadline {
        poll();
        let settled = CONSOLE
            .lock()
            .as_ref()
            .is_some_and(|c| c.ports.iter().all(|p| p.added && p.name.is_some()));
        if settled {
            break;
        }
        core::hint::spin_loop();
    }
    for port in ports() {
        let name = port.name.as_deref().map_or_else(
            || alloc::string::String::from("(unnamed)"),
            |n| alloc::format!("{}", n.escape_ascii()),
        );
        serial_println!(
            "{} port {}: {}, host {}",
            LOG_TAG,
            port.id,
            name,
            if port.host_connected {
                "connected"
            } else {
                "not connected"
            }
        );
    }
    Ok(())
}

/// Queues and buffers for the control channel and for ports 1 up to
/// [`MAX_PORTS`] -- as many as the device has and the descriptor pool, shared
/// by every virtio device, can hold: a port it cannot is one fewer port, not a
/// failed device. On failure the transport comes back, for the caller to reset.
fn set_up(
    transport: ModernTransport,
    hhdm: u64,
    max_nr_ports: u32,
) -> Result<Console, (ModernTransport, KernelError)> {
    // Frames first: a frame that cannot be had then fails before the device
    // has been given any queue.
    let ctrl_rx_frame = match frame::alloc_frame_zeroed() {
        Ok(f) => f,
        Err(e) => return Err((transport, e)),
    };
    let ctrl_tx_frame = match frame::alloc_frame_zeroed() {
        Ok(f) => f,
        Err(e) => {
            release(&[ctrl_rx_frame]);
            return Err((transport, e));
        }
    };
    let control = transport.setup_queue(CTRL_RX_QUEUE, hhdm).and_then(|rx| {
        transport
            .setup_queue(CTRL_TX_QUEUE, hhdm)
            .map(|tx| (rx, tx))
    });
    let (mut ctrl_rxq, mut ctrl_txq) = match control {
        Ok(q) => q,
        Err(e) => {
            release(&[ctrl_rx_frame, ctrl_tx_frame]);
            return Err((transport, e));
        }
    };
    ctrl_rxq.set_no_interrupt();
    ctrl_txq.set_no_interrupt();

    let num_queues = transport.num_queues();
    let mut ports = Vec::new();
    let mut stranded = Vec::new();
    for id in 1..=max_nr_ports.saturating_sub(1).min(MAX_PORTS) {
        let Some((rx_index, tx_index)) = queue_indices(id) else {
            break;
        };
        if tx_index >= num_queues {
            break;
        }
        let Ok(rx_frame) = frame::alloc_frame_zeroed() else {
            break;
        };
        let Ok(tx_frame) = frame::alloc_frame_zeroed() else {
            release(&[rx_frame]);
            break;
        };
        let mut rxq = match transport.setup_queue(rx_index, hhdm) {
            Ok(q) => q,
            Err(e) => {
                serial_println!(
                    "{} port {}: no receive queue ({:?}); no more ports",
                    LOG_TAG,
                    id,
                    e
                );
                release(&[rx_frame, tx_frame]);
                break;
            }
        };
        let mut txq = match transport.setup_queue(tx_index, hhdm) {
            Ok(q) => q,
            Err(e) => {
                serial_println!(
                    "{} port {}: no transmit queue ({:?}); no more ports",
                    LOG_TAG,
                    id,
                    e
                );
                // Enabled already, so kept for the device's life. Its frames
                // were never given to the device.
                rxq.set_no_interrupt();
                stranded.push(rxq);
                release(&[rx_frame, tx_frame]);
                break;
            }
        };
        rxq.set_no_interrupt();
        txq.set_no_interrupt();
        ports.push(Port {
            id,
            rx_index,
            tx_index,
            rxq,
            txq,
            rx_frame,
            tx_frame,
            rx_posted: [None; RX_COUNT],
            received: VecDeque::new(),
            added: false,
            name: None,
            host_connected: false,
        });
    }
    Ok(Console {
        transport,
        hhdm,
        ctrl_rxq,
        ctrl_txq,
        ctrl_rx_frame,
        ctrl_tx_frame,
        ctrl_rx_posted: [None; CTRL_RX_COUNT],
        ports,
        stranded,
    })
}

/// Give back frames this driver allocated and no device holds.
fn release(frames: &[PhysFrame]) {
    for &frame in frames {
        // SAFETY: allocated here by `alloc_frame_zeroed`, never handed to the
        // device or to anyone else, and not used after this.
        // Discarded: a frame the allocator will not take back is lost either
        // way, and the failure being handled is the one to report.
        let _ = unsafe { frame::free_frame(frame) };
    }
}

impl Console {
    /// Hand the device its control buffers, say DRIVER_OK, hand each port its
    /// receive buffers, and say DEVICE_READY.
    fn start(&mut self) -> KernelResult<()> {
        for slot in 0..CTRL_RX_COUNT {
            self.post_control(slot)?;
        }
        self.transport.add_status(STATUS_DRIVER_OK);
        self.transport.notify_queue(CTRL_RX_QUEUE);
        for index in 0..self.ports.len() {
            self.refill(index);
        }
        self.send_control(0, EVENT_DEVICE_READY, 1)
    }

    /// After a reset, on a failed start: the frames, which no device now holds.
    fn release_frames(&self) {
        release(&[self.ctrl_rx_frame, self.ctrl_tx_frame]);
        for port in &self.ports {
            release(&[port.rx_frame, port.tx_frame]);
        }
    }
}

/// The device: the modern ID, else the transitional one.
fn find_device() -> KernelResult<PciDevice> {
    pci::find_device(VIRTIO_VENDOR, DEVICE_ID_MODERN)
        .or_else(|| pci::find_device(VIRTIO_VENDOR, DEVICE_ID_TRANSITIONAL))
        .ok_or(KernelError::NoSuchDevice)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Whether a virtio console was found and started.
#[must_use]
pub fn is_present() -> bool {
    PRESENT.load(Ordering::Acquire)
}

/// Act on everything the device has said: port announcements, names, the host
/// connecting and going, and bytes received.
pub fn poll() {
    if !is_present() {
        return;
    }
    if let Some(console) = CONSOLE.lock().as_mut() {
        console.poll();
    }
}

/// The ports the device has announced.
#[must_use]
pub fn ports() -> Vec<PortInfo> {
    poll();
    CONSOLE.lock().as_ref().map_or_else(Vec::new, |console| {
        console
            .ports
            .iter()
            .filter(|p| p.added)
            .map(|p| PortInfo {
                id: p.id,
                name: p.name.clone(),
                host_connected: p.host_connected,
                buffered: p.received.len(),
            })
            .collect()
    })
}

/// The number of the announced port named `name`.
#[must_use]
pub fn port_named(name: &[u8]) -> Option<u32> {
    ports()
        .into_iter()
        .find(|p| p.name.as_deref() == Some(name))
        .map(|p| p.id)
}

/// Take up to `buf.len()` received bytes from port `id`.
///
/// # Errors
///
/// `NoSuchDevice` for a port not announced; `WouldBlock` when nothing has
/// arrived and the host end is connected. With nothing buffered and the host
/// end gone, `Ok(0)`: the end of what it sent.
pub fn read(id: u32, buf: &mut [u8]) -> KernelResult<usize> {
    let mut guard = CONSOLE.lock();
    let console = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;
    console.poll();
    let index = console
        .ports
        .iter()
        .position(|p| p.id == id && p.added)
        .ok_or(KernelError::NoSuchDevice)?;
    let port = console
        .ports
        .get_mut(index)
        .ok_or(KernelError::NoSuchDevice)?;
    if port.received.is_empty() {
        return if port.host_connected {
            Err(KernelError::WouldBlock)
        } else {
            Ok(0)
        };
    }
    let n = buf.len().min(port.received.len());
    for (slot, byte) in buf.iter_mut().zip(port.received.drain(..n)) {
        *slot = byte;
    }
    // Room again, perhaps, for buffers held back at the limit.
    console.refill(index);
    Ok(n)
}

/// Send `data` on port `id`; the count sent, which is all of it.
///
/// # Errors
///
/// `NoSuchDevice` for a port not announced; `NotConnected` when its host end
/// is not connected; `TimedOut` when the device did not take a buffer.
pub fn write(id: u32, data: &[u8]) -> KernelResult<usize> {
    let mut guard = CONSOLE.lock();
    let console = guard.as_mut().ok_or(KernelError::NoSuchDevice)?;
    console.poll();
    let index = console
        .ports
        .iter()
        .position(|p| p.id == id)
        .ok_or(KernelError::NoSuchDevice)?;
    console.write(index, data)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// The port the boot test attaches for this self-test (`scripts/boot-test.sh`):
/// behind it, a QEMU UDP chardev that sends to its own receiving address, so
/// whatever the guest writes to the port comes back in on it.
///
/// A loopback, not a file the host feeds: QEMU on Windows takes no input file
/// for a file chardev, and one that reaches the end of its input closes the
/// port's host end.
pub const SELFTEST_PORT: &[u8] = b"org.slateos.selftest.0";

/// What the self-test sends round: bytes, not text -- a NUL, a high byte, a
/// newline. Under the 4 KiB QEMU's UDP chardev reads a datagram into.
const SELFTEST_BYTES: &[u8] = b"slateos virtio-console \x00\xff\x7f round trip\n";

/// The protocol's arithmetic and encoding, always; and, when the boot test has
/// attached [`SELFTEST_PORT`], bytes written to it coming back in on it whole.
///
/// # Errors
///
/// `InternalError` naming the first step that answered wrongly.
pub fn self_test() -> KernelResult<()> {
    let fail = |what: &str| -> KernelResult<()> {
        serial_println!("{}   FAIL: {}", LOG_TAG, what);
        Err(KernelError::InternalError)
    };

    // The pure parts.
    if queue_indices(0) != Some((0, 1))
        || queue_indices(1) != Some((4, 5))
        || queue_indices(2) != Some((6, 7))
        || queue_indices(u32::MAX).is_some()
    {
        return fail("port queue indices");
    }
    let encoded = encode_control(7, EVENT_PORT_NAME, 1);
    let mut named = encoded.to_vec();
    named.extend_from_slice(b"org.example.0\0");
    let parsed = parse_control(&named);
    let expected = Control {
        id: 7,
        event: EVENT_PORT_NAME,
        value: 1,
        payload: b"org.example.0\0".to_vec(),
    };
    if parsed.as_ref() != Some(&expected)
        || parse_control(encoded.get(..7).unwrap_or_default()).is_some()
        || port_name(&expected.payload).as_deref() != Some(&b"org.example.0"[..])
        || port_name(b"\0").is_some()
        || port_name(b"no-nul").as_deref() != Some(&b"no-nul"[..])
    {
        return fail("control messages and port names");
    }
    serial_println!(
        "{}   control messages, port names, queue indices: OK",
        LOG_TAG
    );

    // The device, when the boot test attached one.
    if !is_present() {
        serial_println!(
            "{}   no virtio console attached: the device half is skipped",
            LOG_TAG
        );
        return Ok(());
    }
    let Some(id) = port_named(SELFTEST_PORT) else {
        serial_println!(
            "{}   no port named {}: the device half is skipped",
            LOG_TAG,
            SELFTEST_PORT.escape_ascii()
        );
        return Ok(());
    };
    // Out through the transmit queue, round QEMU's loopback, and back in
    // through the receive queue.
    match write(id, SELFTEST_BYTES) {
        Ok(n) if n == SELFTEST_BYTES.len() => {}
        other => {
            serial_println!("{}   writing port {}: {:?}", LOG_TAG, id, other);
            return fail("the bytes were not sent");
        }
    }
    let mut got = Vec::new();
    let deadline = crate::hrtimer::now_ns().saturating_add(2_000_000_000);
    let mut buf = [0u8; 256];
    while got.len() < SELFTEST_BYTES.len() && crate::hrtimer::now_ns() < deadline {
        match read(id, &mut buf) {
            Ok(n) => got.extend_from_slice(buf.get(..n).unwrap_or_default()),
            Err(KernelError::WouldBlock) => core::hint::spin_loop(),
            Err(e) => {
                serial_println!("{}   reading port {}: {:?}", LOG_TAG, id, e);
                return fail("the self-test port could not be read");
            }
        }
    }
    if got != SELFTEST_BYTES {
        serial_println!("{}   came back \"{}\"", LOG_TAG, got.escape_ascii());
        return fail("the bytes did not come back whole");
    }
    serial_println!(
        "{}   port {} ({}): bytes out through the device and back in, whole: OK",
        LOG_TAG,
        id,
        SELFTEST_PORT.escape_ascii()
    );
    Ok(())
}
