use crate::{Metric, Value};
use std::{
  any::Any,
  sync::atomic::{AtomicU64, Ordering},
};

#[derive(Default, Debug)]
pub struct Counter(AtomicU64);

impl Counter {
  pub const fn new() -> Self {
    Self::with_value(0)
  }

  pub const fn with_value(value: u64) -> Self {
    Self(AtomicU64::new(value))
  }

  #[inline]
  pub fn increment(&self) {
    self.add(1);
  }

  #[inline]
  pub fn add(&self, value: u64) {
    self.0.fetch_add(value, Ordering::Relaxed);
  }

  #[inline]
  pub fn value(&self) -> u64 {
    self.0.load(Ordering::Relaxed)
  }

  #[inline]
  pub fn set(&self, value: u64) -> u64 {
    self.0.swap(value, Ordering::Relaxed)
  }

  #[inline]
  pub fn reset(&self) -> u64 {
    self.set(0)
  }
}

impl Metric for Counter {
  fn value(&self) -> Option<Value> {
    Some(Value::Unsigned(self.value()))
  }

  fn as_any(&self) -> Option<&dyn Any> {
    Some(self)
  }
}
