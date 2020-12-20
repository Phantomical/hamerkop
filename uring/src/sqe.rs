use crate::detail::{atomic_load, atomic_store, io_uring_sq};
use crate::raw::*;
use crate::sqes::Prepare;
use crate::{EnterFlags, RawFd, RingBufMut, SetupFlags, SubmissionFlags};

use std::ptr::null_mut;
use std::sync::atomic::Ordering;
use std::{io::Error, ptr::NonNull};

pub struct SubmissionQueue<'ring> {
  sq: &'ring mut io_uring_sq,
  fd: RawFd,
  flags: SetupFlags,
}

impl<'ring> SubmissionQueue<'ring> {
  pub(crate) fn new(fd: RawFd, flags: SetupFlags, sq: &'ring mut io_uring_sq) -> Self {
    Self { fd, flags, sq }
  }

  unsafe fn flush(&mut self) -> u32 {
    let mask = *self.sq.kring_mask;
    let to_submit = self.sq.sqe_tail - self.sq.sqe_head;

    if to_submit == 0 {
      let ktail = *self.sq.ktail;
      return ktail - *self.sq.khead;
    }

    // Fill in sqes that we have queued up, adding them to the
    // kernel ring.
    let mut ktail = *self.sq.ktail;
    for _ in 0..to_submit {
      *self.sq.array.add((ktail & mask) as usize) = self.sq.sqe_head & mask;
      self.sq.sqe_head += 1;
      ktail += 1;
    }

    // Ensure that the kernel sees the SQE updates before it sees the
    // tail update.
    atomic_store(self.sq.ktail, ktail, Ordering::Release);

    ktail - *self.sq.khead
  }

  unsafe fn enter_flags(&mut self, submitted: u32) -> Option<EnterFlags> {
    if !self.flags.contains(SetupFlags::SQPOLL) {
      return match submitted > 0 {
        true => Some(EnterFlags::empty()),
        false => None,
      };
    }

    let kflags = atomic_load(self.sq.kflags, Ordering::Acquire);
    match kflags & IORING_SQ_NEED_WAKEUP {
      0 => None,
      _ => Some(EnterFlags::SQ_WAKEUP),
    }
  }

  /// Get the next available SQE within the queue.
  ///
  /// This does not submit anything, it only returns a reference to
  /// the next SQE. To submit SQEs use `submit`.
  pub fn next_sqe(&mut self) -> Option<&mut SubmissionQueueEvent> {
    unsafe {
      let next = self.sq.sqe_tail + 1;
      let head = atomic_load(self.sq.khead, Ordering::Acquire);

      if next - head <= *self.sq.kring_entries {
        let mask = *self.sq.kring_mask;
        let sqe = self.sq.sqes.add((self.sq.sqe_tail & mask) as usize);
        let sqe = SubmissionQueueEvent::new(&mut *sqe);
        self.sq.sqe_tail = next;

        Some(sqe)
      } else {
        None
      }
    }
  }

  /// Get the number of available SQEs within the queue.
  pub fn free_sqes(&self) -> usize {
    unsafe {
      let head = atomic_load(self.sq.khead, Ordering::Acquire);
      let tail = *self.sq.ktail;

      (head - tail) as usize
    }
  }

  /// Get mutable access to all free sqes currently availble within
  /// the queue.
  pub fn sqes(&mut self) -> RingBufMut<SubmissionQueueEvent> {
    unsafe {
      let head = atomic_load(self.sq.khead, Ordering::Acquire);

      let next = self.sq.sqe_tail;
      let mask = *self.sq.kring_mask;
      let available = next - head;

      RingBufMut::new(
        NonNull::new_unchecked(self.sq.sqes as *mut _),
        next as usize,
        available as usize,
        mask as usize,
      )
    }
  }

  /// Mark the next `count` SQEs as ready-to-submit.
  pub fn advance(&mut self, count: u32) {
    self.sq.sqe_tail += count;
  }

  /// Submit all events in the queue. Returns the number of submitted
  /// events.
  ///
  /// If this function encounters any IO errors an
  /// [`io::Error`](std::io::Error) result variant is returned.
  ///
  /// # Safety
  /// Undefined behaviour occurs if any memory referenced within any
  /// of the submitted SQEs is freed before either the `IoUring`
  /// instanced is dropped or the corresponding completion event
  /// is received in the completion queue.
  ///
  /// It is also UB to read and/or write to any such memory as the
  /// kernel will be concurrently reading/writing from/to the provided
  /// memory and data races will occur.
  pub unsafe fn submit(&mut self) -> Result<usize, Error> {
    let submitted = self.flush();

    if let Some(flags) = self.enter_flags(submitted) {
      sysresult!(io_uring_enter(
        self.fd as u32,
        submitted as u32,
        0,
        flags.bits(),
        null_mut()
      ))?;
    }

    Ok(submitted as usize)
  }

  /// Submit all events in the queue and wait for the specified number
  /// of completion events.
  ///
  /// # Safety
  /// Undefined behaviour occurs if any of the pointers contained within
  /// any of the submitted SQEs have their lifetimes end before the
  /// completion event is recieved. See the safety sections on each of
  /// the `prep_.*` methods for more details.
  pub unsafe fn submit_and_wait(&mut self, wait_for: u32) -> Result<usize, Error> {
    let submitted = self.flush();

    let (mut flags, mut should_enter) = match wait_for {
      0 => (EnterFlags::empty(), false),
      _ => (EnterFlags::GETEVENTS, true),
    };

    if let Some(newflags) = self.enter_flags(submitted) {
      flags |= newflags;
      should_enter = true;
    }

    if should_enter {
      sysresult!(io_uring_enter(
        self.fd as u32,
        submitted as u32,
        wait_for,
        flags.bits(),
        null_mut()
      ))?;
    }

    Ok(submitted as usize)
  }
}

unsafe impl Send for SubmissionQueue<'_> {}
unsafe impl Sync for SubmissionQueue<'_> {}

#[repr(transparent)]
pub struct SubmissionQueueEvent {
  sqe: io_uring_sqe,
}

impl SubmissionQueueEvent {
  pub(crate) fn new<'a>(sqe: &'a mut io_uring_sqe) -> &'a mut Self {
    unsafe { &mut *(sqe as *mut io_uring_sqe as *mut Self) }
  }

  #[inline]
  pub fn user_data(&self) -> u64 {
    self.sqe.user_data
  }

  #[inline]
  pub fn set_user_data(&mut self, user_data: u64) {
    self.sqe.user_data = user_data;
  }

  #[inline]
  pub fn flags(&self) -> SubmissionFlags {
    unsafe { SubmissionFlags::from_bits_unchecked(self.sqe.flags) }
  }

  #[inline]
  pub fn set_flags(&mut self, flags: SubmissionFlags) {
    self.sqe.flags = flags.bits();
  }

  #[inline]
  pub fn add_flags(&mut self, flags: SubmissionFlags) {
    self.sqe.flags |= flags.bits();
  }

  #[inline]
  pub fn buf_group(&mut self) -> u16 {
    unsafe { self.sqe.extra.buf_info.buf_group }
  }

  #[inline]
  pub fn set_buf_group(&mut self, buf_group: u16) {
    self.sqe.extra.buf_info.buf_group = buf_group;
  }

  #[inline]
  pub fn clear(&mut self) {
    self.sqe = unsafe { std::mem::zeroed() };
  }

  #[inline]
  pub fn into_raw(self) -> io_uring_sqe {
    self.sqe
  }

  #[inline]
  pub fn from_raw(sqe: io_uring_sqe) -> Self {
    Self { sqe }
  }

  #[inline]
  pub fn raw(&self) -> &io_uring_sqe {
    &self.sqe
  }

  #[inline]
  pub fn raw_mut(&mut self) -> &mut io_uring_sqe {
    &mut self.sqe
  }

  #[inline]
  pub fn prepare<S: Prepare>(&mut self, sqe: S) {
    sqe.prep_sqe(self);
  }
}

/// Computes the smallest value v > 1 such that `(a + v) & mask == b & mask`.
fn wrapping_dist(a: u32, b: u32, mask: u32) -> u32 {
  let size = mask + 1;

  let a = a & mask;
  let b = b & mask;

  if a == b {
    return size;
  }

  (a - b) & mask
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn basic_wrapping_dist_tests() {
    assert_eq!(wrapping_dist(0, 0, 31), 32);
    assert_eq!(wrapping_dist(6, 5, 31), 1);
    assert_eq!(wrapping_dist(63, 31, 31), 32);
  }
}

