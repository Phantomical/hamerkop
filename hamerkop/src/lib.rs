//!

pub extern crate uring;

#[macro_use]
extern crate log;

mod stable_slotmap;
mod fixedvec;

mod executor;

use crate::stable_slotmap::StableSlotmap;

pub use crate::fixedvec::FixedVec;
pub use crate::executor::{Runtime, IOHandle, CompletionFuture};

pub fn check_fixedvec(v: &mut FixedVec<u8>, u: &mut FixedVec<u8>) {
  v.append(u)
}
