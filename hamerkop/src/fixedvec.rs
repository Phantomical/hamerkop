use std::io::{self, ErrorKind, Write};
use std::mem::MaybeUninit;
use std::ops::{Deref, DerefMut};
use std::{
  borrow::{Borrow, BorrowMut},
  cmp::{Eq, Ord, Ordering, PartialEq, PartialOrd},
  fmt,
  iter::{Extend, IntoIterator},
};
use std::{
  hash::{Hash, Hasher},
  ops::{Bound, RangeBounds},
};

trait IsCopy {
  fn is_copy() -> bool;
}

impl<T> IsCopy for T {
  #[inline(always)]
  default fn is_copy() -> bool {
    false
  }
}

impl<T: Copy> IsCopy for T {
  #[inline(always)]
  fn is_copy() -> bool {
    true
  }
}

#[inline(always)]
fn is_copy<T>() -> bool {
  <T as IsCopy>::is_copy()
}

fn make_boxed_slice<T>(capacity: usize) -> Box<[MaybeUninit<T>]> {
  let mut vec = Vec::with_capacity(capacity);
  // Safe since element type is MaybeUninit<T> and we've just guaranteed
  // that the vector has the correct capacity.
  unsafe { vec.set_len(capacity) };

  vec.into_boxed_slice()
}

pub struct FixedVec<T> {
  data: Box<[MaybeUninit<T>]>,
  size: usize,
}

impl<T> FixedVec<T> {
  /// Construct a new, empty `FixedVec<T>` with the specified capacity.
  ///
  /// The fixedvec will be able to hold at most `capacity` elements and
  /// will never reallocate.
  ///
  /// # Example
  /// ```
  /// # use hamerkop::FixedVec;
  /// let mut fv = FixedVec::<u8>::with_capacity(128);
  ///
  /// assert_eq!(fv.len(), 0);
  /// assert_eq!(fv.capacity(), 128);
  /// ```
  pub fn with_capacity(capacity: usize) -> Self {
    Self {
      data: make_boxed_slice(capacity),
      size: 0,
    }
  }

  /// Returns the maximum number of elements this fixedvec can store.
  #[inline]
  pub fn capacity(&self) -> usize {
    self.data.len()
  }

  /// Returns the number of elements that can be inserted into this `FixedVec`
  /// before it runs out of capacity.
  #[inline]
  pub fn available(&self) -> usize {
    self.capacity() - self.len()
  }

  /// Shortens the vector, keeping the first `len` elements and dropping the
  /// rest.
  ///
  /// If `len` is greater than the fixedvec's current capacity, this has no
  /// effect.
  pub fn truncate(&mut self, len: usize) {
    if len >= self.len() {
      return;
    }

    unsafe {
      let oldlen = self.len();
      self.set_len(len);

      if std::mem::needs_drop::<T>() {
        for i in len..oldlen {
          std::ptr::drop_in_place(self.as_mut_ptr().add(i));
        }
      }
    }
  }

  /// Clears the vector, removing all values.
  ///
  /// # Example
  /// ```
  /// # use hamerkop::FixedVec;
  /// let mut vec = FixedVec::with_capacity(16);
  /// vec.extend_from_slice(&[1, 2, 3, 4, 5]);
  /// vec.clear();
  ///
  /// assert!(vec.is_empty());
  /// ```
  #[inline]
  pub fn clear(&mut self) {
    unsafe {
      let len = self.len();
      self.set_len(0);

      if std::mem::needs_drop::<T>() {
        for i in 0..len {
          std::ptr::drop_in_place(self.as_mut_ptr().add(i));
        }
      }
    }
  }

  /// Resizes the `FixedVec` in-place so that `len` is equal to `new_len`.
  ///
  /// If `new_len` is greater than `len`, the `FixedVec` is extended by the
  /// difference, with each additional slot filled with the result of calling
  /// the closure `f`. The return values from `f` will end up in the `FixedVec`
  /// in the order they have been generated.
  ///
  /// If `new_len` is less than `len`, the `FixedVec` is simply truncated.
  ///
  /// # Panics
  /// This method will panic if `new_len` is greater than `capacity()`.
  ///
  /// # Examples
  /// ```
  /// # use hamerkop::FixedVec;
  /// let mut vec = FixedVec::with_capacity(16);
  /// vec.extend_from_slice(&[1, 2, 3]);
  /// vec.resize_with(5, Default::default);
  /// assert_eq!(vec, &[1, 2, 3, 0, 0][..]);
  ///
  /// let mut vec = FixedVec::with_capacity(16);
  /// let mut p = 1;
  /// vec.resize_with(4, || { p *= 2; p });
  /// assert_eq!(vec, &[2, 4, 8, 16][..]);
  /// ```
  pub fn resize_with<F>(&mut self, new_len: usize, mut f: F)
  where
    F: FnMut() -> T,
  {
    if new_len <= self.len() {
      self.truncate(new_len);
      return;
    }

    assert!(
      new_len <= self.capacity(),
      "attempted to resize a FixedVec to greater than it's capacity"
    );

    for i in self.len()..new_len {
      unsafe { std::ptr::write(self.as_mut_ptr().add(i), f()) };
      self.size += 1;
    }
  }

  /// Forces the length of the vector to `new_len`.
  ///
  /// This is a low-level operation that maintains none of the normal invariants
  /// ofthe type. Normally changing the length of a `FixedVec` is done using one
  /// of the safe operations instead, such as [`truncate`], [`resize`],
  /// [`extend`] or [`clear`].
  ///
  /// # Safety
  /// - `new_len` must be less than or equal to [`capacity()`].
  /// - The elements at `old_len..new_len` must be initialized.
  ///
  /// [`truncate`]: FixedVec::truncate
  /// [`resize`]: FixedVec::resize
  /// [`extend`]: FixedVec::extend
  /// [`clear`]: FixedVec::clear
  /// [`capacity()`]: FixedVec::capacity
  #[inline]
  pub unsafe fn set_len(&mut self, new_len: usize) {
    debug_assert!(new_len <= self.capacity());
    self.size = new_len;
  }

  #[inline]
  pub fn as_slice(&self) -> &[T] {
    unsafe { std::slice::from_raw_parts(self.as_ptr(), self.size) }
  }

  #[inline]
  pub fn as_mut_slice(&mut self) -> &mut [T] {
    unsafe { std::slice::from_raw_parts_mut(self.as_mut_ptr(), self.size) }
  }

  /// Returns a raw pointer to the vector's buffer.
  ///
  /// The caller must ensure that the vector outlives the pointer this function
  /// returns or else it will end up pointing to garbage. Modifying a `FixedVec`
  /// will never cause its buffer to be reallocated so it is safe to hold on to
  /// the pointer up until the point at which the `FixedVec` is dropped.
  ///
  /// The caller must also ensure that the memory the pointer (non-transitively)
  /// points to is never written to (except inside an `UnsafeCell`) using this
  /// pointer or any pointer derived from it. If you need to mutate the contents
  /// of the vector, use [`as_mut_ptr()`](crate::FixedVec::as_mut_ptr).
  #[inline]
  pub fn as_ptr(&self) -> *const T {
    self.data.deref().as_ptr() as _
  }

  /// Returns an unsafe mutable pointer to the vector's buffer.
  ///
  /// The caller must ensure that the vector outlives the pointer this function
  /// returns, or else it will end up pointing to garbage. Modifying a
  /// `FixedVec` will never cause its buffer to be reallocated so it is safe
  /// to hold on to the pointer up until the point at which the `FixedVec` is
  /// dropped.
  #[inline]
  pub fn as_mut_ptr(&mut self) -> *mut T {
    self.data.deref_mut().as_mut_ptr() as _
  }

  /// Appends an element to the back of a collection.
  ///
  /// If there is not enough capacity to fit another element then it returns
  /// an error with the provided element.
  pub fn push(&mut self, value: T) -> Result<(), T> {
    if self.available() == 0 {
      return Err(value);
    }

    unsafe { std::ptr::write(self.as_mut_ptr().add(self.len()), value) };
    self.size += 1;

    Ok(())
  }

  /// removes the last element from the `FixedVec` and returns it, or `None` if
  /// empty.
  pub fn pop(&mut self) -> Option<T> {
    if self.is_empty() {
      return None;
    }

    self.size -= 1;
    unsafe { Some(std::ptr::read(self.as_ptr().add(self.len()))) }
  }

  /// Removes an element from the vector and returns it, replacing it with the
  /// last element of the vector.
  ///
  /// # Panics
  /// Panics if `index` is out of bounds.
  ///
  /// # Example
  /// ```
  /// # use hamerkop::FixedVec;
  /// let mut v = FixedVec::with_capacity(16);
  /// v.extend_from_slice(&[1u8, 2, 3, 4]);
  ///
  /// assert_eq!(v.swap_remove(1), 2);
  /// assert_eq!(v, &[1, 4, 3][..]);
  ///
  /// assert_eq!(v.swap_remove(0), 1);
  /// assert_eq!(v, &[3, 4][..]);
  /// ```
  pub fn swap_remove(&mut self, index: usize) -> T {
    assert!(index < self.len());

    if index + 1 < self.len() {
      let (last, rest) = match self.split_last_mut() {
        Some(x) => x,
        None => unreachable!(),
      };

      std::mem::swap(last, &mut rest[index]);
    }

    match self.pop() {
      Some(x) => x,
      None => unreachable!(),
    }
  }

  /// Inserts an element at poition `index` within the `FixedVec`, shifting all
  /// elements after it to the right.
  ///
  /// # Panics
  /// Panics if `index > len` or if `available == 0`.
  pub fn insert(&mut self, index: usize, element: T) {
    assert!(index <= self.len());
    assert!(self.available() > 0);

    let elem = unsafe { self.as_mut_ptr().add(index) };
    let next = unsafe { elem.add(1) };

    let count = self.len() - index;

    unsafe {
      std::ptr::copy(elem, next, count);
      std::ptr::write(elem, element);

      self.set_len(self.len() + 1);
    }
  }

  /// Removes and returns the element at position `index` within the vector,
  /// shifting all elements afer it to the left.
  ///
  /// # Panics
  /// Panics if `index` is out of bounds.
  pub fn remove(&mut self, index: usize) -> T {
    assert!(index < self.len());

    unsafe {
      let elem = self.as_mut_ptr().add(index);
      let next = elem.add(1);
      let count = self.len() - index - 1;

      let value = std::ptr::read(elem);
      std::ptr::copy(next, elem, count);

      self.set_len(self.len() - 1);

      value
    }
  }

  /// Retains only the elements specified by the predicate.
  ///
  /// In other words, remove all elements `e` such that `f(&e)` returns `false`.
  /// This method operates in place, visiting each element exactly once in the
  /// original order, and preserves the order of the retained elements.
  ///
  /// # Examples
  /// ```
  /// # use hamerkop::FixedVec;
  /// let mut vec = FixedVec::with_capacity(16);
  /// vec.extend_from_slice(&[1, 2, 3, 4]);
  /// vec.retain(|&x| x % 2 == 0u8);
  ///
  /// assert_eq!(vec, &[2, 4][..]);
  /// ```
  pub fn retain<F>(&mut self, mut f: F)
  where
    F: FnMut(&T) -> bool,
  {
    let mut j = 0;
    for i in 0..self.len() {
      if f(&self[i]) {
        // Safe since i and j are in-bounds and swap works even if i == j
        unsafe { std::ptr::swap(self.as_mut_ptr().add(i), self.as_mut_ptr().add(j)) };
        j += 1;
      }
    }
    self.truncate(j);
  }

  /// Creates a draining iterator that removes the specified range in the
  /// `FixedVec` and yields the removed items.
  ///
  /// When the iterator is dropped, all elements in the range are removed from
  /// the vector, even if the iterator is not fully consumed. If the iterator
  /// is not dropped (with [`mem::forget`] for example), it is unspecified how
  /// many elements are removed.
  ///
  /// # Panics
  /// Panics if the starting point is greater than the end point or if the end
  /// point is greater than the length of the vector.
  ///
  /// # Examples
  /// ```
  /// # use hamerkop::FixedVec;
  /// let mut a = FixedVec::with_capacity(16);
  /// let mut b = FixedVec::with_capacity(16);
  /// a.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
  ///
  /// b.extend(a.drain(2..=4));
  ///
  /// assert_eq!(a, &[1, 2, 6][..]);
  /// assert_eq!(b, &[3, 4, 5][..]);
  /// ```
  ///
  /// [`mem::forget`]: std::mem::forget
  pub fn drain<R>(&mut self, range: R) -> Drain<'_, T>
  where
    R: RangeBounds<usize>,
  {
    let start = match range.start_bound() {
      Bound::Excluded(&x) => x + 1,
      Bound::Included(&x) => x,
      Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
      Bound::Excluded(&x) => x,
      Bound::Included(&x) => x + 1,
      Bound::Unbounded => self.len(),
    };

    assert!(start <= end && end <= self.len());

    let len = self.len();
    unsafe { self.set_len(start) };

    Drain {
      vec: self,
      start,
      end,
      current: start,
      len,
    }
  }

  /// Moves as many elements as possible from `other` into `self`.
  ///
  /// # Examples
  /// ```
  /// # use hamerkop::FixedVec;
  /// let mut a = FixedVec::with_capacity(8);
  /// let mut b = FixedVec::with_capacity(8);
  /// a.extend_from_slice(&[1, 2, 3, 4, 5]);
  /// b.extend_from_slice(&[1, 2, 3, 4, 5]);
  ///
  /// a.append(&mut b);
  ///
  /// assert_eq!(a, &[1, 2, 3, 4, 5, 1, 2, 3][..]);
  /// assert_eq!(b, &[4, 5][..]);
  /// ```
  pub fn append(&mut self, other: &mut FixedVec<T>) {
    let amount = self.available().min(other.len());

    unsafe {
      std::ptr::copy_nonoverlapping(other.as_ptr(), self.as_mut_ptr().add(self.len()), amount);
      self.set_len(self.len() + amount);
      other.excise_front(amount);
    }
  }
}

// Private utility methods
impl<T> FixedVec<T> {
  /// Remove the first count elements
  unsafe fn excise_front(&mut self, count: usize) {
    debug_assert!(count <= self.len());

    std::ptr::copy(self.as_ptr().add(count), self.as_mut_ptr(), count);
    self.set_len(self.len() - count);
  }
}

impl<T: Clone> FixedVec<T> {
  /// Resizes the `FixedVec` in-place so that `len` is equal to `new_len`.
  ///
  /// If `new_len` is greater than `len`, the `FixedVec` is extended by the
  /// difference, with each additional slot filled with `value`. If `new_len`
  /// is less than `len` the `Fixedvec` is truncated.
  ///
  /// # Panics
  /// Panics if `new_len` is greater than `capacity`.
  pub fn resize(&mut self, new_len: usize, value: T) {
    // TODO: Don't clone for the last value?
    self.resize_with(new_len, || value.clone());
  }

  /// Clones and appends all elements in a slice to the `FixedVec`.
  ///
  /// Iterates over the slice `other`, clones each element, and then appends
  /// it to this `FixedVec`. The `other` slice is traversed in-order.
  ///
  /// # Panics
  /// This function panics if there is not enough capacity remaining to hold
  /// all the elements of the slice.
  pub fn extend_from_slice(&mut self, other: &[T]) -> usize {
    let other = &other[..self.available().min(other.len())];

    if is_copy::<T>() {
      unsafe {
        std::ptr::copy_nonoverlapping(
          other.as_ptr(),
          self.as_mut_ptr().add(self.len()),
          other.len(),
        );
        self.set_len(self.len() + other.len());
      }
    } else {
      for item in other {
        unsafe { std::ptr::write(self.as_mut_ptr().add(self.len()), item.clone()) };
        self.size += 1;
      }
    }

    other.len()
  }
}

impl<T: Clone> Clone for FixedVec<T> {
  fn clone(&self) -> Self {
    let mut new = Self::with_capacity(self.capacity());
    new.extend_from_slice(self);
    new
  }

  fn clone_from(&mut self, source: &Self) {
    // TODO: Can probably manually reallocate here in cases where
    //       self.capacity() > source.capacity()
    if self.capacity() == source.capacity() {
      self.clear();
      self.extend_from_slice(source);
    } else {
      *self = source.clone();
    }
  }
}

impl<T> Drop for FixedVec<T> {
  fn drop(&mut self) {
    self.clear();
  }
}

impl<T> Deref for FixedVec<T> {
  type Target = [T];

  #[inline]
  fn deref(&self) -> &Self::Target {
    self.as_slice()
  }
}

impl<T> DerefMut for FixedVec<T> {
  #[inline]
  fn deref_mut(&mut self) -> &mut Self::Target {
    self.as_mut_slice()
  }
}

impl<T: fmt::Debug> fmt::Debug for FixedVec<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.deref().fmt(f)
  }
}

impl<T> AsRef<[T]> for FixedVec<T> {
  fn as_ref(&self) -> &[T] {
    self.as_slice()
  }
}
impl<T> AsRef<FixedVec<T>> for FixedVec<T> {
  #[inline]
  fn as_ref(&self) -> &Self {
    self
  }
}

impl<T> AsMut<[T]> for FixedVec<T> {
  fn as_mut(&mut self) -> &mut [T] {
    self.as_mut_slice()
  }
}
impl<T> AsMut<FixedVec<T>> for FixedVec<T> {
  #[inline]
  fn as_mut(&mut self) -> &mut Self {
    self
  }
}

impl<T> Borrow<[T]> for FixedVec<T> {
  #[inline]
  fn borrow(&self) -> &[T] {
    self.as_slice()
  }
}
impl<T> BorrowMut<[T]> for FixedVec<T> {
  fn borrow_mut(&mut self) -> &mut [T] {
    self.as_mut_slice()
  }
}

impl<B, T: PartialEq<B>> PartialEq<FixedVec<B>> for FixedVec<T> {
  #[inline]
  fn eq(&self, other: &FixedVec<B>) -> bool {
    self.eq(other.deref())
  }

  #[inline]
  fn ne(&self, other: &FixedVec<B>) -> bool {
    self.ne(other.deref())
  }
}

impl<B, T: PartialEq<B>> PartialEq<[B]> for FixedVec<T> {
  #[inline]
  fn eq(&self, other: &[B]) -> bool {
    self.deref().eq(other)
  }

  #[inline]
  fn ne(&self, other: &[B]) -> bool {
    self.deref().ne(other)
  }
}
impl<B, T: PartialEq<B>> PartialEq<FixedVec<B>> for [T] {
  #[inline]
  fn eq(&self, other: &FixedVec<B>) -> bool {
    self.eq(other.deref())
  }

  #[inline]
  fn ne(&self, other: &FixedVec<B>) -> bool {
    self.ne(other.deref())
  }
}

impl<B, T: PartialEq<B>> PartialEq<&'_ [B]> for FixedVec<T> {
  #[inline]
  fn eq(&self, other: &&[B]) -> bool {
    self.deref().eq(*other)
  }

  #[inline]
  fn ne(&self, other: &&[B]) -> bool {
    self.deref().ne(*other)
  }
}
impl<B, T: PartialEq<B>> PartialEq<&'_ mut [B]> for FixedVec<T> {
  #[inline]
  fn eq(&self, other: &&mut [B]) -> bool {
    self.deref().eq(*other)
  }

  #[inline]
  fn ne(&self, other: &&mut [B]) -> bool {
    self.deref().ne(*other)
  }
}

impl<B, T: PartialEq<B>> PartialEq<FixedVec<B>> for &'_ [T] {
  #[inline]
  fn eq(&self, other: &FixedVec<B>) -> bool {
    (*self).eq(other.deref())
  }

  #[inline]
  fn ne(&self, other: &FixedVec<B>) -> bool {
    (*self).ne(other.deref())
  }
}
impl<B, T: PartialEq<B>> PartialEq<FixedVec<B>> for &'_ mut [T] {
  #[inline]
  fn eq(&self, other: &FixedVec<B>) -> bool {
    (&**self).eq(other.deref())
  }

  #[inline]
  fn ne(&self, other: &FixedVec<B>) -> bool {
    (&**self).ne(other.deref())
  }
}

impl<'a, T: Clone + 'a> Extend<&'a T> for FixedVec<T> {
  /// Extend the fixedvec with an iterator.
  ///
  /// Does not extract more items than there is space for.
  fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
    let mut iter = iter.into_iter();

    while self.available() > 0 {
      let item = match iter.next() {
        Some(item) => item,
        None => break,
      };

      if let Err(_) = self.push(item.clone()) {
        unsafe { std::hint::unreachable_unchecked() };
      }
    }
  }
}
impl<T> Extend<T> for FixedVec<T> {
  /// Extend the fixedvec with an iterator.
  ///
  /// Does not extract more items than there is space for.
  fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
    let mut iter = iter.into_iter();

    while self.available() > 0 {
      let item = match iter.next() {
        Some(item) => item,
        None => break,
      };

      if let Err(_) = self.push(item) {
        unreachable!()
      }
    }
  }
}

impl<T: Eq> Eq for FixedVec<T> {}

impl<T: PartialOrd> PartialOrd for FixedVec<T> {
  #[inline]
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    self.deref().partial_cmp(other.deref())
  }
}

impl<T: Ord> Ord for FixedVec<T> {
  #[inline]
  fn cmp(&self, other: &Self) -> Ordering {
    self.deref().cmp(other.deref())
  }
}

impl<T: Hash> Hash for FixedVec<T> {
  #[inline]
  fn hash<H: Hasher>(&self, state: &mut H) {
    self.deref().hash(state)
  }
}

impl Write for FixedVec<u8> {
  #[inline]
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    let remaining = self.capacity() - self.len();

    if remaining == 0 {
      return Ok(0);
    }

    let slice = &buf[..buf.len().min(remaining)];
    self.extend_from_slice(slice);
    Ok(slice.len())
  }

  #[inline]
  fn write_vectored(&mut self, bufs: &[io::IoSlice<'_>]) -> io::Result<usize> {
    let mut total = 0;

    for buf in bufs {
      let n = self.write(buf)?;
      total += n;

      if n != buf.len() {
        break;
      }
    }

    Ok(total)
  }
  /*
  // TODO: Enable only on nightly?
  #[inline]
  fn is_write_vectored(&self) -> bool {
    true
  }
  */
  #[inline]
  fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
    match self.write(buf) {
      Ok(n) if n == buf.len() => Ok(()),
      Ok(_) => Err(io::Error::new(
        ErrorKind::WriteZero,
        "failed to write whole buffer",
      )),
      Err(e) => Err(e),
    }
  }

  #[inline]
  fn flush(&mut self) -> io::Result<()> {
    Ok(())
  }
}

pub struct Drain<'v, T> {
  vec: &'v mut FixedVec<T>,
  start: usize,
  end: usize,
  current: usize,
  len: usize,
}

impl<T> Iterator for Drain<'_, T> {
  type Item = T;

  fn next(&mut self) -> Option<Self::Item> {
    if self.current == self.end {
      return None;
    }

    let value = unsafe { std::ptr::read(self.vec.as_mut_ptr().add(self.current)) };
    self.current += 1;

    Some(value)
  }
}

impl<T> Drop for Drain<'_, T> {
  fn drop(&mut self) {
    unsafe {
      for i in self.start..self.end {
        std::ptr::drop_in_place(self.vec.as_mut_ptr().add(i));
      }

      std::ptr::copy(
        self.vec.as_ptr().add(self.end),
        self.vec.as_mut_ptr().add(self.start),
        self.len - self.end,
      );

      self.vec.set_len(self.len - (self.end - self.start));
    }
  }
}
