use std::sync::atomic::AtomicU32;
use std::{
  mem::{align_of, size_of, MaybeUninit},
  sync::atomic::Ordering,
};

use crate::{SegCacheError, Segment, utils::round_up};

const fn log2(x: usize) -> u32 {
  (usize::BITS - 1) as u32 - x.leading_zeros()
}

const fn min(x: u32, y: u32) -> u32 {
  match x > y {
    true => y,
    false => x,
  }
}

#[derive(Copy, Clone)]
struct ChunkHeader {
  data: u32,
}

impl ChunkHeader {
  const SCALE_BITS: u32 = log2(Self::SIZE_BITS as usize);
  const SIZE_BITS: u32 = log2(Segment::MAX_KEY_SIZE - 1);
  const TOTAL_SIZE_BITS: u32 = min(
    log2(Segment::MAX_KEY_SIZE - 1) + log2(Segment::MAX_VAL_SIZE),
    log2(Segment::AVAILABLE) + 1,
  );
  const FLAG_BITS: u32 = u32::BITS - Self::SCALE_BITS - Self::TOTAL_SIZE_BITS;

  const SCALE_MASK: u32 = (1 << Self::SCALE_BITS) - 1;
  const FLAG_MASK: u32 = (1 << Self::FLAG_BITS) - 1;

  pub fn new(klen: usize, vlen: usize, flags: u16) -> Result<Self, SegCacheError> {
    let total = ChunkHandle::total_size(klen, vlen);
    if total > Segment::AVAILABLE || klen > Segment::MAX_KEY_SIZE || vlen > Segment::MAX_VAL_SIZE {
      return Err(SegCacheError::DataTooLarge);
    }

    if klen == 0 {
      return Err(SegCacheError::EmptyKey);
    }

    assert!(flags < (1 << Self::FLAG_BITS), "");

    let scale = log2(klen - 1);

    let mut data = 0;
    data |= (vlen as u32) << scale;
    data |= (klen - 1) as u32;
    data <<= Self::SCALE_BITS;
    data |= scale;
    data <<= Self::FLAG_BITS;
    data |= (flags as u32) & Self::FLAG_MASK;

    Ok(Self { data })
  }

  pub fn from_raw(data: u32) -> Self {
    Self { data }
  }

  pub fn into_raw(&self) -> u32 {
    self.data
  }

  fn scale(&self) -> u32 {
    (self.data >> Self::FLAG_BITS) & Self::SCALE_MASK
  }

  pub fn key_len(&self) -> usize {
    let mask = (1 << self.scale()) - 1;
    ((self.data >> (Self::FLAG_BITS + Self::SCALE_BITS)) & mask) as usize + 1
  }

  pub fn val_len(&self) -> usize {
    (self.data >> (Self::FLAG_BITS + Self::SCALE_BITS + self.scale())) as usize
  }

  pub fn total_len(&self) -> usize {
    ChunkHandle::total_size(self.key_len(), self.val_len())
  }

  pub fn flags(&self) -> u16 {
    (self.data & Self::FLAG_MASK) as u16
  }
}

pub struct ChunkHandle<'seg> {
  data: *const u8,
  segment: &'seg Segment,
}

impl<'seg> ChunkHandle<'seg> {
  pub const DEAD_FLAG: u16 = 1;

  pub(crate) unsafe fn new(data: &'seg [u8], segment: &'seg Segment) -> Self {
    segment.header.refcnt.fetch_add(1, Ordering::Relaxed);

    Self {
      data: data.as_ptr(),
      segment,
    }
  }

  pub(crate) fn create(
    data: &mut [MaybeUninit<u8>],
    segment: &'seg Segment,
    key: &[u8],
    val: &[u8],
    flags: u16,
  ) -> Result<Self, SegCacheError> {
    use std::ptr;

    let header = ChunkHeader::new(key.len(), val.len(), flags)?;
    assert_eq!(data.len(), header.total_len());

    let ptr = data.as_mut_ptr();

    unsafe {
      let pheader = ptr as *mut AtomicU32;
      ptr::write(pheader, AtomicU32::new(0));

      let pkey = ptr.wrapping_add(size_of::<AtomicU32>()) as *mut u8;
      let pval = pkey.wrapping_add(key.len());

      ptr::copy_nonoverlapping(key.as_ptr(), pkey, key.len());
      ptr::copy_nonoverlapping(val.as_ptr(), pval, val.len());

      (*pheader).store(header.into_raw(), Ordering::Release);
    }

    Ok(Self {
      data: data.as_ptr() as _,
      segment,
    })
  }

  fn header(&self) -> ChunkHeader {
    unsafe { ChunkHeader::from_raw((*(self.data as *const AtomicU32)).load(Ordering::Acquire)) }
  }

  pub fn total_size(klen: usize, vlen: usize) -> usize {
    round_up(
      klen + vlen + size_of::<ChunkHeader>(),
      align_of::<ChunkHeader>(),
    )
  }

  pub fn key(&self) -> &[u8] {
    let header = self.header();

    unsafe {
      std::slice::from_raw_parts(
        self.data.wrapping_add(size_of::<AtomicU32>()),
        header.key_len(),
      )
    }
  }

  pub fn value(&self) -> &[u8] {
    let header = self.header();

    unsafe {
      std::slice::from_raw_parts(
        self
          .data
          .wrapping_add(size_of::<AtomicU32>() + header.key_len()),
        header.val_len(),
      )
    }
  }

  pub fn flags(&self) -> u16 {
    self.header().flags()
  }

  pub fn is_dead(&self) -> bool {
    (self.header().flags() & Self::DEAD_FLAG) != 0
  }

  pub fn set_dead(&self) {
    unsafe {
      (*(self.data as *const AtomicU32)).fetch_or(Self::DEAD_FLAG as u32, Ordering::Release);
    }
  }
}

impl Clone for ChunkHandle<'_> {
  fn clone(&self) -> Self {
    self.segment.header.refcnt.fetch_add(1, Ordering::Relaxed);

    Self {
      data: self.data,
      segment: self.segment,
    }
  }
}

impl Drop for ChunkHandle<'_> {
  fn drop(&mut self) {
    self.segment.header.refcnt.fetch_sub(1, Ordering::Relaxed);
  }
}
