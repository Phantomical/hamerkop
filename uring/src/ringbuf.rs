use std::marker::PhantomData;
use std::ops::{Deref, Index, IndexMut};
use std::ptr::NonNull;

#[derive(Copy, Clone)]
struct RingBufCommon<'a, T> {
  data: NonNull<T>,
  offset: usize,
  mask: usize,
  size: usize,
  _marker: PhantomData<&'a mut [T]>,
}

impl<'a, T> RingBufCommon<'a, T> {
  unsafe fn new(data: NonNull<T>, offset: usize, size: usize, mask: usize) -> Self {
    assert!(size <= mask + 1);

    Self {
      data,
      offset,
      size,
      mask,
      _marker: PhantomData,
    }
  }

  #[inline]
  fn len(&self) -> usize {
    self.size
  }

  #[inline]
  unsafe fn backing(&self) -> &'a [T] {
    std::slice::from_raw_parts(self.data.as_ptr(), self.mask + 1)
  }
  #[inline]
  unsafe fn backing_mut(&mut self) -> &'a mut [T] {
    std::slice::from_raw_parts_mut(self.data.as_ptr(), self.mask + 1)
  }
}

/// Immutable ringbuffer
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct RingBuf<'a, T>(RingBufCommon<'a, T>);

#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct RingBufMut<'a, T>(RingBufCommon<'a, T>);

impl<'a, T> RingBuf<'a, T> {
  #[allow(dead_code)]
  pub(crate) unsafe fn new(ptr: NonNull<T>, offset: usize, size: usize, mask: usize) -> Self {
    assert!(mask & (mask + 1) == 0);

    Self(RingBufCommon::new(ptr, offset, size, mask))
  }

  #[inline]
  pub fn len(&self) -> usize {
    self.0.len()
  }

  pub unsafe fn get_unchecked(&self, index: usize) -> &'a T {
    &*self
      .0
      .data
      .as_ptr()
      .add((self.0.offset + index) & self.0.mask)
  }

  pub fn get(&self, index: usize) -> Option<&'a T> {
    if index >= self.len() {
      return None;
    }

    Some(unsafe { self.get_unchecked(index) })
  }
}

impl<'a, T> RingBufMut<'a, T> {
  pub(crate) unsafe fn new(ptr: NonNull<T>, offset: usize, size: usize, mask: usize) -> Self {
    assert!(mask & (mask + 1) == 0);

    Self(RingBufCommon::new(ptr, offset, size, mask))
  }

  pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &'a mut T {
    &mut *self
      .0
      .data
      .as_ptr()
      .add((self.0.offset + index) & self.0.mask)
  }

  pub fn get_mut(&mut self, index: usize) -> Option<&'a mut T> {
    if index >= self.len() {
      return None;
    }

    Some(unsafe { self.get_unchecked_mut(index) })
  }
}

impl<'a, T> Deref for RingBufMut<'a, T> {
  type Target = RingBuf<'a, T>;

  fn deref(&self) -> &Self::Target {
    unsafe { &*(self as *const Self as *const RingBufCommon<T> as *const RingBuf<T>) }
  }
}

impl<'a, T> Index<usize> for RingBuf<'a, T> {
  type Output = T;

  fn index(&self, index: usize) -> &Self::Output {
    self.get(index).expect("Index out of bounds")
  }
}

impl<'a, T> Index<usize> for RingBufMut<'a, T> {
  type Output = T;

  fn index(&self, index: usize) -> &Self::Output {
    self.get(index).expect("Index out of bounds")
  }
}

impl<'a, T> IndexMut<usize> for RingBufMut<'a, T> {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    self.get_mut(index).expect("Index out of bounds")
  }
}

mod _detail {
  use super::{RingBuf, RingBufMut};
  use std::iter;

  impl<'a, T: 'a> IntoIterator for RingBuf<'a, T> {
    type Item = &'a T;
    type IntoIter =
      iter::Chain<<&'a [T] as IntoIterator>::IntoIter, <&'a [T] as IntoIterator>::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
      let slice = unsafe { self.0.backing() };

      let (slice1, slice2): (_, &[T]) =
        if (self.0.offset + self.0.size) & self.0.mask == self.0.offset + self.0.size {
          (&slice[self.0.offset..self.0.offset + self.0.size], &[])
        } else {
          let (slice1, slice2) = slice.split_at(self.0.offset);
          (
            slice2,
            &slice1[..(self.0.offset + self.0.size) & self.0.mask],
          )
        };

      slice1.into_iter().chain(slice2.into_iter())
    }
  }

  impl<'a, 'b: 'a, T: 'a> IntoIterator for &'b RingBuf<'a, T> {
    type Item = &'a T;
    type IntoIter =
      iter::Chain<<&'a [T] as IntoIterator>::IntoIter, <&'a [T] as IntoIterator>::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
      // Safe because RingBufCommon (and, by extension, RingBuf and RingBufMut)
      // is always copiable, this just can't currently be expressed in rust.
      unsafe { std::ptr::read(self) }.into_iter()
    }
  }

  impl<'a, 'b: 'a, T: 'a> IntoIterator for &'b RingBufMut<'a, T> {
    type Item = &'a T;
    type IntoIter =
      iter::Chain<<&'a [T] as IntoIterator>::IntoIter, <&'a [T] as IntoIterator>::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
      let buf: &RingBuf<'a, T> = self;
      buf.into_iter()
    }
  }

  impl<'a, T: 'a> IntoIterator for RingBufMut<'a, T> {
    type Item = &'a mut T;
    type IntoIter =
      iter::Chain<<&'a mut [T] as IntoIterator>::IntoIter, <&'a mut [T] as IntoIterator>::IntoIter>;

    fn into_iter(mut self) -> Self::IntoIter {
      let (offset, size, mask) = (self.0.offset, self.0.size, self.0.mask);
      let slice = unsafe { self.0.backing_mut() };

      let (slice1, slice2): (_, &mut [T]) = if (offset + size) & mask == offset + size {
        (&mut slice[offset..offset + size], &mut [])
      } else {
        let (slice1, slice2) = slice.split_at_mut(offset);
        (slice2, &mut slice1[..(offset + size) & mask])
      };

      slice1.into_iter().chain(slice2.into_iter())
    }
  }

  impl<'a, 'b: 'a, T: 'a> IntoIterator for &'b mut RingBufMut<'a, T> {
    type Item = &'a mut T;
    type IntoIter =
      iter::Chain<<&'a mut [T] as IntoIterator>::IntoIter, <&'a mut [T] as IntoIterator>::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
      // Safe because RingBufCommon (and, by extension, RingBuf and RingBufMut)
      // is always copiable, this just can't currently be expressed in rust.
      unsafe { std::ptr::read(self) }.into_iter()
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn ringbuf_iter() {
    let mut backing = [0, 1, 2, 3, 4, 5, 6, 7];

    let buf = unsafe { RingBuf::new(NonNull::new_unchecked(backing.as_mut_ptr()), 5, 6, 7) };
    let collected = (&buf).into_iter().cloned().collect::<Vec<_>>();

    assert_eq!(collected, [5, 6, 7, 0, 1, 2]);
  }
}
