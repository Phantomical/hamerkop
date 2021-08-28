use std::mem::MaybeUninit;
use std::ops::{Index, IndexMut};

const SENTINEL: usize = usize::MAX;
const SLABSIZE: usize = 1024;

enum Entry<T> {
  Element(T),
  Link(usize),
}

/// Slotmap with stable addresses.
///
/// This means that you can stably keep a pointer to an element in this map
/// even as values are inserted and removed.
pub(crate) struct StableSlotmap<T> {
  slabs: Vec<Box<[Entry<T>; SLABSIZE]>>,
  head: usize,
  size: usize,
}

impl<T> StableSlotmap<T> {
  pub fn new() -> Self {
    Self {
      slabs: Vec::new(),
      head: SENTINEL,
      size: 0,
    }
  }

  #[inline]
  pub fn len(&self) -> usize {
    self.size
  }

  #[inline]
  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }

  pub fn insert(&mut self, value: T) -> usize {
    let index = self.next_slot();
    let entry = self.slot_at_mut(index).unwrap();
    let next = match entry {
      Entry::Link(next) => *next,
      _ => unreachable!(),
    };

    *entry = Entry::Element(value);
    self.head = next;
    self.size += 1;

    index
  }
  pub fn remove(&mut self, index: usize) -> Option<T> {
    let head = self.head;
    let entry = self.slot_at_mut(index)?;

    if let Entry::Link(_) = entry {
      return None;
    }

    let value = match std::mem::replace(entry, Entry::Link(head)) {
      Entry::Element(v) => v,
      _ => unreachable!(),
    };

    self.head = index;
    self.size -= 1;

    Some(value)
  }

  pub fn get(&self, index: usize) -> Option<&T> {
    match self.slot_at(index)? {
      Entry::Element(v) => Some(v),
      _ => None,
    }
  }
  pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
    match self.slot_at_mut(index)? {
      Entry::Element(v) => Some(v),
      _ => None,
    }
  }

  #[allow(dead_code)]
  pub fn iter(&self) -> impl Iterator<Item = &T> {
    self
      .slabs
      .iter()
      .flat_map(|slab| slab.iter())
      .filter_map(|entry| match entry {
        Entry::Element(e) => Some(e),
        _ => None,
      })
  }
  #[allow(dead_code)]
  pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
    self
      .slabs
      .iter_mut()
      .flat_map(|slab| slab.iter_mut())
      .filter_map(|entry| match entry {
        Entry::Element(e) => Some(e),
        _ => None,
      })
  }

  fn slot_at(&self, index: usize) -> Option<&Entry<T>> {
    self
      .slabs
      .get(index / SLABSIZE)
      .and_then(|slab| slab.get(index % SLABSIZE))
  }
  fn slot_at_mut(&mut self, index: usize) -> Option<&mut Entry<T>> {
    self
      .slabs
      .get_mut(index / SLABSIZE)
      .and_then(|slab| slab.get_mut(index % SLABSIZE))
  }

  fn next_slot(&mut self) -> usize {
    if self.head == SENTINEL {
      self.expand();
    }

    self.head
  }

  fn expand(&mut self) {
    let base = self.slabs.len() * SLABSIZE;
    self.slabs.push(create_slab(self.head, base));
    self.head = base;
  }
}

impl<T> Index<usize> for StableSlotmap<T> {
  type Output = T;

  fn index(&self, index: usize) -> &Self::Output {
    match self.get(index) {
      Some(value) => value,
      None => panic!("Invalid index {}", index),
    }
  }
}
impl<T> IndexMut<usize> for StableSlotmap<T> {
  fn index_mut(&mut self, index: usize) -> &mut Self::Output {
    match self.get_mut(index) {
      Some(value) => value,
      None => panic!("Invalid index {}", index),
    }
  }
}

fn create_slab<T>(head: usize, base: usize) -> Box<[Entry<T>; SLABSIZE]> {
  unsafe {
    // TODO: Use Box::new_uninit once stable.
    let mut slab: Box<[MaybeUninit<Entry<T>>; SLABSIZE]> =
      Box::new(MaybeUninit::uninit().assume_init());

    for i in 0..SLABSIZE - 1 {
      slab[i] = MaybeUninit::new(Entry::Link(base + i + 1));
    }
    slab[SLABSIZE - 1] = MaybeUninit::new(Entry::Link(head));

    std::mem::transmute(slab)
  }
}
