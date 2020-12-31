//!

#![feature(min_specialization, min_const_generics)]

pub extern crate uring;

#[macro_use]
extern crate log;
#[macro_use]
extern crate metrics;

mod stable_slotmap;
mod fixedvec;
mod error;

mod executor;
pub mod util;

use crate::stable_slotmap::StableSlotmap;

pub use crate::fixedvec::FixedVec;
pub use crate::executor::{Runtime, IOHandle, CompletionFuture, LinkedSubmitter};
pub use crate::error::ProvideBuffersError;
