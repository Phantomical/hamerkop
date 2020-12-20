use crate::raw::*;
use crate::*;

use std::ffi::c_void;
use std::fmt;
use std::io::{IoSliceMut, Result};
use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::os::raw::c_uint;

// For backwards compatiblity we'll use this so that if
// `io_uring_files_update` gains additional fields then we don't have
// to change the signature of `Registrar::update_files`.
#[derive(Copy, Clone)]
pub struct FileUpdate<'fd> {
  offset: u32,
  fds: &'fd [RawFd],
}

impl<'fd> FileUpdate<'fd> {
  pub fn new(offset: u32, fds: &'fd [RawFd]) -> Self {
    Self { offset, fds }
  }
}

pub struct Registrar<'ring> {
  ringfd: RawFd,
  _marker: PhantomData<&'ring mut IoUring>,
}

impl<'ring> Registrar<'ring> {
  pub(crate) fn new(fd: &'ring RawFd) -> Self {
    Self {
      ringfd: *fd,
      _marker: PhantomData,
    }
  }

  unsafe fn register<T>(&self, opcode: u32, slice: &[T]) -> Result<()> {
    sysresult!(io_uring_register(
      self.ringfd as c_uint,
      opcode,
      slice.as_ptr() as *const _ as *mut c_void,
      slice.len() as _
    ))?;
    Ok(())
  }

  unsafe fn unregister(&self, opcode: u32) -> Result<()> {
    sysresult!(io_uring_register(
      self.ringfd as c_uint,
      opcode,
      std::ptr::null_mut(),
      0
    ))?;
    Ok(())
  }

  pub unsafe fn register_buffers(&self, buffers: &[IoSliceMut<'_>]) -> Result<()> {
    self.register(IORING_REGISTER_BUFFERS, buffers)
  }

  pub fn unregister_buffers(&self) -> Result<()> {
    unsafe { self.unregister(IORING_UNREGISTER_BUFFERS) }
  }

  pub fn register_files(&self, files: &[RawFd]) -> Result<()> {
    unsafe { self.register(IORING_REGISTER_FILES, files) }
  }

  pub fn unregister_files(&self) -> Result<()> {
    unsafe { self.unregister(IORING_UNREGISTER_FILES) }
  }

  pub fn update_files(&self, upd: FileUpdate) -> Result<()> {
    let FileUpdate { fds, offset } = upd;

    assert!(fds.len() <= u32::MAX as usize);

    unsafe {
      let mut updates = io_uring_files_update {
        offset,
        fds: fds.as_ptr() as usize as u64,
        ..std::mem::zeroed()
      };

      sysresult!(io_uring_register(
        self.ringfd as c_uint,
        IORING_REGISTER_FILES_UPDATE,
        &mut updates as *mut _ as *mut c_void,
        fds.len() as u32
      ))?;

      Ok(())
    }
  }

  pub fn register_eventfd(&self, fd: &RawFd) -> Result<()> {
    unsafe {
      sysresult!(io_uring_register(
        self.ringfd as c_uint,
        IORING_REGISTER_EVENTFD,
        fd as *const _ as *mut c_void,
        1
      ))?;
      Ok(())
    }
  }

  pub fn register_eventfd_async(&self, fd: &RawFd) -> Result<()> {
    unsafe {
      sysresult!(io_uring_register(
        self.ringfd as c_uint,
        IORING_REGISTER_EVENTFD_ASYNC,
        fd as *const _ as *mut c_void,
        1
      ))?;
      Ok(())
    }
  }

  pub fn unregister_eventfd(&self) -> Result<()> {
    unsafe { self.unregister(IORING_UNREGISTER_EVENTFD) }
  }

  pub fn probe(&self) -> Result<Probe> {
    unsafe {
      let mut probe = MaybeUninit::uninit();

      sysresult!(io_uring_register(
        self.ringfd as c_uint,
        IORING_REGISTER_PROBE,
        probe.as_mut_ptr() as *mut _,
        Probe::NUM_OPS as u32
      ))?;

      Ok(probe.assume_init())
    }
  }

  pub fn register_personality(&self) -> Result<c_uint> {
    Ok(sysresult!(unsafe {
      io_uring_register(
        self.ringfd as c_uint,
        IORING_REGISTER_PERSONALITY,
        std::ptr::null_mut(),
        0,
      )
    })? as c_uint)
  }

  pub fn unregister_personality(&self, personality: c_uint) -> Result<()> {
    sysresult!(unsafe {
      io_uring_register(
        self.ringfd as c_uint,
        IORING_UNREGISTER_PERSONALITY,
        std::ptr::null_mut(),
        personality as _,
      )
    })?;

    Ok(())
  }
}

#[repr(packed)]
pub struct Probe {
  probe: io_uring_probe,
  _ops: [MaybeUninit<io_uring_probe_op>; Self::NUM_OPS],
}

impl Probe {
  const NUM_OPS: usize = IORING_OP_LAST as usize;

  pub fn last_op(&self) -> u8 {
    self.probe.last_op
  }

  pub fn ops(&self) -> &[ProbeOp] {
    unsafe {
      std::slice::from_raw_parts(
        self.probe.ops.as_ptr() as *const ProbeOp,
        self.probe.ops_len as usize,
      )
    }
  }

  pub fn ops_mut(&mut self) -> &mut [ProbeOp] {
    unsafe {
      std::slice::from_raw_parts_mut(
        self.probe.ops.as_mut_ptr() as *mut ProbeOp,
        self.probe.ops_len as usize,
      )
    }
  }
}

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct ProbeOp {
  op: io_uring_probe_op,
}

impl ProbeOp {
  pub fn from_raw(op: io_uring_probe_op) -> Self {
    Self { op }
  }

  pub fn into_raw(self) -> io_uring_probe_op {
    self.op
  }

  pub fn raw(&self) -> &io_uring_probe_op {
    &self.op
  }

  pub fn raw_mut(&mut self) -> &mut io_uring_probe_op {
    &mut self.op
  }

  pub fn op(&self) -> u8 {
    self.op.op
  }

  pub fn flags(&self) -> ProbeOpFlags {
    unsafe { ProbeOpFlags::from_bits_unchecked(self.op.flags) }
  }
}

struct OpFormatter(u8);

macro_rules! static_max {
    ($t:ty : $arg:expr $(,)?) => {
        $arg
    };
    ($ty:ty : $first:expr, $( $rest:expr ),* $(,)?) => {{
        let first = $first;
        let rest = static_max!($ty : $( $rest ),*);
        let bit = (first < rest) as $ty;
        // 0 if first < rest, -1 otherwise
        let mask = bit.wrapping_sub(1);

        (first & mask) | (rest & !mask)
    }}
}

macro_rules! filled_array {
    {
        $( $key:expr => $val:expr, )*
        _ => $default:expr
    } => {{
        const SIZE: usize = static_max!(usize : $( $key as usize ),*) + 1;
        let mut arr = [$default; SIZE];
        $(
            arr[$key as usize] = $val;
        )*
        arr
    }}
}

#[rustfmt::skip]
impl fmt::Debug for OpFormatter {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        const IDENTS: [&str; IORING_OP_LAST as usize] = filled_array!{
            IORING_OP_NOP               => "nop",
            IORING_OP_READV             => "readv",
            IORING_OP_WRITEV            => "writev",
            IORING_OP_FSYNC             => "fsync",
            IORING_OP_READ_FIXED        => "read_fixed",
            IORING_OP_WRITE_FIXED       => "write_fixed",
            IORING_OP_POLL_ADD          => "poll_add",
            IORING_OP_POLL_REMOVE       => "poll_remove",
            IORING_OP_SYNC_FILE_RANGE   => "sync_file_range",
            IORING_OP_SENDMSG           => "sendmsg",
            IORING_OP_RECVMSG           => "recvmsg",
            IORING_OP_TIMEOUT           => "timeout",
            IORING_OP_TIMEOUT_REMOVE    => "timeout_remove",
            IORING_OP_ACCEPT            => "accept",
            IORING_OP_ASYNC_CANCEL      => "async_cancel",
            IORING_OP_LINK_TIMEOUT      => "link_timeout",
            IORING_OP_CONNECT           => "connect",
            IORING_OP_FALLOCATE         => "fallocate",
            IORING_OP_OPENAT            => "openat",
            IORING_OP_CLOSE             => "close",
            IORING_OP_FILES_UPDATE      => "files_update",
            IORING_OP_STATX             => "statx",
            IORING_OP_READ              => "read",
            IORING_OP_WRITE             => "write",
            IORING_OP_FADVISE           => "fadvise",
            IORING_OP_MADVISE           => "madvise",
            IORING_OP_SEND              => "send",
            IORING_OP_RECV              => "recv",
            IORING_OP_OPENAT2           => "openat2",
            IORING_OP_EPOLL_CTL         => "epoll_ctl",
            IORING_OP_SPLICE            => "splice",
            IORING_OP_PROVIDE_BUFFERS   => "provide_buffers",
            IORING_OP_REMOVE_BUFFERS    => "remove_buffers",
            IORING_OP_TEE               => "tee",
            _ => "unknown"
        };

        let ident = IDENTS.get(self.0 as usize).copied().unwrap_or("unknown"); 

        write!(fmt, "{} ({})", ident, self.0)
    }
}

impl fmt::Debug for ProbeOp {
  fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
    fmt
      .debug_struct("ProbeOp")
      .field("op", &OpFormatter(self.op()))
      .field("flags", &self.flags())
      .finish()
  }
}
