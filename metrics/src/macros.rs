
/// Macro to extract the parameter from the first #[name = "blah"] attribute
/// or use a default otherwise.
#[doc(hidden)]
#[macro_export]
macro_rules! __metric_first_name {
  { $name:ident => { $( #[$( $attr:tt )+] )*  }} => {{
    const NAMES: &[&'static str] = &$crate::__metric_first_name!({ $( #[$( $attr )+] )* } []);

    if !NAMES.is_empty() {
      NAMES[NAMES.len() - 1]
    } else {
      concat!(module_path!(), "::", stringify!($name))
    }
  }};
  ( { #[name = $name:expr] $( #[$( $attr:tt )+] )* } [ $( $rest:expr ),* ] ) => {
    $crate::__metric_first_name!({ $( #[$( $attr )+] )* } [ $name $(, $rest )* ])
  };
  ( { #[$( $attr:tt )+] $( #[$( $rattr:tt )+] )* } [ $( $rest:expr ),* ]) => {
    $crate::__metric_first_name!({ $( #[$( $rattr )+] )* } [ $( $rest ),* ])
  };
  ( {} [ $( $rest:expr ),* ]) => {
    [ $( $rest ),* ]
  }
}

/// Macro that filters out attributes of the style
/// #[name = "expr"]
#[doc(hidden)]
#[macro_export]
macro_rules! __metric_filter_attrs {
  {
    $( #[$( $attr:tt )+] )*
    $vis:vis static $name:ident : $ty:ty = $init:expr;
  } => {
    $crate::__metric_filter_attrs!(
      { $( #[$( $attr )+] )* }
      $vis static $name: $ty = $init;
    );
  };
  (
    {}
    $( #[$( $attr:tt )+] )*
    $vis:vis static $name:ident : $ty:ty = $init:expr;
  ) => {
    $( #[$( $attr )+] )*
    $vis static $name: $ty = $init;
  };
  (
    { #[name = $value:expr] $( [$( $left:tt )+] )* }
    $( #[$( $attr:tt )+] )*
    $vis:vis static $name:ident : $ty:ty = $init:expr;
  ) => {
    $crate::__metric_filter_attrs!(
      { $( #[$( $left )+] )* }
      $( #[$( $attr )+] )*
      $vis static $name : $ty = $init;
    );
  };
  (
    { #[$( $current:tt )+] $( #[$( $left:tt )+] )* }
    $( #[$( $attr:tt )+] )*
    $vis:vis static $name:ident : $ty:ty = $init:expr;
  ) => {
    $crate::__metric_filter_attrs!(
      { $( #[$( $left )+] )* }
      #[$( $current )+]
      $( #[$( $attr )+] )*
      $vis static $name: $ty = $init;
    );
  }
}

/// Declare a set of new metrics.
/// 
/// These metrics will appear in the global array returned by the [`metrics`]
/// function without anything else needing to be done.
/// 
/// # Syntax
/// The basic syntax is
/// ```
/// # use metrics::{metric, Gauge};
/// metric! {
///   /// My custom metric!
///   pub static METRIC: Gauge = Gauge::new();
/// }
/// ```
/// The above declares a gauge metric with its name being the the fully qualified
/// path of the metric. That is, if we declared the above in the crate `metrics`
/// and in the module `custom` then its name would be `metrics::custom::METRIC`.
/// 
/// The name of the metric can be customized by adding a `name` attribute like so
/// ```
/// # use metrics::{metric, Counter};
/// metric! {
///   /// A counter!
///   #[name = "my-counter"]
///   pub static METRIC: Counter = Counter::new();
/// }
/// ```
/// The above declares a counter metric with name `my-counter`. The name attribute
/// can be freely mixed with other attributes. If multiple `name` attributes are
/// specified then the last one will be used.
/// 
/// [`metrics`]: crate::metrics
#[macro_export]
macro_rules! metric {
  {
    $(
      $( #[ $( $attr:tt )+ ] )*
      $vis:vis static $name:ident : $ty:ty = $init:expr ;
    )*
  } => {
    $(
      $crate::__metric_filter_attrs! {
        $( #[$( $attr )+] )*
        $vis static $name: $ty = {
          #[$crate::export::linkme::distributed_slice($crate::export::METRICS)]
          #[$crate::export::replace(linkme, metrics::export::linkme)]
          static __METRIC_ENTRY: $crate::MetricEntry = $crate::MetricEntry {
            name: $crate::__metric_first_name!{
              $name => {
                $( #[$( $attr )+] )*
              }
            },
            metric: &$name
          };

          $init
        };
      }
    )*
  }
}
