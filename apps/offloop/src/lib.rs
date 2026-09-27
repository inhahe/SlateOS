//! Work done off an application's event-loop thread, and handed back to it.
//!
//! # Why
//!
//! An application's loop thread is the one that draws its frames. Work that
//! takes longer than a frame -- decoding a photograph, reading a large file --
//! done on that thread freezes the window until it finishes: the picture a
//! user clicked on arrives, but nothing else does meanwhile, not even the
//! click on the next one. `known-issues.md` ->
//! `TD-C-DECODING-A-PHOTOGRAPH-BLOCKS-THE-FRAME-THAT-ASKED-FOR-IT`.
//!
//! The work belongs on a thread of its own. What was missing was a way for
//! that thread to say it had finished: the loop is parked waiting for the
//! display, and a result left where the drawing looks for it would sit unseen
//! until the user next moved the mouse. `oswindow::app::App::attach_waker`
//! gives an application that asks a [`Waker`] that wakes its loop, and
//! `App::on_wake` runs on the loop's thread when it does
//! (`requests/f-ce-a-finished-decode-can-now-wake-the-window-that-asked-for-it.md`).
//! This crate is the other half: the thread, the queue in front of it, and
//! the hand-back.
//!
//! # Newest wins
//!
//! [`Latest`] is for work whose earlier requests are worth nothing once a
//! newer one is made -- the picture a viewer shows, when the user has already
//! paged past it. So:
//!
//! - a request supersedes every request not yet started, which is dropped
//!   unrun: holding an arrow key down across a directory of photographs
//!   decodes the one the key stops on, not every one it passed;
//! - a result is handed back only for the newest request. One for a request
//!   made before it -- the decode that was already running when the user moved
//!   on -- is dropped, so a slow picture can never land on top of the one the
//!   user asked for after it.
//!
//! The work runs one request at a time, in the order the surviving requests
//! were made, on one thread that lives as long as the [`Latest`].
//!
//! # Every one wanted
//!
//! [`Queue`] is for work where each request's result is worth having, and the
//! caller knows a whole set at once -- the thumbnails of every card a grid
//! shows. Its request is that set, in the order to make them: a new set
//! replaces whatever of the last is not yet started (cards scrolled away are
//! not wanted any more), and each result is handed back as soon as it is
//! made, waking the loop, so a grid fills card by card rather than all at
//! the end.
//!
//! # A panic
//!
//! Userspace builds with `panic = "abort"` (the workspace's profiles), so a
//! panic in the work ends the program, exactly as a panic on the loop thread
//! would. Nothing here tries to catch one: under `abort` there is nothing to
//! catch, and a result type carrying "the work panicked" would be a branch no
//! build could take.

use std::collections::VecDeque;
use std::io;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::task::Waker;
use std::thread;
use std::time::{Duration, Instant};

/// Which request a result answers: requests are numbered from 1 as they are
/// made.
pub type Ticket = u64;

/// A request, as it waits for the worker.
struct Job<J> {
    ticket: Ticket,
    job: J,
}

/// A result, as it waits for the loop.
struct Done<R> {
    ticket: Ticket,
    result: R,
}

/// Work run on a thread of its own, newest request first and only; see the
/// module docs.
///
/// Dropping it ends the worker once the request it is running (if any) is
/// done. The drop does not wait for that: it happens on the loop's thread,
/// usually as the window closes, and waiting there for a decode nobody will
/// see is exactly the stall this crate exists to remove.
pub struct Latest<J, R> {
    jobs: Sender<Job<J>>,
    results: Receiver<Done<R>>,
    /// The newest request's ticket; 0 before the first.
    asked: Ticket,
    /// The ticket of the newest result handed back; 0 before the first.
    answered: Ticket,
}

impl<J: Send + 'static, R: Send + 'static> Latest<J, R> {
    /// Start the worker: a thread called `name`, running `work` on each
    /// request it takes and waking the loop through `waker` when a result is
    /// ready.
    ///
    /// # Errors
    ///
    /// When the thread cannot be started. The caller still has its work to
    /// do; doing it on its own thread, as before, is the answer that keeps the
    /// program working.
    pub fn start<F>(name: &str, waker: Waker, mut work: F) -> io::Result<Self>
    where
        F: FnMut(J) -> R + Send + 'static,
    {
        let (jobs, requests) = mpsc::channel::<Job<J>>();
        let (answers, results) = mpsc::channel::<Done<R>>();
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                // Ends when the `Latest` is dropped: its sender goes, and
                // `recv` says so once the queue is empty.
                while let Ok(mut next) = requests.recv() {
                    // Only the newest request waiting is worth running.
                    while let Ok(newer) = requests.try_recv() {
                        next = newer;
                    }
                    let result = work(next.job);
                    let done = Done {
                        ticket: next.ticket,
                        result,
                    };
                    if answers.send(done).is_err() {
                        // The `Latest` is gone; nobody is left to answer.
                        break;
                    }
                    waker.wake_by_ref();
                }
            })?;
        Ok(Self {
            jobs,
            results,
            asked: 0,
            answered: 0,
        })
    }

    /// Ask for `job` to be run, in place of every request not yet started.
    /// Its ticket is the one [`Latest::take`] will answer.
    ///
    /// # Errors
    ///
    /// Gives `job` back when the worker is gone -- which, with the thread
    /// only ending when this is dropped, means it panicked in a build that
    /// unwinds (a test). The caller can still do the work itself.
    pub fn ask(&mut self, job: J) -> Result<Ticket, J> {
        let ticket = self.asked.saturating_add(1);
        match self.jobs.send(Job { ticket, job }) {
            Ok(()) => {
                self.asked = ticket;
                Ok(ticket)
            }
            Err(mpsc::SendError(Job { job, .. })) => Err(job),
        }
    }

    /// Whether the newest request has not been answered yet.
    #[must_use]
    pub fn busy(&self) -> bool {
        self.answered < self.asked
    }

    /// The newest request's result, if it has arrived and not been taken.
    ///
    /// Results for older requests that have arrived are dropped here: their
    /// requests were superseded, and handing one back would put a picture
    /// the user has moved past on top of the one they moved to.
    pub fn take(&mut self) -> Option<R> {
        let mut newest = None;
        while let Ok(done) = self.results.try_recv() {
            if done.ticket == self.asked {
                self.answered = done.ticket;
                newest = Some(done.result);
            }
        }
        newest
    }

    /// Wait up to `timeout` for the newest request's result.
    ///
    /// Not for the loop's thread, whose waiting is the display's to do: this
    /// is for a caller with nothing to draw -- a test, or a tool that runs the
    /// same work to completion.
    pub fn wait(&mut self, timeout: Duration) -> Option<R> {
        let deadline = Instant::now().checked_add(timeout)?;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.results.recv_timeout(left) {
                Ok(done) if done.ticket == self.asked => {
                    self.answered = done.ticket;
                    return Some(done.result);
                }
                // A superseded request's; keep waiting for the newest.
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
            }
        }
    }
}

/// Work run on a thread of its own, a set of requests at a time, every result
/// handed back as it is made; see the module docs.
///
/// Dropping it ends the worker once the request it is running (if any) is
/// done, without waiting for it -- as for [`Latest`].
pub struct Queue<J, R> {
    sets: Sender<Vec<J>>,
    results: Receiver<R>,
}

impl<J: Send + 'static, R: Send + 'static> Queue<J, R> {
    /// Start the worker: a thread called `name`, running `work` on each
    /// request in turn and waking the loop through `waker` after each result.
    ///
    /// # Errors
    ///
    /// When the thread cannot be started; the caller can still do the work
    /// itself.
    pub fn start<F>(name: &str, waker: Waker, mut work: F) -> io::Result<Self>
    where
        F: FnMut(J) -> R + Send + 'static,
    {
        let (sets, requests) = mpsc::channel::<Vec<J>>();
        let (answers, results) = mpsc::channel::<R>();
        thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let mut waiting = VecDeque::new();
                loop {
                    // With nothing to do, wait for a set; the `Queue` going
                    // is the end.
                    if waiting.is_empty() {
                        match requests.recv() {
                            Ok(set) => waiting = VecDeque::from(set),
                            Err(_) => break,
                        }
                    }
                    // A newer set replaces what is left of this one.
                    while let Ok(set) = requests.try_recv() {
                        waiting = VecDeque::from(set);
                    }
                    let Some(job) = waiting.pop_front() else {
                        continue;
                    };
                    if answers.send(work(job)).is_err() {
                        // The `Queue` is gone; nobody is left to answer.
                        break;
                    }
                    waker.wake_by_ref();
                }
            })?;
        Ok(Self { sets, results })
    }

    /// Ask for `jobs`, in this order, in place of every request not yet
    /// started. An empty set cancels what is waiting.
    ///
    /// # Errors
    ///
    /// Gives the set back when the worker is gone (it can only have panicked,
    /// in a build that unwinds); the caller can still do the work itself.
    pub fn replace(&mut self, jobs: Vec<J>) -> Result<(), Vec<J>> {
        self.sets.send(jobs).map_err(|mpsc::SendError(jobs)| jobs)
    }

    /// Every result made since the last call, in the order they were made.
    pub fn take(&mut self) -> Vec<R> {
        self.results.try_iter().collect()
    }

    /// Wait up to `timeout` for the next result.
    ///
    /// Not for the loop's thread; for a caller with nothing to draw -- a test,
    /// or a tool that runs the same work to completion.
    pub fn wait(&mut self, timeout: Duration) -> Option<R> {
        self.results.recv_timeout(timeout).ok()
    }
}

/// A waker that says each wake on a channel, and the channel's receiving end.
///
/// For a caller with no event loop to wake -- a test above all, which can
/// wait on the receiver for the work to finish instead of sleeping and
/// hoping.
#[must_use]
pub fn channel_waker() -> (Waker, Receiver<()>) {
    /// Sends one `()` per wake.
    struct Said(std::sync::Mutex<Sender<()>>);

    impl std::task::Wake for Said {
        fn wake(self: std::sync::Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &std::sync::Arc<Self>) {
            // A poisoned lock still holds a working sender; and a receiver
            // that has gone means nobody is listening, which is no error of
            // the waker's.
            let said = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = said.send(());
        }
    }

    let (said, heard) = mpsc::channel();
    let waker = Waker::from(std::sync::Arc::new(Said(std::sync::Mutex::new(said))));
    (waker, heard)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::Wake;

    /// A waker that counts its wakes and says each on a channel.
    struct Counted {
        wakes: AtomicUsize,
        said: Mutex<Sender<()>>,
    }

    impl Wake for Counted {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.wakes.fetch_add(1, Ordering::SeqCst);
            // The test may have stopped listening; the count still stands.
            let _ = self.said.lock().unwrap().send(());
        }
    }

    fn counted() -> (Arc<Counted>, Waker, Receiver<()>) {
        let (said, heard) = mpsc::channel();
        let counter = Arc::new(Counted {
            wakes: AtomicUsize::new(0),
            said: Mutex::new(said),
        });
        (
            Arc::clone(&counter),
            Waker::from(Arc::clone(&counter)),
            heard,
        )
    }

    const LONG: Duration = Duration::from_secs(10);

    /// **The work runs off the caller's thread, and wakes the loop when it is
    /// done.**
    #[test]
    fn a_result_is_handed_back_and_the_loop_woken() {
        let (counter, waker, heard) = counted();
        let caller = thread::current().id();
        let mut latest = Latest::start("test", waker, move |n: u32| {
            (n * 2, thread::current().id() != caller)
        })
        .unwrap();
        assert!(!latest.busy());
        assert_eq!(latest.ask(21), Ok(1));
        assert!(latest.busy());
        heard.recv_timeout(LONG).expect("the loop was never woken");
        assert_eq!(latest.take(), Some((42, true)), "run on another thread");
        assert!(!latest.busy());
        assert_eq!(latest.take(), None, "handed back twice");
        assert_eq!(counter.wakes.load(Ordering::SeqCst), 1);
    }

    /// **Requests made while the worker is busy are not all run: the newest
    /// is.** The worker is held on its first request until three more are
    /// queued; of those it runs only the last.
    #[test]
    fn requests_waiting_are_superseded_by_the_newest() {
        let (_counter, waker, _heard) = counted();
        let (release, held) = mpsc::channel::<()>();
        let held = Mutex::new(held);
        let (started, taken) = mpsc::channel::<()>();
        let started = Mutex::new(started);
        let ran = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&ran);
        let mut latest = Latest::start("test", waker, move |n: u32| {
            if n == 1 {
                started.lock().unwrap().send(()).unwrap();
                held.lock().unwrap().recv_timeout(LONG).unwrap();
            }
            log.lock().unwrap().push(n);
            n
        })
        .unwrap();
        latest.ask(1).unwrap();
        // The worker has the first before the rest arrive.
        taken.recv_timeout(LONG).unwrap();
        for n in 2..=4 {
            latest.ask(n).unwrap();
        }
        release.send(()).unwrap();
        assert_eq!(latest.wait(LONG), Some(4));
        assert_eq!(*ran.lock().unwrap(), [1, 4], "2 and 3 were run for nothing");
    }

    /// **A result for a superseded request is never handed back**, even when
    /// it is the only one there: the picture the user moved past must not
    /// land on top of the one they moved to.
    #[test]
    fn a_superseded_result_is_dropped() {
        let (_counter, waker, heard) = counted();
        let (release, held) = mpsc::channel::<()>();
        let held = Mutex::new(held);
        let mut latest = Latest::start("test", waker, move |n: u32| {
            if n == 2 {
                held.lock().unwrap().recv_timeout(LONG).unwrap();
            }
            n
        })
        .unwrap();
        latest.ask(1).unwrap();
        heard.recv_timeout(LONG).unwrap();
        // 1's result is waiting; 2 makes it stale, and 2 is held.
        latest.ask(2).unwrap();
        assert_eq!(latest.take(), None, "1's result was handed back");
        assert!(latest.busy());
        release.send(()).unwrap();
        assert_eq!(latest.wait(LONG), Some(2));
        assert_eq!(latest.take(), None, "2's result was handed back twice");
    }

    /// `wait` gives up at its deadline, and says nothing for an answered
    /// request.
    #[test]
    fn wait_gives_up_at_its_deadline() {
        let (_counter, waker, _heard) = counted();
        let (release, held) = mpsc::channel::<()>();
        let held = Mutex::new(held);
        let mut latest = Latest::start("test", waker, move |n: u32| {
            held.lock().unwrap().recv_timeout(LONG).unwrap();
            n
        })
        .unwrap();
        latest.ask(7).unwrap();
        let started = Instant::now();
        assert_eq!(latest.wait(Duration::from_millis(100)), None);
        assert!(started.elapsed() >= Duration::from_millis(100));
        release.send(()).unwrap();
        assert_eq!(latest.wait(LONG), Some(7));
        assert_eq!(latest.wait(Duration::from_millis(10)), None);
    }

    /// **A queue runs every request in a set, in order, and hands each result
    /// back as it is made**, waking the loop each time.
    #[test]
    fn a_queue_hands_back_every_result_as_it_is_made() {
        let (counter, waker, heard) = counted();
        let mut queue = Queue::start("test", waker, |n: u32| n * 10).unwrap();
        queue.replace(vec![1, 2, 3]).unwrap();
        for _ in 0..3 {
            heard.recv_timeout(LONG).expect("the loop was never woken");
        }
        // Everything made since the last `take`, in one.
        assert_eq!(queue.take(), [10, 20, 30]);
        assert_eq!(counter.wakes.load(Ordering::SeqCst), 3, "one wake a result");
        assert!(queue.take().is_empty());
    }

    /// **A new set replaces what is left of the last** -- the cards scrolled
    /// away are not made -- though the one already running finishes.
    #[test]
    fn a_new_set_replaces_what_is_left_of_the_last() {
        let (_counter, waker, _heard) = counted();
        let (release, held) = mpsc::channel::<()>();
        let held = Mutex::new(held);
        let (started, taken) = mpsc::channel::<()>();
        let started = Mutex::new(started);
        let mut queue = Queue::start("test", waker, move |n: u32| {
            if n == 1 {
                started.lock().unwrap().send(()).unwrap();
                held.lock().unwrap().recv_timeout(LONG).unwrap();
            }
            n
        })
        .unwrap();
        queue.replace(vec![1, 2, 3]).unwrap();
        taken.recv_timeout(LONG).unwrap();
        queue.replace(vec![7, 8]).unwrap();
        release.send(()).unwrap();
        let got: Vec<u32> = (0..3).filter_map(|_| queue.wait(LONG)).collect();
        assert_eq!(got, [1, 7, 8], "2 and 3 were made after they were replaced");
        assert_eq!(queue.wait(Duration::from_millis(100)), None);
    }

    /// An empty set cancels what is waiting.
    #[test]
    fn an_empty_set_cancels_what_is_waiting() {
        let (_counter, waker, _heard) = counted();
        let (release, held) = mpsc::channel::<()>();
        let held = Mutex::new(held);
        let (started, taken) = mpsc::channel::<()>();
        let started = Mutex::new(started);
        let mut queue = Queue::start("test", waker, move |n: u32| {
            if n == 1 {
                started.lock().unwrap().send(()).unwrap();
                held.lock().unwrap().recv_timeout(LONG).unwrap();
            }
            n
        })
        .unwrap();
        queue.replace(vec![1, 2, 3]).unwrap();
        taken.recv_timeout(LONG).unwrap();
        queue.replace(Vec::new()).unwrap();
        release.send(()).unwrap();
        assert_eq!(queue.wait(LONG), Some(1));
        assert_eq!(
            queue.wait(Duration::from_millis(200)),
            None,
            "2 and 3 were made"
        );
    }

    /// `channel_waker` says every wake, and survives nobody listening.
    #[test]
    fn a_channel_waker_says_each_wake() {
        let (waker, heard) = channel_waker();
        waker.wake_by_ref();
        // `wake` by value as well as by reference: both must say it.
        let owned = waker.clone();
        owned.wake();
        assert_eq!(heard.try_iter().count(), 2);
        drop(heard);
        waker.wake_by_ref();
    }

    /// Dropping it ends the worker, and does not wait for it.
    #[test]
    fn dropping_it_ends_the_worker_without_waiting() {
        let (_counter, waker, _heard) = counted();
        let (release, held) = mpsc::channel::<()>();
        let held = Mutex::new(held);
        let (gone, ended) = mpsc::channel::<()>();
        struct Tell(Sender<()>);
        impl Drop for Tell {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let tell = Tell(gone);
        let mut latest = Latest::start("test", waker, move |n: u32| {
            let _keep = &tell;
            held.lock().unwrap().recv_timeout(LONG).unwrap();
            n
        })
        .unwrap();
        latest.ask(1).unwrap();
        let started = Instant::now();
        drop(latest);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the drop waited"
        );
        release.send(()).unwrap();
        ended
            .recv_timeout(LONG)
            .expect("the worker outlived its Latest");
    }
}
