//! Rust bindings for `io_uring`.

// Address this once docs are more fleshed out.
#![allow(clippy::missing_safety_doc, clippy::should_implement_trait)]

macro_rules! resultify {
  ($e:expr) => {{
    let res = $e;
    match res < 0 {
      true => Err(std::io::Error::from_raw_os_error(-res)),
      false => Ok(res as _),
    }
  }};
}

macro_rules! sysresult {
  ($ret:expr) => {{
    match $ret {
      ret if ret < 0 => Err(std::io::Error::last_os_error()),
      ret => Ok(ret),
    }
  }};
}

mod cqe;
mod detail;
mod flags;
mod registrar;
mod ringbuf;
mod sqe;
mod sqe_types;

pub mod raw;

pub mod sqes {
  pub use crate::sqe_types::*;
}

pub use crate::cqe::{CompletionQueue, CompletionQueueEvent};
pub use crate::flags::*;
pub use crate::registrar::{Probe, ProbeOp, Registrar};
pub use crate::ringbuf::{RingBuf, RingBufMut};
pub use crate::sqe::{SubmissionQueue, SubmissionQueueEvent};

use crate::detail::{io_uring_cq, io_uring_sq};

use std::io::Error;

use std::os::raw::c_int as RawFd;
// use std::os::unix::io::RawFd;

#[derive(Copy, Clone, Debug, Default)]
#[non_exhaustive]
pub struct UringParams {
  pub flags: SetupFlags,
  pub cq_entries: u32,
  pub sq_thread_cpu: u32,
  pub sq_thread_idle: u32,
}

pub struct IoUring {
  cq: io_uring_cq,
  sq: io_uring_sq,
  flags: SetupFlags,
  fd: RawFd,
}

impl IoUring {
  /// Create an `IoUring` with the specified number of entries in both the
  /// completion and submission queues.
  pub fn new(entries: u32) -> Result<Self, Error> {
    Self::with_flags(entries, SetupFlags::empty())
  }

  /// Create an `IoUring` with the specified number of entries along with
  /// optional setup flags.
  ///
  /// Note that using some of the requires that corresponding fields be
  /// specified in a `UringParams` structure. In that case you'll want to
  /// use `with_params` instead.
  pub fn with_flags(entries: u32, flags: SetupFlags) -> Result<Self, Error> {
    let params = UringParams {
      flags,
      ..Default::default()
    };

    Self::with_params(entries, params)
  }

  pub fn with_params(entries: u32, params: UringParams) -> Result<Self, Error> {
    use crate::raw::*;

    let mut p = io_uring_params {
      flags: params.flags.bits(),
      cq_entries: params.cq_entries,
      sq_thread_cpu: params.sq_thread_cpu,
      sq_thread_idle: params.sq_thread_idle,
      ..unsafe { std::mem::zeroed() }
    };

    unsafe {
      let fd: RawFd = sysresult!(io_uring_setup(entries, &mut p))?;

      let (sq, cq) = match crate::detail::mmap_queues(fd, &p) {
        Ok(queues) => queues,
        Err(e) => {
          libc::close(fd);
          return Err(e);
        }
      };

      // We don't really make use of the indirection array so setting it up right
      // at the start simplifies things later on.
      let mask = *sq.kring_mask;
      for i in 0..(mask + 1) {
        *sq.array.add(i as usize) = i;
      }

      // Probably not necessary but makes absolutely sure that the kernel can see
      // the indirection array.
      std::sync::atomic::fence(std::sync::atomic::Ordering::Release);

      Ok(Self {
        sq,
        cq,
        fd,
        flags: params.flags,
      })
    }
  }

  pub fn sq(&mut self) -> SubmissionQueue<'_> {
    SubmissionQueue::new(self.fd, self.flags, &mut self.sq)
  }

  pub fn cq(&mut self) -> CompletionQueue<'_> {
    CompletionQueue::new(self.fd, &mut self.cq)
  }

  pub fn registrar(&self) -> Registrar<'_> {
    Registrar::new(&self.fd)
  }

  /// Probe for the supported operations.
  pub fn probe(&self) -> Result<Probe, Error> {
    self.registrar().probe()
  }

  pub fn split(&mut self) -> (SubmissionQueue<'_>, CompletionQueue<'_>, Registrar<'_>) {
    (
      SubmissionQueue::new(self.fd, self.flags, &mut self.sq),
      CompletionQueue::new(self.fd, &mut self.cq),
      Registrar::new(&self.fd),
    )
  }
}

impl Drop for IoUring {
  fn drop(&mut self) {
    unsafe {
      crate::detail::unmap_queues(&self.sq, &self.cq);
      libc::close(self.fd);
    }
  }
}

unsafe impl Send for IoUring {}
unsafe impl Sync for IoUring {}
