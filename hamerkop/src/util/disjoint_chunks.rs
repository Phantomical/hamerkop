use std::mem::MaybeUninit;

use arrayvec::ArrayVec;

pub struct DisjointChunksExact<'a, T: Copy, const N: usize> {
  slices: ArrayVec<[&'a [T]; 2]>,
}

impl<'a, T: Copy, const N: usize> DisjointChunksExact<'a, T, N> {
  pub fn new(a: &'a [T], b: &'a [T]) -> Self {
    let mut slices = ArrayVec::new();
    slices.push(b);
    slices.push(a);

    Self { slices }
  }
}

impl<'a, T: Copy, const N: usize> Iterator for DisjointChunksExact<'a, T, N> {
  type Item = [T; N];

  fn next(&mut self) -> Option<Self::Item> {
    let mut remaining = N;
    let mut tmp: MaybeUninit<[T; N]> = MaybeUninit::uninit();

    while remaining > 0 {
      let first = self.slices.last_mut()?;
      let count = first.len().min(remaining);

      unsafe {
        std::ptr::copy_nonoverlapping(
          first.as_ptr(),
          (tmp.as_mut_ptr() as *mut T).add(N - remaining),
          count,
        )
      };

      remaining -= count;
      *first = &first[count..];

      if first.is_empty() {
        self.slices.pop();
      }
    }

    Some(unsafe { tmp.assume_init() })
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn chunks_exact_basic() {
    let a = [1u8, 2, 3];
    let b = [4, 5, 6, 7];

    let mut chunks = DisjointChunksExact::<_, 2>::new(&a, &b);

    assert_eq!(chunks.next(), Some([1, 2]));
    assert_eq!(chunks.next(), Some([3, 4]));
    assert_eq!(chunks.next(), Some([5, 6]));
    assert_eq!(chunks.next(), None);
  }

  #[test]
  fn chunks_exact_empty_first() {
    let a = [];
    let b = [1u8, 2, 3];

    let mut chunks = DisjointChunksExact::<_, 2>::new(&a, &b);

    assert_eq!(chunks.next(), Some([1, 2]));
    assert_eq!(chunks.next(), None);
  }
}
