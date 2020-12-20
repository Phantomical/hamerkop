use crate::detail::{atomic_load, atomic_store, io_uring_cq};
use crate::raw::{io_uring_cqe, io_uring_enter};
use crate::{CompletionFlags, EnterFlags, RawFd, RingBufMut};

use std::io::Error;
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::Ordering;

pub struct CompletionQueue<'ring> {
  cq: &'ring mut io_uring_cq,
  fd: RawFd,
}

impl<'ring> CompletionQueue<'ring> {
  pub(crate) fn new(fd: RawFd, cq: &'ring mut io_uring_cq) -> Self {
    Self { fd, cq }
  }

  /// Get a slice of all the available CQEs at the current time.
  ///
  /// Once the CQEs have been processed advance must be called to
  /// tell the kernel that it can use those completion slots.
  pub fn available(&mut self) -> RingBufMut<CompletionQueueEvent> {
    unsafe {
      let head = *self.cq.khead;
      let tail = atomic_load(self.cq.ktail, Ordering::Acquire);
      let mask = *self.cq.kring_mask;

      RingBufMut::new(
        NonNull::new_unchecked(self.cq.cqes as *mut _),
        (head & mask) as usize,
        (tail - head) as usize,
        mask as usize,
      )
    }
  }

  /// Advance the completion queue.
  ///
  /// This tells the kernel that it can put new events into the next
  /// count completion queue slots.
  pub fn advance(&mut self, count: usize) {
    debug_assert!(count <= u32::MAX as usize);

    unsafe {
      let head = *self.cq.khead;

      if cfg!(debug_assertions) {
        let tail = atomic_load(self.cq.ktail, Ordering::Acquire);
        assert!(head + count as u32 <= tail);
      }

      atomic_store(self.cq.khead, head + count as u32, Ordering::Release);
    }
  }

  /// Wait for at least 1 CQE to be ready within the queue.
  pub fn wait(&mut self) -> Result<(), Error> {
    self.wait_n(1)
  }

  /// Wait for at least `count` CQEs to be ready within the queue.
  ///
  /// This can be used to call io_uring_enter for the side-effects (call
  /// with count=0) but generally you should be using CompletionQueue::submit
  /// to accomplish that.
  pub fn wait_n(&mut self, count: usize) -> Result<(), Error> {
    let flags = EnterFlags::GETEVENTS;

    unsafe {
      sysresult!(io_uring_enter(
        self.fd as u32,
        0,
        count as u32,
        flags.bits(),
        null_mut()
      ))?
    };

    Ok(())
  }

  #[inline]
  pub fn has_next(&self) -> bool {
    unsafe { *self.cq.khead < atomic_load(self.cq.ktail, Ordering::Acquire) }
  }
}

unsafe impl Send for CompletionQueue<'_> {}
unsafe impl Sync for CompletionQueue<'_> {}

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct CompletionQueueEvent {
  cqe: io_uring_cqe,
}

impl CompletionQueueEvent {
  pub fn from_raw(cqe: io_uring_cqe) -> Self {
    Self { cqe }
  }

  pub fn into_raw(self) -> io_uring_cqe {
    self.cqe
  }

  pub fn raw(&self) -> &io_uring_cqe {
    &self.cqe
  }

  pub fn raw_mut(&mut self) -> &mut io_uring_cqe {
    &mut self.cqe
  }

  pub fn user_data(&self) -> u64 {
    self.cqe.user_data
  }

  pub fn buf_id(&self) -> Option<u16> {
    self.flags().buf_id()
  }

  pub fn result(&self) -> Result<usize, Error> {
    resultify!(self.cqe.res)
  }

  pub fn result_raw(&self) -> i32 {
    self.cqe.res
  }

  pub fn flags(&self) -> CompletionFlags {
    unsafe { CompletionFlags::from_bits_unchecked(self.cqe.flags) }
  }
}
