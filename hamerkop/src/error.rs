
use std::io;
use std::fmt;
use crate::FixedVec;

pub struct ProvideBuffersError {
  error: io::Error,
  buffer: FixedVec<u8>,
}

impl ProvideBuffersError {
  pub fn new(error: io::Error, buffer: FixedVec<u8>) -> Self {
    Self { error, buffer }
  }

  pub fn take_buffer(&mut self) -> FixedVec<u8> {
    std::mem::replace(&mut self.buffer, FixedVec::with_capacity(0))
  }

  pub fn into_buffer(self) -> FixedVec<u8> {
    self.buffer
  }
}

impl From<ProvideBuffersError> for io::Error {
  fn from(p: ProvideBuffersError) -> Self {
    p.error
  }
}

impl fmt::Display for ProvideBuffersError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.error.fmt(f)
  }
}

impl fmt::Debug for ProvideBuffersError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_tuple("ProvideBuffersError")
      .field(&self.error)
      .finish()
  }
}
