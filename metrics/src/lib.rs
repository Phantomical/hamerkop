//!

mod macros;

mod counter;
mod ddsketch;
mod gauge;
mod lazy_cell;

use std::{any::Any, fmt, ops::Deref};

pub use crate::counter::Counter;
pub use crate::ddsketch::DDSketch;
pub use crate::gauge::Gauge;
pub use crate::lazy_cell::LazyCell;

#[doc(hidden)]
pub mod export {
  pub use super::{Metric, MetricEntry};

  pub extern crate linkme;

  #[linkme::distributed_slice]
  pub static METRICS: [MetricEntry] = [..];
}

/// Global interface to a metric.
///
/// Most use of metrics should use the directly declared constants.
pub trait Metric: Sync {
  /// Indicate whether this metric has been set up.
  ///
  /// Generally, if this returns `false` then the other methods on this
  /// trait should return `None`.
  fn is_enabled(&self) -> bool {
    true
  }

  /// Get the value of the metric at the current point in time.
  ///
  /// For single-value metrics this will return the value of the metric
  /// at the current point in time. For more complex metric, this will
  /// return a view into the metric's state which may change as the
  /// program modifies the metric.
  fn value(&self) -> Option<Value>;

  /// Get the current metric as an [`Any`] instance. This is meant to allow
  /// custom processing for known metric types.
  ///
  /// [`Any`]: std::any::Any
  fn as_any(&self) -> Option<&dyn Any>;
}

pub trait Summary {
  /// The total number of elements that have been added to the summary.
  fn count(&self) -> u64;

  /// Estimate the quantile q for the summary.
  fn quantile(&self, q: f64) -> u64;

  /// Estimate the quantile of the value v if it were to be inserted into the
  /// summary.
  fn rank(&self, v: u64) -> f64;
}

/// The value of a metric.
///
/// For simple metrics this is a single value, and for more complicated metrics
/// this corresponds to an interface that allows access.
#[non_exhaustive]
pub enum Value<'a> {
  Unsigned(u64),
  Signed(i64),
  Summary(&'a dyn Summary),
  OwnedSummary(Box<dyn Summary>),
  Other(&'a dyn Any),
}

/// A statically declared metric entry.
pub struct MetricEntry {
  #[doc(hidden)]
  pub metric: &'static dyn Metric,
  #[doc(hidden)]
  pub name: &'static str,
}

impl MetricEntry {
  pub fn metric(&self) -> &'static dyn Metric {
    self.metric
  }

  pub fn name(&self) -> &'static str {
    self.name
  }
}

/// Get a list of all globally created metrics.
pub fn metrics() -> &'static [MetricEntry] {
  &*crate::export::METRICS
}

impl Deref for MetricEntry {
  type Target = dyn Metric;

  fn deref(&self) -> &Self::Target {
    self.metric()
  }
}

impl fmt::Debug for MetricEntry {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("MetricEntry")
      .field("name", &self.name())
      .field("metric", &"<dyn Metric>")
      .finish()
  }
}
