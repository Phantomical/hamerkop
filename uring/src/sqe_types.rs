use crate::detail::SaturatingCast;
use crate::raw;
use crate::{RawFd, SubmissionFlags, SubmissionQueueEvent, TimeoutFlags};

use libc::{
  c_char, c_int, c_void, epoll_event, iovec, loff_t, mode_t, off64_t, off_t, sockaddr, socklen_t,
  statx,
};

use std::io;

pub trait Prepare {
  type Return: Resultify;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent);
}

pub trait Resultify: Sized {
  fn from_result(result: c_int) -> io::Result<Self>;
}

#[derive(Copy, Clone, Debug)]
pub enum Target {
  Fd(RawFd),
  Fixed(u32),
}

impl Target {
  pub fn is_fixed(&self) -> bool {
    match self {
      Self::Fixed(_) => true,
      _ => false,
    }
  }
}

pub type Timespec = crate::raw::__kernel_timespec;
pub type RwFlags = crate::raw::__kernel_rwf_t;

impl Target {
  pub(self) fn into_raw(self) -> i32 {
    match self {
      Self::Fd(fd) => fd,
      Self::Fixed(fixed) => fixed as i32,
    }
  }
}

macro_rules! default_impl {
  ($type:ty ; ) => {
    impl Default for $type {
      fn default() -> Self {
        Self::new()
      }
    }
  };
  ($type:ty ; $( $args:tt )*) => {};
}

macro_rules! opcode_impl {
    {
        $(
            $( #[$attr:meta] )*
            $vis:vis struct $name:ident {
                $( #[$newdoc:meta] )*
                const OPCODE = $opcode:expr ;
                $( $field:ident : $fieldty:ty, )*
                ;;
                $(
                    $( #[$opt_attr:meta] )*
                    $opt_field:ident : $opt_ty:ty = $default:expr,
                )*
            }
        )*
    } => {
        $(
            $( #[$attr] )*
            #[derive(Copy, Clone, Debug)]
            $vis struct $name {
                $( $field: $fieldty, )*
                $( $opt_field: $opt_ty, )*
            }

            impl $name {
                pub const OPCODE: u8 = $opcode;

                $( #[$newdoc] )*
                #[inline]
                pub const fn new($( $field : $fieldty ),*) -> Self {
                    Self {
                        $( $field, )*
                        $( $opt_field: $default, )*
                    }
                }

                $(
                    $( #[$opt_attr] )*
                    #[inline]
                    pub const fn $opt_field(mut self, value: $opt_ty) -> Self {
                        self.$opt_field = value;
                        self
                    }
                )*
            }

            default_impl!($name ; $( $field : $fieldty ),*);
        )*

        #[cfg(test)]
        mod prepare {
            use super::*;

            $(
                #[test]
                #[allow(clippy::all, non_snake_case)]
                fn $name() {
                    let mut sqe: crate::raw::io_uring_sqe = unsafe { ::core::mem::zeroed() };
                    let sqe = crate::SubmissionQueueEvent::new(&mut sqe);
                    let me: $name = unsafe { ::core::mem::zeroed() };

                    me.prep_sqe(sqe);

                    assert_eq!(sqe.raw().opcode, <$name>::OPCODE);
                }
            )*
        }
    }
}

macro_rules! opcode {
    {
        $(
            $( #[$attr:meta] )*
            $vis:vis struct $name:ident {
                $( #[$newdoc:meta] )*
                const OPCODE = $opcode:expr ;
                $( $field:ident : $fieldty:ty ),* $(,)?
                $(
                    ;;
                    $(
                        $( #[$opt_attr:meta] )*
                        $opt_field:ident : $opt_ty:ty = $default:expr
                    ),* $(,)?
                )?
            }
        )*
    } => {
        opcode_impl! {
            $(
                $( #[$attr] )*
                $vis struct $name {
                    $( #[$newdoc] )*
                    const OPCODE = $opcode ;
                    $( $field : $fieldty, )*
                    ;;
                    $($(
                        $( #[$opt_attr] )*
                        $opt_field : $opt_ty = $default,
                    )*)?
                }
            )*
        }
    }
}

opcode! {
    /// Do not perform any I/O.
    ///
    /// This may be useful for testing the performance of the
    /// `io_uring` implementation itself.
    pub struct Nop {
        const OPCODE = raw::IORING_OP_NOP;
    }

    /// Implements a vectored read operation, equivalent to `preadv2(2)`.
    pub struct ReadVectored {
        const OPCODE = raw::IORING_OP_READV;

        fd: Target,
        iovec: *mut iovec,
        len: u32,
        ;;
        ioprio: u16   = 0,
        offset: off_t = 0,
        rw_flags: RwFlags = 0,
    }

    /// Implements a vectored write operation, equivalent to `pwritev2(2)`.
    pub struct WriteVectored {
        const OPCODE = raw::IORING_OP_WRITEV;

        fd: Target,
        iovec: *const iovec,
        len: u32,
        ;;
        ioprio: u16 = 0,
        offset: off_t = 0,
        rw_flags: RwFlags = 0,
    }

    /// Read to pre-mapped buffers.
    ///
    /// Buffers must be setup with the `IoUring` instance beforehand.
    pub struct ReadFixed {
        const OPCODE = raw::IORING_OP_READ_FIXED;

        fd: Target,
        buf: *mut u8,
        len: u32,
        buf_index: u16
        ;;
        offset: off_t = 0,
        ioprio: u16   = 0,
        rw_flags: RwFlags = 0
    }

    /// Write from pre-mapped buffers.
    ///
    /// Buffers must have been setup with the `IoUring` instance
    /// beforehand.
    pub struct WriteFixed {
        const OPCODE = raw::IORING_OP_WRITE_FIXED;

        fd: Target,
        buf: *const u8,
        len: u32,
        buf_index: u16
        ;;
        ioprio: u16   = 0,
        offset: off_t = 0,
        rw_flags: RwFlags = 0
    }

    /// Poll the specified fd.
    ///
    /// Unlike poll or epoll without `EPOLLONESHOT`, this interface
    /// always works in one shot mode. That is, once the poll operation
    /// is completed, it will have to be resubmitted.
    pub struct PollAdd {
        /// The bits that may be set in `flags` are defined in `<poll.h>`
        /// and documented in `poll(2)`.
        const OPCODE = raw::IORING_OP_POLL_ADD;

        fd:          Target,
        poll_events: u16
    }

    /// Remove an existing poll request.
    ///
    /// If not found, then the result will be `0`. Otherwise the CQE
    /// will error with `ENOENT`.
    pub struct PollRemove {
        const OPCODE = raw::IORING_OP_POLL_REMOVE;

        user_data: u64
    }

    /// Issue the equivalent of a `sync_file_range(2)` on the file
    /// descriptor.
    ///
    /// See also `sync_file_range(2)` for the general description
    /// of the related system call.
    pub struct SyncFileRange {
        const OPCODE = raw::IORING_OP_SYNC_FILE_RANGE;

        fd:  Target,
        len: u32
        ;;
        /// The offset in bytes into the file
        offset: off64_t = 0,
        flags: u32 = 0
    }

    /// Issue the equivalent of a `sendmsg(2)` system call.
    ///
    /// See also `sendmsg(2)` for the general description of the
    /// related syscall.
    pub struct SendMsg {
        const OPCODE = raw::IORING_OP_SENDMSG;

        fd:  Target,
        msg: *const libc::msghdr
        ;;
        ioprio: u16 = 0,
        flags:  u32 = 0
    }

    /// Issue the equivalent of a `recvmsg(2)` system call.
    ///
    /// See also `sendmsg(2)` for the general description of the
    /// related syscall.
    pub struct RecvMsg {
        const OPCODE = raw::IORING_OP_RECVMSG;

        fd:  Target,
        msg: *mut libc::msghdr,
        ;;
        ioprio: u16 = 0,
        flags:  u32 = 0
    }

    /// Register a timeout operation.
    ///
    /// A timeout will trigger a wakeup event on the completion ring
    /// for anyone waiting for events. A timeout condition is met when
    /// either the specified timeout expires, or the specified number
    /// of events have completed. Either condition will trigger the
    /// event. The request will error with `ETIME` if the timeout got
    /// completed through expiration of the timer or return 0 if the timeout
    /// got completed through requests completing on their own.
    /// If the timeout was cancelled before it expired, the request will
    /// error with `ECANCELLED`.
    pub struct Timeout {
        const OPCODE = raw::IORING_OP_TIMEOUT;

        timespec: *const Timespec
        ;;
        /// `count` may contain a completion event count. If not set,
        /// this defaults to 1.
        count: u32 = 0,
        /// `flags` may contain [`TimeoutFlags::ABS`] for an absolute
        /// timeout value, or 0 for a relative timeout value.
        ///
        /// [`TimeoutFlags::ABS`]: crate::flags::TimeoutFlags::ABS
        flags: TimeoutFlags = TimeoutFlags::empty()
    }

    /// Attempt to remove an existing timeout operation
    pub struct TimeoutRemove {
        const OPCODE = raw::IORING_OP_TIMEOUT_REMOVE;

        user_data: u64
        ;;
        flags: TimeoutFlags = TimeoutFlags::empty()
    }

    /// Issue the equivalent of an `accept4(2)` syscall.
    pub struct Accept {
        const OPCODE = raw::IORING_OP_ACCEPT;

        fd: Target,
        addr: *mut sockaddr,
        addrlen: *mut socklen_t
        ;;
        flags: u32 = 0
    }

    /// Attempt to cancel an alread issued request.
    pub struct AsyncCancel {
        const OPCODE = raw::IORING_OP_ASYNC_CANCEL;

        user_data: u64
        ;;
        flags: u32 = 0
    }

    /// Cancel the linked request after a specific amount of time.
    ///
    /// This request must be linked with another request through
    /// [`SubmissionFlags::IO_LINK`][0]. Unlike [`Timeout`][1],
    /// `LinkTimeout` acts on the linked request, not the completion
    /// queue.
    ///
    /// [0]: crate::flags::SubmissionFlags::IO_LINK
    /// [1]: crate::sqes::Timeout
    pub struct LinkTimeout {
        const OPCODE = raw::IORING_OP_LINK_TIMEOUT;

        timespec: *const Timespec,
        ;;
        flags: TimeoutFlags = TimeoutFlags::empty()
    }

    /// Issue the equivalent of a `connect(2)` syscall.
    pub struct Connect {
        const OPCODE = raw::IORING_OP_CONNECT;

        fd: Target,
        addr: *const sockaddr,
        addrlen: *const socklen_t
    }

    /// Issue the equivalent of a `fallocate(2)` syscall.
    pub struct Fallocate {
        const OPCODE = raw::IORING_OP_FALLOCATE;

        fd: Target,
        len: u32,
        ;;
        offset: off_t = 0,
        mode: i32 = 0
    }

    /// Issue the equivalent of a `posix_fadvise(2)` syscall.
    pub struct Fadvise {
        const OPCODE = raw::IORING_OP_FADVISE;

        fd: Target,
        len: off_t,
        advice: i32,
        ;;
        offset: off_t = 0
    }

    /// Issue the equivalent of a `madvise(2)` syscall.
    pub struct Madvise {
        const OPCODE = raw::IORING_OP_MADVISE;

        addr: *const c_void,
        len: off_t,
        advice: i32
    }

    /// Issue the equivalent of an `openat(2)` syscall.
    pub struct OpenAt {
        const OPCODE = raw::IORING_OP_OPENAT;

        dirfd: RawFd,
        pathname: *const c_char,
        ;;
        flags: i32 = 0,
        mode: mode_t = 0
    }

    /// Issue the equvalent of an `openat2(2)` syscall.
    pub struct OpenAt2 {
        const OPCODE = raw::IORING_OP_OPENAT2;

        dirfd: RawFd,
        pathname: *const char,
        how: *const raw::open_how,
        ;;
        len: u32 = std::mem::size_of::<raw::open_how>() as _,
    }

    /// Issue the equivalent of a `close(2)` syscall.
    pub struct Close {
        const OPCODE = raw::IORING_OP_CLOSE;

        fd: RawFd,
    }

    /// Issue the equivalent of a `statx(2)` syscall.
    pub struct Statx {
        const OPCODE = raw::IORING_OP_STATX;

        dirfd: RawFd,
        pathname: *const c_char,
        statxbuf: *mut statx,
        ;;
        flags: i32 = 0,
        mask: u32  = 0
    }

    /// Issue the equivalent of a `read(2)` syscall.
    pub struct Read {
        const OPCODE = raw::IORING_OP_READ;

        fd: Target,
        buf: *mut u8,
        len: u32,
        ;;
        offset: off_t = 0,
        ioprio: u16 = 0,
        rw_flags: RwFlags = 0
    }

    /// Issue the equivalent of a `write(2)` syscall.
    pub struct Write {
        const OPCODE = raw::IORING_OP_WRITE;

        fd: Target,
        buf: *const u8,
        len: u32,
        ;;
        offset: off_t = 0,
        ioprio: u16 = 0,
        rw_flags: RwFlags = 0
    }

    /// Issue the equivalent of a `send(2)` syscall.
    pub struct Send {
        const OPCODE = raw::IORING_OP_SEND;

        fd: Target,
        buf: *const u8,
        len: u32,
        ;;
        flags: i32 = 0
    }

    /// Issue the equivalent of a `recv(2)` syscall.
    pub struct Recv {
        const OPCODE = raw::IORING_OP_RECV;

        fd: Target,
        buf: *mut u8,
        len: u32,
        ;;
        flags: i32 = 0
    }

    /// Issue the equivalent of an `epoll_ctl(2)` syscall.
    pub struct EpollCtl {
        const OPCODE = raw::IORING_OP_EPOLL_CTL;

        epfd: Target,
        fd: RawFd,
        op: i32,
        ev: *const epoll_event
    }

    /// Issue the equivalent of a `splice(2)` syscall.
    pub struct Splice {
        const OPCODE = raw::IORING_OP_SPLICE;

        fd_in: Target,
        off_in: loff_t,
        fd_out: Target,
        off_out: loff_t,
        len: u32
        ;;
        flags: u32 = 0
    }

    /// Issue the equivalent of a `tee(2)` syscall.
    pub struct Tee {
        const OPCODE = raw::IORING_OP_TEE;

        fd_in:  Target,
        fd_out: Target,
        len:    u32,
        ;;
        flags: u32 = 0
    }

    /// Equivalent to performing `register_files_update` for the uring
    /// but in an async fashion.
    pub struct FilesUpdate {
        const OPCODE = raw::IORING_OP_FILES_UPDATE;

        fds: *const RawFd,
        len: u32,
        ;;
        offset: u32 = 0
    }

    /// This comand allows an application to register a group of buffers to be
    /// used by commands that read/receive data.
    ///
    /// Using buffers in this manner can eliminate the need to separate pool + read,
    /// which provides a convenient point in time to allocate a buffer for a given
    /// request. It's often infeasible to have as many buffers available as pending
    /// reads or receive. With this feature, the application can have its pool of
    /// buffers ready in the kernel, and when the file or socket is ready to
    /// read/receive data, a buffer can be selected for the operation.
    ///
    /// Buffers are grouped by the group ID, and each buffer within this group will
    /// be identical in size according to the above arguments. This allows the
    /// application to provide different groups of above arguments, and this is
    /// often used to have differently sized buffers available depending on what the
    /// expectations are of an individual request.
    ///
    /// Available since 5.7.
    pub struct ProvideBuffers {
        const OPCODE = raw::IORING_OP_PROVIDE_BUFFERS;

        addr: *mut c_void,
        len: u32,
        nbufs: u16,
        buf_group: u16,
        buf_id: u16
    }

    /// Remove buffers previously registered with `ProvideBuffers`.
    pub struct RemoveBuffers {
        const OPCODE = raw::IORING_OP_REMOVE_BUFFERS;

        nbufs: u16,
        buf_group: u16
    }


}

impl Prepare for Nop {
  type Return = ();

  #[inline]
  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    sqe.clear();
    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = -1;
  }
}
impl Prepare for ReadVectored {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: self.fd.into_raw(),
      len: self.len,
      ioprio: self.ioprio,
      addr_splice: raw::addr_splice {
        addr: self.iovec as u64,
      },
      off_addr2: raw::off_addr2 {
        off: self.offset as _,
      },
      ..unsafe { std::mem::zeroed() }
    };

    if self.fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for WriteVectored {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: self.fd.into_raw(),
      len: self.len,
      ioprio: self.ioprio,
      addr_splice: raw::addr_splice {
        addr: self.iovec as u64,
      },
      off_addr2: raw::off_addr2 {
        off: self.offset as _,
      },
      ..unsafe { std::mem::zeroed() }
    };

    if self.fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for ReadFixed {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: self.fd.into_raw(),
      len: self.len,
      ioprio: self.ioprio,
      addr_splice: raw::addr_splice {
        addr: self.buf as u64,
      },
      off_addr2: raw::off_addr2 {
        off: self.offset as _,
      },
      extra: raw::sqe_extra {
        buf_info: raw::sqe_buf_info {
          buf_index: self.buf_index,
        },
        ..unsafe { std::mem::zeroed() }
      },
      ..unsafe { std::mem::zeroed() }
    };

    if self.fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for WriteFixed {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: self.fd.into_raw(),
      len: self.len,
      ioprio: self.ioprio,
      addr_splice: raw::addr_splice {
        addr: self.buf as u64,
      },
      off_addr2: raw::off_addr2 {
        off: self.offset as _,
      },
      extra: raw::sqe_extra {
        buf_info: raw::sqe_buf_info {
          buf_index: self.buf_index,
        },
        ..unsafe { std::mem::zeroed() }
      },
      ..unsafe { std::mem::zeroed() }
    };

    if self.fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for PollAdd {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: self.fd.into_raw(),
      op_flags: raw::sqe_op_flags {
        poll_events: self.poll_events,
      },
      ..unsafe { std::mem::zeroed() }
    };

    if self.fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for PollRemove {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: -1,
      addr_splice: raw::addr_splice {
        addr: self.user_data,
      },
      ..unsafe { std::mem::zeroed() }
    };
  }
}
impl Prepare for SyncFileRange {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: self.fd.into_raw(),
      len: self.len,
      op_flags: raw::sqe_op_flags {
        sync_range_flags: self.flags,
      },
      off_addr2: raw::off_addr2 {
        off: self.offset as _,
      },
      ..unsafe { std::mem::zeroed() }
    };

    if self.fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for SendMsg {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: self.fd.into_raw(),
      addr_splice: raw::addr_splice {
        addr: self.msg as u64,
      },
      op_flags: raw::sqe_op_flags {
        msg_flags: self.flags,
      },
      ..unsafe { std::mem::zeroed() }
    };

    if self.fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for RecvMsg {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: self.fd.into_raw(),
      addr_splice: raw::addr_splice {
        addr: self.msg as u64,
      },
      op_flags: raw::sqe_op_flags {
        msg_flags: self.flags,
      },
      ..unsafe { std::mem::zeroed() }
    };

    if self.fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for Timeout {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: -1,
      addr_splice: raw::addr_splice {
        addr: self.timespec as _,
      },
      len: 1,
      off_addr2: raw::off_addr2 {
        off: self.count as _,
      },
      op_flags: raw::sqe_op_flags {
        timeout_flags: self.flags.bits(),
      },
      ..unsafe { std::mem::zeroed() }
    };
  }
}
impl Prepare for TimeoutRemove {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: -1,
      addr_splice: raw::addr_splice {
        addr: self.user_data as _,
      },
      op_flags: raw::sqe_op_flags {
        timeout_flags: self.flags.bits(),
      },
      ..unsafe { std::mem::zeroed() }
    };
  }
}
impl Prepare for Accept {
  type Return = RawFd;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      fd,
      addr,
      addrlen,
      flags,
    } = self;

    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: fd.into_raw(),
      addr_splice: raw::addr_splice { addr: addr as _ },
      off_addr2: raw::off_addr2 {
        addr2: addrlen as _,
      },
      op_flags: raw::sqe_op_flags {
        accept_flags: flags,
      },
      ..unsafe { std::mem::zeroed() }
    };

    if fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for AsyncCancel {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self { user_data, flags } = self;

    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: -1,
      addr_splice: raw::addr_splice { addr: user_data },
      op_flags: raw::sqe_op_flags {
        cancel_flags: flags,
      },
      ..unsafe { std::mem::zeroed() }
    };
  }
}
impl Prepare for LinkTimeout {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self { timespec, flags } = self;

    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: -1,
      addr_splice: raw::addr_splice {
        addr: timespec as _,
      },
      op_flags: raw::sqe_op_flags {
        timeout_flags: flags.bits(),
      },
      ..unsafe { std::mem::zeroed() }
    };
  }
}
impl Prepare for Connect {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self { fd, addr, addrlen } = self;

    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: fd.into_raw(),
      addr_splice: raw::addr_splice { addr: addr as _ },
      off_addr2: raw::off_addr2 { off: addrlen as _ },
      ..unsafe { std::mem::zeroed() }
    };

    if fd.is_fixed() {
      sqe.set_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for Fallocate {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      fd,
      len,
      offset,
      mode,
    } = self;

    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: fd.into_raw(),
      off_addr2: raw::off_addr2 { off: offset as _ },
      len: mode as _,
      addr_splice: raw::addr_splice { addr: len as _ },
      ..unsafe { std::mem::zeroed() }
    };

    if fd.is_fixed() {
      sqe.add_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for Fadvise {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      fd,
      len,
      advice,
      offset,
    } = self;

    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: fd.into_raw(),
      len: len.saturating_cast(),
      off_addr2: raw::off_addr2 { off: offset as _ },
      op_flags: raw::sqe_op_flags {
        fadvise_advice: advice as _,
      },
      ..unsafe { std::mem::zeroed() }
    };

    if fd.is_fixed() {
      sqe.add_flags(SubmissionFlags::FIXED_FILE);
    }
  }
}
impl Prepare for Madvise {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self { addr, len, advice } = self;

    *sqe.raw_mut() = raw::io_uring_sqe {
      opcode: Self::OPCODE,
      fd: -1,
      addr_splice: raw::addr_splice { addr: addr as _ },
      len: len.saturating_cast(),
      op_flags: raw::sqe_op_flags {
        fadvise_advice: advice as _,
      },
      ..unsafe { std::mem::zeroed() }
    };
  }
}
impl Prepare for OpenAt {
  type Return = RawFd;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      dirfd,
      pathname,
      flags,
      mode,
    } = self;

    sqe.clear();
    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = dirfd;
    sqe.addr_splice.addr = pathname as _;
    sqe.len = mode;
    sqe.op_flags.open_flags = flags as _;
  }
}
impl Prepare for OpenAt2 {
  type Return = RawFd;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      dirfd,
      pathname,
      how,
      len,
    } = self;

    sqe.clear();
    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = dirfd;
    sqe.addr_splice.addr = pathname as _;
    sqe.len = len;
    sqe.off_addr2.off = how as _;
  }
}
impl Prepare for Close {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self { fd } = self;

    sqe.clear();
    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = fd;
  }
}
impl Prepare for Statx {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      dirfd,
      pathname,
      statxbuf,
      flags,
      mask,
    } = self;

    sqe.clear();
    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = dirfd;
    sqe.addr_splice.addr = pathname as _;
    sqe.len = mask;
    sqe.off_addr2.off = statxbuf as _;
    sqe.op_flags.statx_flags = flags as _;
  }
}
impl Prepare for Read {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      fd,
      buf,
      len,
      offset,
      ioprio,
      rw_flags,
    } = self;

    sqe.clear();
    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = fd.into_raw();
    sqe.ioprio = ioprio;
    sqe.addr_splice.addr = buf as _;
    sqe.len = len;
    sqe.off_addr2.off = offset as _;
    sqe.op_flags.rw_flags = rw_flags;

    if fd.is_fixed() {
      sqe.flags |= SubmissionFlags::FIXED_FILE.bits();
    }
  }
}
impl Prepare for Write {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      fd,
      buf,
      len,
      offset,
      ioprio,
      rw_flags,
    } = self;

    sqe.clear();

    if fd.is_fixed() {
      sqe.add_flags(SubmissionFlags::FIXED_FILE);
    }

    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = fd.into_raw();
    sqe.ioprio = ioprio;
    sqe.addr_splice.addr = buf as _;
    sqe.len = len;
    sqe.off_addr2.off = offset as _;
    sqe.op_flags.rw_flags = rw_flags;
  }
}
impl Prepare for Send {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      fd,
      buf,
      len,
      flags,
    } = self;

    sqe.clear();

    if fd.is_fixed() {
      sqe.add_flags(SubmissionFlags::FIXED_FILE);
    }

    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = fd.into_raw();
    sqe.addr_splice.addr = buf as _;
    sqe.len = len;
    sqe.op_flags.msg_flags = flags as _;
  }
}
impl Prepare for Recv {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      fd,
      buf,
      len,
      flags,
    } = self;

    sqe.clear();

    if fd.is_fixed() {
      sqe.add_flags(SubmissionFlags::FIXED_FILE);
    }

    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = fd.into_raw();
    sqe.addr_splice.addr = buf as _;
    sqe.len = len;
    sqe.op_flags.msg_flags = flags as _;
  }
}
impl Prepare for EpollCtl {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self { epfd, fd, op, ev } = self;

    sqe.clear();

    if epfd.is_fixed() {
      sqe.add_flags(SubmissionFlags::FIXED_FILE);
    }

    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = epfd.into_raw();
    sqe.addr_splice.addr = ev as _;
    sqe.len = op as _;
    sqe.off_addr2.off = fd as _;
  }
}
impl Prepare for Splice {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      fd_in,
      off_in,
      fd_out,
      off_out,
      len,
      mut flags,
    } = self;

    sqe.clear();

    if fd_out.is_fixed() {
      sqe.add_flags(SubmissionFlags::FIXED_FILE);
    }

    if fd_in.is_fixed() {
      flags |= raw::SPLICE_F_FD_IN_FIXED;
    }

    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = fd_out.into_raw();
    sqe.len = len;
    sqe.off_addr2.off = off_out as _;
    sqe.extra.splice_fd_in = fd_in.into_raw();
    sqe.addr_splice.splice_off_in = off_in as _;
    sqe.op_flags.splice_flags = flags;
  }
}
impl Prepare for Tee {
  type Return = usize;

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      fd_in,
      fd_out,
      len,
      mut flags,
    } = self;

    sqe.clear();

    if fd_out.is_fixed() {
      sqe.add_flags(SubmissionFlags::FIXED_FILE);
    }

    if fd_in.is_fixed() {
      flags |= raw::SPLICE_F_FD_IN_FIXED;
    }

    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = fd_out.into_raw();
    sqe.len = len;
    sqe.extra.splice_fd_in = fd_in.into_raw();
    sqe.op_flags.splice_flags = flags;
  }
}
impl Prepare for FilesUpdate {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self { fds, len, offset } = self;

    sqe.clear();
    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = -1;
    sqe.addr_splice.addr = fds as _;
    sqe.len = len;
    sqe.off_addr2.off = offset as _;
  }
}
impl Prepare for ProvideBuffers {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self {
      addr,
      len,
      nbufs,
      buf_group,
      buf_id,
    } = self;

    sqe.clear();
    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = nbufs as _;
    sqe.addr_splice.addr = addr as _;
    sqe.len = len as _;
    sqe.off_addr2.off = buf_id as _;
    sqe.extra.buf_info.buf_group = buf_group;
  }
}
impl Prepare for RemoveBuffers {
  type Return = ();

  fn prep_sqe(self, sqe: &mut SubmissionQueueEvent) {
    let Self { nbufs, buf_group } = self;

    sqe.clear();
    let sqe = sqe.raw_mut();
    sqe.opcode = Self::OPCODE;
    sqe.fd = nbufs as _;
    sqe.extra.buf_info.buf_group = buf_group;
  }
}

//===============================================
// Result types
//===============================================

impl Resultify for () {
  fn from_result(result: c_int) -> io::Result<Self> {
    sysresult!(result).map(|_| ())
  }
}
impl Resultify for usize {
  fn from_result(result: c_int) -> io::Result<Self> {
    sysresult!(result).map(|val| val as _)
  }
}
impl Resultify for RawFd {
  fn from_result(result: c_int) -> io::Result<Self> {
    sysresult!(result).map(|val| val as _)
  }
}

// trace_macros!(false);
