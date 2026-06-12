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

// TODO: Splice/vmsplice for pipes.

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
            // permission tokens (PortIo, DeviceIrq, IoScheduler) or
            // managed by other subsystems (Socket, Service, Namespace).
            ResourceType::Process
            | ResourceType::Thread
            | ResourceType::PortIo
            | ResourceType::DeviceIrq
            | ResourceType::Socket
            | ResourceType::IoScheduler
            | ResourceType::Service
            | ResourceType::Namespace => {}
        }
    }
}
