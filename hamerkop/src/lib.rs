//!

#![feature(min_specialization)]

pub extern crate uring;

#[macro_use]
extern crate log;
#[macro_use]
extern crate metrics;

mod error;
mod fixedvec;
mod stable_slotmap;

mod executor;
pub mod util;

use crate::stable_slotmap::StableSlotmap;

pub use crate::error::ProvideBuffersError;
pub use crate::executor::{CompletionFuture, IOHandle, LinkedSubmitter, Runtime};
pub use crate::fixedvec::FixedVec;
