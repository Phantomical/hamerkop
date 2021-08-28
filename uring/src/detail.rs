#![allow(non_camel_case_types)]

use crate::RawFd;

use std::io::Error;
use std::sync::atomic::{AtomicU32, Ordering};

use libc::c_void;

use crate::flags::Features;
use crate::raw::{
  io_uring_cqe, io_uring_params, io_uring_sqe, IORING_OFF_CQ_RING, IORING_OFF_SQES,
  IORING_OFF_SQ_RING,
};

#[inline]
pub(crate) unsafe fn atomic_load(ptr: *const u32, ord: Ordering) -> u32 {
  let ptr = ptr as *const AtomicU32;
  (*ptr).load(ord)
}

#[inline]
pub(crate) unsafe fn atomic_store(ptr: *const u32, value: u32, ord: Ordering) {
  let ptr = ptr as *const AtomicU32;
  (*ptr).store(value, ord);
}

#[inline]
unsafe fn raw_offset<T, U>(ptr: *mut T, off: u32) -> *mut U {
  (ptr as *mut u8).add(off as usize) as *mut U
}

#[rustfmt::skip]
#[allow(dead_code)]
pub(crate) struct io_uring_sq {
  pub khead:          *mut u32,
  pub ktail:          *mut u32,
  pub kring_mask:     *mut u32,
  pub kring_entries:  *mut u32,
  pub kflags:         *mut u32,
  pub kdropped:       *mut u32,
  pub array:          *mut u32,
  pub sqes:           *mut io_uring_sqe,

  pub sqe_head:       u32,
  pub sqe_tail:       u32,

  pub ring_sz:        usize,
  pub ring_ptr:       *mut c_void,
}

#[rustfmt::skip]
#[allow(dead_code)]
pub(crate) struct io_uring_cq {
  pub khead:          *mut u32,
  pub ktail:          *mut u32,
  pub kring_mask:     *mut u32,
  pub kring_entries:  *mut u32,
  pub kflags:         *mut u32,
  pub koverflow:      *mut u32,
  pub cqes:           *mut io_uring_cqe,

  pub ring_sz:        usize,
  pub ring_ptr:       *mut c_void,
}

const MAP_FAILED: *mut c_void = !0usize as *mut c_void;

pub(crate) unsafe fn mmap_queues(
  fd: RawFd,
  p: &io_uring_params,
) -> Result<(io_uring_sq, io_uring_cq), Error> {
  use libc::{MAP_POPULATE, MAP_SHARED, PROT_READ, PROT_WRITE};
  use std::mem::size_of;
  use std::ptr::null_mut;

  let features = Features::from_bits_truncate(p.features);

  let mut sq_ring_sz = p.sq_off.array as usize + p.sq_entries as usize * size_of::<u32>();
  let mut cq_ring_sz = p.cq_off.cqes as usize + p.cq_entries as usize * size_of::<io_uring_cqe>();

  if features.contains(Features::SINGLE_MMAP) {
    sq_ring_sz = sq_ring_sz.max(cq_ring_sz);
    cq_ring_sz = sq_ring_sz.max(cq_ring_sz);
  }

  let sq_ring_ptr = libc::mmap(
    null_mut(),
    sq_ring_sz,
    PROT_READ | PROT_WRITE,
    MAP_SHARED | MAP_POPULATE,
    fd,
    IORING_OFF_SQ_RING as libc::off_t,
  );

  if sq_ring_ptr == MAP_FAILED {
    return Err(Error::last_os_error());
  }

  let cq_ring_ptr;
  if features.contains(Features::SINGLE_MMAP) {
    cq_ring_ptr = sq_ring_ptr;
  } else {
    cq_ring_ptr = libc::mmap(
      null_mut(),
      cq_ring_sz,
      PROT_READ | PROT_WRITE,
      MAP_SHARED | MAP_POPULATE,
      fd,
      IORING_OFF_CQ_RING as libc::off_t,
    );

    if cq_ring_ptr == MAP_FAILED {
      let e = Error::last_os_error();
      libc::munmap(sq_ring_ptr, sq_ring_sz);
      return Err(e);
    }
  }

  let sqes_size = p.sq_entries as usize * size_of::<io_uring_sqe>();
  let sqes = libc::mmap(
    null_mut(),
    sqes_size,
    PROT_READ | PROT_WRITE,
    MAP_SHARED | MAP_POPULATE,
    fd,
    IORING_OFF_SQES as libc::off_t,
  );

  if sqes == MAP_FAILED {
    let e = Error::last_os_error();
    libc::munmap(sq_ring_ptr, sq_ring_sz);
    libc::munmap(cq_ring_ptr, cq_ring_sz);
    return Err(e);
  }

  #[rustfmt::skip]
  let cq = io_uring_cq {
    khead:          raw_offset(cq_ring_ptr, p.cq_off.head),
    ktail:          raw_offset(cq_ring_ptr, p.cq_off.tail),
    kring_mask:     raw_offset(cq_ring_ptr, p.cq_off.ring_mask),
    kring_entries:  raw_offset(cq_ring_ptr, p.cq_off.ring_entries),
    koverflow:      raw_offset(cq_ring_ptr, p.cq_off.overflow),
    cqes:           raw_offset(cq_ring_ptr, p.cq_off.cqes),
    kflags:         match p.cq_off.flags {
      0 => std::ptr::null_mut(),
      x => raw_offset(cq_ring_ptr, x)
    },

    ring_ptr: cq_ring_ptr,
    ring_sz: cq_ring_sz,
  };

  #[rustfmt::skip]
  let sq = io_uring_sq {
    khead:          raw_offset(sq_ring_ptr, p.sq_off.head),
    ktail:          raw_offset(sq_ring_ptr, p.sq_off.tail),
    kring_mask:     raw_offset(sq_ring_ptr, p.sq_off.ring_mask),
    kring_entries:  raw_offset(sq_ring_ptr, p.sq_off.ring_entries),
    kflags:         raw_offset(sq_ring_ptr, p.sq_off.flags),
    kdropped:       raw_offset(sq_ring_ptr, p.sq_off.dropped),
    array:          raw_offset(sq_ring_ptr, p.sq_off.array),

    sqes: sqes as _,
    sqe_head: 0,
    sqe_tail: 0,

    ring_sz: sq_ring_sz,
    ring_ptr: sq_ring_ptr,
  };

  Ok((sq, cq))
}

pub(crate) unsafe fn unmap_queues(sq: &io_uring_sq, cq: &io_uring_cq) {
  libc::munmap(sq.ring_ptr, sq.ring_sz);

  if !cq.ring_ptr.is_null() && cq.ring_ptr != sq.ring_ptr {
    libc::munmap(cq.ring_ptr, cq.ring_sz);
  }
}

pub(crate) trait SaturatingCast: Sized {
  fn saturating_cast<T>(self) -> T
  where
    Self: SaturatingCastImpl<T>,
  {
    <Self as SaturatingCastImpl<T>>::saturating_cast(self)
  }
}

pub(crate) trait SaturatingCastImpl<T> {
  fn saturating_cast(self) -> T;
}

macro_rules! cast_clamp {
  ($src:ty => $( $tgt:ty ),*) => {
    impl SaturatingCast for $src {}

    $(
      impl SaturatingCastImpl<$tgt> for $src {
        fn saturating_cast(mut self) -> $tgt {
          if (<$tgt>::MAX as u128) < Self::MAX as u128 {
            self = self.min(<$tgt>::MAX as Self);
          }

          if (<$tgt>::MIN as i128) > Self::MIN as i128 {
            self = self.max(<$tgt>::MIN as Self);
          }

          self as $tgt
        }
      }
    )*
  }
}

macro_rules! all_casts {
  ($( $types:ty ),*) => {
    all_casts!({ $( $types; )* } => $( $types ),*);
  };
  ({$( $types:ty; )*} => $first:ty $( , $rest:ty )*) => {
    cast_clamp!($first => $( $types ),*);
    all_casts!({ $( $types; )* } => $( $rest ),*);
  };
  ({$( $types:ty; )*} => ) => {}
}

all_casts!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);
