//!

use fxhash::FxBuildHasher;
use std::hash::{BuildHasher, Hash, Hasher};

const SEED1: u64 = 4200296977;
const SEED2: u64 = 1109209391370908281;

fn hash1<T: Hash, H: BuildHasher>(value: &T, builder: &H) -> usize {
  let mut hasher = builder.build_hasher();
  SEED1.hash(&mut hasher);
  value.hash(&mut hasher);
  hasher.finish() as usize
}

fn hash2<T: Hash, H: BuildHasher>(value: &T, builder: &H) -> usize {
  let mut hasher = builder.build_hasher();
  SEED2.hash(&mut hasher);
  value.hash(&mut hasher);
  hasher.finish() as usize
}

/// If the pointer is not null, compare *ptr with value
///
/// # Safety
/// `ptr` must be either null or dereferencable to a valid instance of T.
unsafe fn ptr_cmp<T: PartialEq>(ptr: *const T, value: &T) -> bool {
  !ptr.is_null() && *ptr == *value
}

/// Probabilistically remove duplicates within a vector.
///
/// This function provides two guarantees:
/// - All consecutive equal values will be removed except for the first one
///   (i.e. it subsumes `Vec::dedup`).
/// - At the end `data` will contain at least one equal value for every element
///   within the array.
///
/// Basically, this function will remove duplicates on a best-effort
/// basis but it will never introduce new values or entirely eliminate
/// a value from the input vector.
///
/// # Example
/// ```
/// # use hamerkop::util::dedup_partial;
/// let mut v = vec![1, 1, 1, 2, 2, 1];
/// dedup_partial(&mut v);
/// assert_eq!(v, [1, 2]);
/// ```
pub fn dedup_partial<T>(data: &mut Vec<T>)
where
  T: PartialEq + Hash,
{
  // This method basically works by using a hash table that evicts
  // entries when a newer one is inserted. To improve the load factor
  // we use cuckoo hashing here. Finally, to avoid performing any
  // allocations the cache hash table is stack-based.

  let mut table = [std::ptr::null::<T>(); 128];
  let builder = FxBuildHasher::default();

  let mut i = 0;
  while i < data.len() {
    let value = &data[i];
    let hash1 = hash1(value, &builder) % table.len();
    let hash2 = hash2(value, &builder) % table.len();

    // SAFETY: All pointers are either null or point to earlier within
    //         the vector. So it's safe to dereference them.
    if unsafe { ptr_cmp(table[hash1], value) || ptr_cmp(table[hash2], value) } {
      data.swap_remove(i);
      continue;
    }

    if table[hash1].is_null() {
      table[hash1] = value;
    } else if table[hash2].is_null() {
      table[hash2] = value;
    } else if i % 2 == 0 {
      table[hash1] = value;
    } else {
      table[hash2] = value;
    }

    i += 1;
  }
}
