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

  /// Get the number of available SQEs within the queue.
  pub fn free_sqes(&self) -> usize {
    unsafe {
      let head = atomic_load(self.sq.khead, Ordering::Acquire);
      let tail = *self.sq.ktail;
      let mask = *self.sq.kring_mask;

      wrapping_dist(head, tail, mask) as usize
    }
  }

  pub fn queue_size(&self) -> usize {
    unsafe { *self.sq.kring_entries as usize }
  }

  /// Get mutable access to all free sqes currently availble within
  /// the queue.
  pub fn sqes(&mut self) -> RingBufMut<SubmissionQueueEvent> {
    unsafe {
      let head = atomic_load(self.sq.khead, Ordering::Acquire);
      let tail = *self.sq.ktail;
      let mask = *self.sq.kring_mask;
      let available = wrapping_dist(head, tail, mask);

      RingBufMut::new(
        NonNull::new_unchecked(self.sq.sqes as *mut _),
        tail as usize,
        available as usize,
        mask as usize,
      )
    }
  }

  /// Mark the next `count` SQEs as ready-to-submit.
  pub fn advance(&mut self, count: u32) {
    unsafe {
      let tail = *self.sq.ktail;
      atomic_store(self.sq.ktail, tail + count, Ordering::Release);
    }
  }

  /// Submit all events in the queue. Returns the number of submitted
  /// events.
  ///
  /// If this function encounters any IO errors an
  /// [`io::Error`](std::io::Error) result variant is returned.
  ///
  /// # Safety
  /// Undefined behaviour occurs if any of the following occur:
  /// - Any memory referenced by any of the submitted SQEs is read from, written
  ///   to, or freed before the corresponding CQE is received or the IoUring
  ///   instance is dropped (either is fine).
  pub unsafe fn submit(&mut self) -> Result<usize, Error> {
    self.submit_and_wait(0)
  }

  /// Submit all events in the queue and wait for the specified number
  /// of completion events.
  ///
  /// # Safety
  /// Undefined behaviour occurs if any of the following occur:
  /// - Any memory referenced by any of the submitted SQEs is read from, written
  ///   to, or freed before the corresponding CQE is received or the IoUring
  ///   instance is dropped (either is fine).
  pub unsafe fn submit_and_wait(&mut self, wait_for: u32) -> Result<usize, Error> {
    let tail = *self.sq.ktail;
    let submitted = tail - self.sq.sqe_tail;

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

    self.sq.sqe_tail = tail;
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
  #[allow(dead_code)]
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

  (a.wrapping_sub(b)) & mask
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn basic_wrapping_dist_tests() {
    assert_eq!(wrapping_dist(0, 0, 31), 32);
    assert_eq!(wrapping_dist(6, 5, 31), 1);
    assert_eq!(wrapping_dist(63, 31, 31), 32);
    assert_eq!(wrapping_dist(4, 6, 7), 6);
  }
}
