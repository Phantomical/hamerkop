#![allow(non_camel_case_types, clippy::all)]

use libc::{c_int, c_longlong, c_uint, c_void, sigset_t};

/// Helper macro for declaring sequential constants.
///
/// Makes use of a hidden enum to actually declare the constant values.
macro_rules! declare_sequential {
    {
        enum $name:ident : $base:ident;
        $( $( #[$meta:meta] )* $vis:vis const $case:ident; )*
    } => {
        #[repr($base)]
        enum $name {
            $( $case, )*
        }

        $(
            $( #[$meta] )*
            $vis const $case : $base = $name::$case as $base;
        )*
    }
}

pub type __kernel_rwf_t = libc::c_int;

#[repr(C)]
#[derive(Default)]
pub struct __IncompleteArrayField<T>(::core::marker::PhantomData<T>, [T; 0]);
impl<T> __IncompleteArrayField<T> {
  #[inline]
  pub const fn new() -> Self {
    __IncompleteArrayField(::std::marker::PhantomData, [])
  }
  #[inline]
  pub unsafe fn as_ptr(&self) -> *const T {
    ::std::mem::transmute(self)
  }
  #[inline]
  pub unsafe fn as_mut_ptr(&mut self) -> *mut T {
    ::std::mem::transmute(self)
  }
  #[inline]
  pub unsafe fn as_slice(&self, len: usize) -> &[T] {
    ::std::slice::from_raw_parts(self.as_ptr(), len)
  }
  #[inline]
  pub unsafe fn as_mut_slice(&mut self, len: usize) -> &mut [T] {
    ::std::slice::from_raw_parts_mut(self.as_mut_ptr(), len)
  }
}
impl<T> ::core::fmt::Debug for __IncompleteArrayField<T> {
  fn fmt(&self, fmt: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
    fmt.write_str("__IncompleteArrayField")
  }
}
impl<T> ::core::clone::Clone for __IncompleteArrayField<T> {
  #[inline]
  fn clone(&self) -> Self {
    Self::new()
  }
}

pub struct open_how {
  pub flags: u64,
  pub mode: u64,
  pub resolve: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct io_uring_sqe {
  pub opcode: u8,
  pub flags: u8,
  pub ioprio: u16,
  pub fd: i32,
  pub off_addr2: off_addr2,
  pub addr_splice: addr_splice,
  pub len: u32,
  pub op_flags: sqe_op_flags,
  pub user_data: u64,
  pub extra: sqe_extra,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union off_addr2 {
  pub off: u64,
  pub addr2: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union addr_splice {
  pub addr: u64,
  pub splice_off_in: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub union sqe_op_flags {
  pub rw_flags: __kernel_rwf_t,
  pub fsync_flags: u32,
  pub poll_events: u16,
  pub sync_range_flags: u32,
  pub msg_flags: u32,
  pub timeout_flags: u32,
  pub accept_flags: u32,
  pub cancel_flags: u32,
  pub open_flags: u32,
  pub statx_flags: u32,
  pub fadvise_advice: u32,
  pub splice_flags: u32,
}

// Note: The C header uses an anonymous union to ensure that padding
//       is correct. However for ergonomics purposes I've flattened
//       this into a struct and adjusted the padding accordingly.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct sqe_extra {
  pub buf_info: sqe_buf_info,
  pub personality: u16,
  pub splice_fd_in: i32,
  pub __pad: [u64; 2],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub union sqe_buf_info {
  pub buf_index: u16,
  pub buf_group: u16,
}

// io_uring_setup() flags
pub const IORING_SETUP_IOPOLL: u32 = 1 << 0;
pub const IORING_SETUP_SQPOLL: u32 = 1 << 1;
pub const IORING_SETUP_SQ_AFF: u32 = 1 << 2;
pub const IORING_SETUP_CQSIZE: u32 = 1 << 3;
pub const IORING_SETUP_CLAMP: u32 = 1 << 4;
pub const IORING_SETUP_ATTACH_WQ: u32 = 1 << 5;

// sqe->flags
pub const IOSQE_FIXED_FILE: u8 = 1 << 0;
pub const IOSQE_IO_DRAIN: u8 = 1 << 1;
pub const IOSQE_IO_LINK: u8 = 1 << 2;
pub const IOSQE_IO_HARDLINK: u8 = 1 << 3;
pub const IOSQE_ASYNC: u8 = 1 << 4;
pub const IOSQE_BUFFER_SELECT: u8 = 1 << 5;

// sqe->opcode
declare_sequential! {
    enum IoRingOp : u8;

    pub const IORING_OP_NOP;
    pub const IORING_OP_READV;
    pub const IORING_OP_WRITEV;
    pub const IORING_OP_FSYNC;
    pub const IORING_OP_READ_FIXED;
    pub const IORING_OP_WRITE_FIXED;
    pub const IORING_OP_POLL_ADD;
    pub const IORING_OP_POLL_REMOVE;
    pub const IORING_OP_SYNC_FILE_RANGE;
    pub const IORING_OP_SENDMSG;
    pub const IORING_OP_RECVMSG;
    pub const IORING_OP_TIMEOUT;
    pub const IORING_OP_TIMEOUT_REMOVE;
    pub const IORING_OP_ACCEPT;
    pub const IORING_OP_ASYNC_CANCEL;
    pub const IORING_OP_LINK_TIMEOUT;
    pub const IORING_OP_CONNECT;
    pub const IORING_OP_FALLOCATE;
    pub const IORING_OP_OPENAT;
    pub const IORING_OP_CLOSE;
    pub const IORING_OP_FILES_UPDATE;
    pub const IORING_OP_STATX;
    pub const IORING_OP_READ;
    pub const IORING_OP_WRITE;
    pub const IORING_OP_FADVISE;
    pub const IORING_OP_MADVISE;
    pub const IORING_OP_SEND;
    pub const IORING_OP_RECV;
    pub const IORING_OP_OPENAT2;
    pub const IORING_OP_EPOLL_CTL;
    pub const IORING_OP_SPLICE;
    pub const IORING_OP_PROVIDE_BUFFERS;
    pub const IORING_OP_REMOVE_BUFFERS;
    pub const IORING_OP_TEE;

    // Note: Will automatically be updated as new operations are added
    pub const IORING_OP_LAST;
}

#[test]
fn nop_is_0() {
  assert_eq!(IORING_OP_NOP, 0);
}

// sqe->fsync_flags
pub const IORING_FSYNC_DATASYNC: u32 = 1 << 0;

// sqe->timeout_flags
pub const IORING_TIMEOUT_ABS: u32 = 1 << 0;

// sqe->splice_flags
// extends the flags for splice(2)
pub const SPLICE_F_FD_IN_FIXED: u32 = 1 << 31;

/// IO copmletion data structure (Completion Queue Entry)
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct io_uring_cqe {
  pub user_data: u64,
  pub res: i32,
  pub flags: u32,
}

// cqe->flags
pub const IORING_CQE_F_BUFFER: u32 = 1 << 0;

pub const IORING_CQE_BUFFER_SHIFT: u32 = 16;

// Magic offsets for the application to mmap the data it needs
pub const IORING_OFF_SQ_RING: usize = 0;
pub const IORING_OFF_CQ_RING: usize = 0x8000000;
pub const IORING_OFF_SQES: usize = 0x10000000;

/// Filled with the offsets for mmap(2)
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct io_sqring_offsets {
  pub head: u32,
  pub tail: u32,
  pub ring_mask: u32,
  pub ring_entries: u32,
  pub flags: u32,
  pub dropped: u32,
  pub array: u32,
  pub resv1: u32,
  pub resv2: u64,
}

// sq_ring->flags
pub const IORING_SQ_NEED_WAKEUP: u32 = 1 << 0;

// cq_ring->flags
pub const IORING_CQ_EVENTFD_DISABLED: u32 = 1 << 0;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct io_cqring_offsets {
  pub head: u32,
  pub tail: u32,
  pub ring_mask: u32,
  pub ring_entries: u32,
  pub overflow: u32,
  pub cqes: u32,
  pub flags: u32,
  pub resv1: u32,
  pub resv2: u64,
}

// io_uring_enter(2) flags
pub const IORING_ENTER_GETEVENTS: u32 = 1 << 0;
pub const IORING_ENTER_SQ_WAKEUP: u32 = 1 << 1;

/// Passed in for io_uring_setup(2). Copied back with updated info on success.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct io_uring_params {
  pub sq_entries: u32,
  pub cq_entries: u32,
  pub flags: u32,
  pub sq_thread_cpu: u32,
  pub sq_thread_idle: u32,
  pub features: u32,
  pub wq_fd: u32,
  pub resv: [u32; 3],
  pub sq_off: io_sqring_offsets,
  pub cq_off: io_cqring_offsets,
}

// io_uring_params->features flags
pub const IORING_FEAT_SINGLE_MMAP: u32 = 1 << 0;
pub const IORING_FEAT_NODROP: u32 = 1 << 1;
pub const IORING_FEAT_SUBMIT_STABLE: u32 = 1 << 2;
pub const IORING_FEAT_RW_CUR_POS: u32 = 1 << 3;
pub const IORING_FEAT_CUR_PERSONALITY: u32 = 1 << 4;
pub const IORING_FEAT_FAST_POLL: u32 = 1 << 5;

// io_uring_register(2) opcodes and arguments
pub const IORING_REGISTER_BUFFERS: u32 = 0;
pub const IORING_UNREGISTER_BUFFERS: u32 = 1;
pub const IORING_REGISTER_FILES: u32 = 2;
pub const IORING_UNREGISTER_FILES: u32 = 3;
pub const IORING_REGISTER_EVENTFD: u32 = 4;
pub const IORING_UNREGISTER_EVENTFD: u32 = 5;
pub const IORING_REGISTER_FILES_UPDATE: u32 = 6;
pub const IORING_REGISTER_EVENTFD_ASYNC: u32 = 7;
pub const IORING_REGISTER_PROBE: u32 = 8;
pub const IORING_REGISTER_PERSONALITY: u32 = 9;
pub const IORING_UNREGISTER_PERSONALITY: u32 = 10;

#[repr(C, align(8))]
#[derive(Copy, Clone, Debug, Default)]
pub struct io_uring_files_update {
  pub offset: u32,
  pub resv: u32,
  pub fds: u64,
}

pub const IO_URING_OP_SUPPORTED: u16 = 1 << 0;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct io_uring_probe_op {
  pub op: u8,
  pub resv: u8,
  pub flags: u16,
  pub resv2: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct io_uring_probe {
  pub last_op: u8,
  pub ops_len: u8,
  pub resv: u16,
  pub resv2: [u32; 3],
  pub ops: __IncompleteArrayField<io_uring_probe_op>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct __kernel_timespec {
  pub tv_sec: i64,
  pub tv_nsec: c_longlong,
}

#[link(name = "uring-syscall")]
extern "C" {
  pub fn io_uring_setup(entries: u32, p: *mut io_uring_params) -> c_int;

  pub fn io_uring_enter(
    fd: c_uint,
    to_submit: u32,
    min_complete: u32,
    flags: u32,
    sig: *mut sigset_t,
  ) -> c_int;

  pub fn io_uring_register(fd: c_uint, opcode: u32, arg: *mut c_void, nr_args: u32) -> c_int;
}
