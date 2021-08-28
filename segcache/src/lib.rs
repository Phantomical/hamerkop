#![allow(dead_code)]

use std::{
  cell::UnsafeCell,
  marker::PhantomData,
  mem::MaybeUninit,
  sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};

use parking_lot::{Mutex, RwLock};

mod chunk;
mod segment;
mod utils;

pub use crate::chunk::ChunkHandle;
pub use crate::segment::{Segment, SegmentHeader};

pub enum SegCacheError {
  /// The combination of keysize + valuesize + overhead was larger than the
  /// cache is able to store.
  DataTooLarge,
  /// The key may not be 0 bytes in length.
  EmptyKey,
}

pub struct SegCache<'a> {
  segments: &'a mut [Segment],
  freelist: Mutex<Vec<usize>>,
}
