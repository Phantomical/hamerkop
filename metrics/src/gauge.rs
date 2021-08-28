use crate::{Metric, Value};
use std::{
  any::Any,
  sync::atomic::{AtomicI64, Ordering},
};

#[derive(Default, Debug)]
pub struct Gauge(AtomicI64);

impl Gauge {
  pub const fn new() -> Self {
    Self(AtomicI64::new(0))
  }

  #[inline]
  pub fn increment(&self) {
    self.add(1);
  }

  #[inline]
  pub fn decrement(&self) {
    self.sub(1);
  }

  #[inline]
  pub fn add(&self, value: i64) {
    self.0.fetch_add(value, Ordering::Relaxed);
  }

  #[inline]
  pub fn sub(&self, value: i64) {
    self.0.fetch_sub(value, Ordering::Relaxed);
  }

  #[inline]
  pub fn value(&self) -> i64 {
    self.0.load(Ordering::Relaxed)
  }

  #[inline]
  pub fn set(&self, value: i64) -> i64 {
    self.0.swap(value, Ordering::Relaxed)
  }

  #[inline]
  pub fn reset(&self) -> i64 {
    self.set(0)
  }
}

impl Metric for Gauge {
  fn value(&self) -> Option<Value> {
    Some(Value::Signed(self.value()))
  }

  fn as_any(&self) -> Option<&dyn Any> {
    Some(self)
  }
}
