use std::{
  cell::UnsafeCell,
  mem::MaybeUninit,
  sync::atomic::{AtomicUsize, Ordering},
};

use parking_lot::RwLock;

use crate::{ChunkHandle, SegCacheError};

pub struct SegmentHeader {
  pub(crate) offset: AtomicUsize,
  pub(crate) refcnt: AtomicUsize,
  pub(crate) next: AtomicUsize,
  pub(crate) expiry: u64,

  pub(crate) lock: RwLock<()>,
}

impl SegmentHeader {
  pub const INVALID_NEXT: usize = usize::MAX;

  fn new() -> Self {
    Self {
      offset: AtomicUsize::new(0),
      refcnt: AtomicUsize::new(0),
      next: AtomicUsize::new(Self::INVALID_NEXT),
      expiry: 0,
      lock: RwLock::new(()),
    }
  }
}

/// Individual memory segment
pub struct Segment {
  pub(crate) header: SegmentHeader,
  data: UnsafeCell<[MaybeUninit<u8>; Self::AVAILABLE]>,
}

impl Segment {
  pub const TOTAL_SIZE: usize = 1024 * 1024;
  pub const AVAILABLE: usize = Self::TOTAL_SIZE - std::mem::size_of::<SegmentHeader>();
  pub const MAX_KEY_SIZE: usize = Self::AVAILABLE;
  pub const MAX_VAL_SIZE: usize = Self::AVAILABLE;

  fn init(mem: &mut MaybeUninit<Self>) -> &mut Self {
    // SAFETY: On the exit from this block the header is initialized. The rest of
    //         the data is MaybeUninit so it doesn't need to be initialized.
    unsafe {
      std::ptr::write(mem.as_mut_ptr() as *mut SegmentHeader, SegmentHeader::new());

      &mut *mem.as_mut_ptr()
    }
  }

  pub fn next(&self) -> Option<usize> {
    match self.header.next.load(Ordering::Acquire) {
      SegmentHeader::INVALID_NEXT => None,
      next => Some(next),
    }
  }

  unsafe fn subslice(&self, offset: usize, len: usize) -> &mut [MaybeUninit<u8>] {
    assert!(offset + len <= Self::AVAILABLE);

    std::slice::from_raw_parts_mut((self.data.get() as *mut MaybeUninit<u8>).add(offset), len)
  }

  pub fn try_insert<'s>(
    &'s self,
    key: &[u8],
    value: &[u8],
  ) -> Result<Option<ChunkHandle<'s>>, SegCacheError> {
    if key.len() > Self::MAX_KEY_SIZE || value.len() > Self::MAX_VAL_SIZE {
      return Err(SegCacheError::DataTooLarge);
    }

    if key.len() == 0 {
      return Err(SegCacheError::EmptyKey);
    }

    let total = ChunkHandle::total_size(key.len(), value.len());
    if total > Self::AVAILABLE {
      return Err(SegCacheError::DataTooLarge);
    }

    let offset = self.header.offset.fetch_add(total, Ordering::SeqCst);
    if offset + total > Self::AVAILABLE {
      self.header.offset.fetch_sub(total, Ordering::SeqCst);
      return Ok(None);
    }

    let subslice = unsafe { self.subslice(offset, total) };
    Ok(Some(ChunkHandle::create(subslice, self, key, value, 0)?))
  }

  // pub unsafe fn chunk_at(&self, offset: u32) ->
}
