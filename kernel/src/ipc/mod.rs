//! Inter-Process Communication subsystem.
//!
//! ## IPC Mechanisms
//!
//! - **Channels** (primary): structured messages + capability transfer.
//!   Async (buffered) by default, sync (rendezvous) as option.
//! - **Pipes**: one-way byte streams with splice/vmsplice optimization.
//! - **Shared memory**: direct memory sharing with ring buffer support,
//!   futex signaling, and seqlocks.
//! - **Eventfd counters**: lightweight wake-up notifications.
//! - **Completion port**: unified wait on heterogeneous kernel objects
//!   (IOCP-style).
//! - **Service registry**: named service discovery + connection brokering.
//! - **Namespaces**: per-process filesystem path isolation and remapping.
//!
//! ## Performance Targets
//!
//! - Channel round-trip: < 2us (Fuchsia: 1-2us, L4: 0.5-1us)
//! - Eventfd wake: 0.5-1us
//! - Completion port ready events: sub-microsecond

pub mod alsa_pcm;
pub mod channel;
pub mod completion;
pub mod epoll;
pub mod eventfd;
pub mod futex;
pub mod inotify;
pub mod io_ring;
pub mod memfd;
pub mod namespace;
pub mod pipe;
pub mod semaphore;
pub mod service;
pub mod service_limits;
pub mod shm;
pub mod signalfd;
pub mod stats;
pub mod stream_socket;
pub mod timer;
pub mod timerfd;

// Pipe splice/tee/vmsplice: the syscall-level transfers (sys_splice,
// sys_tee, sys_vmsplice in syscall/linux.rs) are implemented against the
// pipe primitives in `pipe` (read/write/try_*/peek_at/wait_readable).
// They are currently *copy*-based; the remaining work is a true zero-copy
// buffer-move path (Linux's reference-counted `pipe_buffer` model) so
// splice transfers page ownership instead of copying — tracked in
// known-issues.md TD21 (pipe->pipe data-loss race + zero-copy fix).

use crate::cap::ResourceType;

/// Release all IPC handles in the given list.
///
/// Called during process reaping to clean up any handles the process
/// didn't explicitly close before dying.  Dispatches to the appropriate
/// IPC subsystem based on the resource type.
///
/// Handles that refer to shared resources (channels, pipes) will close
/// the dying process's end — the peer will observe a "closed" status
/// on subsequent operations.
pub fn cleanup_handles(handles: &[(ResourceType, u64)]) {
    for &(resource_type, handle_raw) in handles {
        match resource_type {
            ResourceType::Channel => {
                channel::close(channel::ChannelHandle::from_raw(handle_raw));
            }
            ResourceType::Pipe => {
                pipe::close(pipe::PipeHandle::from_raw(handle_raw));
            }
            ResourceType::SharedMemory => {
                shm::close(shm::ShmHandle::from_raw(handle_raw));
            }
            ResourceType::EventFd => {
                eventfd::close(eventfd::EventFdHandle::from_raw(handle_raw));
            }
            ResourceType::CompletionPort => {
                completion::close(completion::CpHandle::from_raw(handle_raw));
            }
            ResourceType::Timer => {
                timer::cancel(handle_raw);
            }
            ResourceType::StreamSocket => {
                stream_socket::close(stream_socket::StreamSocketHandle::from_raw(handle_raw));
            }
            ResourceType::MemFd => {
                memfd::close(memfd::MemFdHandle::from_raw(handle_raw));
            }
            ResourceType::Epoll => {
                epoll::close(epoll::EpollHandle::from_raw(handle_raw));
            }
            ResourceType::SignalFd => {
                signalfd::close(signalfd::SignalFdHandle::from_raw(handle_raw));
            }
            ResourceType::Timerfd => {
                timerfd::close(timerfd::TimerFdHandle::from_raw(handle_raw));
            }
            ResourceType::Inotify => {
                inotify::close(inotify::InotifyHandle::from_raw(handle_raw));
            }
            ResourceType::AlsaPcm => {
                alsa_pcm::close(alsa_pcm::AlsaPcmHandle::from_raw(handle_raw));
            }
            ResourceType::Drm => {
                crate::drm::card_fd::close(
                    crate::drm::card_fd::DrmCardHandle::from_raw(handle_raw),
                );
            }
            ResourceType::NetSocket => {
                // Drop this process's reference to the AF_INET socket's daemon
                // connection.  The final owner's close runs `NetstackConn`
                // teardown (a blocking daemon round-trip); safe in the
                // exit-cleanup context (kernel thread, no locks held here).
                crate::net::socket::close(
                    crate::net::socket::SocketHandle::from_raw(handle_raw),
                );
            }
            ResourceType::File => {
                // Open file handles are refcounted in the open-file table;
                // closing here drops this process's single reference.  A
                // forked child holds its own reference (fork dup_shares the
                // id), so the underlying description survives until the last
                // owner closes.  `SYS_FS_CLOSE` deregisters the handle, so a
                // handle reaching this path was still open at exit.
                let _ = crate::fs::handle::close(handle_raw);
            }
            // No cleanup needed for these types — they're either
            // permission tokens (PortIo, DeviceIrq, IoScheduler, NetRaw) or
            // managed by other subsystems (Socket, Service, Namespace).
            // NetRaw's exclusive NIC claim is released lazily by
            // `net::raw::is_claimed` when it observes the owner has died.
            ResourceType::Process
            | ResourceType::Thread
            | ResourceType::PortIo
            | ResourceType::DeviceIrq
            | ResourceType::Socket
            | ResourceType::IoScheduler
            | ResourceType::Service
            | ResourceType::NetRaw
            | ResourceType::Namespace => {}
        }
    }
}
