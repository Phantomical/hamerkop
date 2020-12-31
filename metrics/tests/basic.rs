use metrics::{metric, Counter, DDSketch, Gauge, LazyCell, Value};

metric! {
  static TEST_METRIC_1: Gauge = Gauge::new();
  #[name = "test-metric"]
  static TEST_METRIC_2: Counter = Counter::new();

  #[name = "lazy-metric"]
  static LAZY_METRIC: LazyCell<DDSketch> = LazyCell::new(|| DDSketch::new(0.01));
}

#[test]
fn lazy_metric_lazily_enabled() {
  let metric = metrics::metrics()
    .iter()
    .find(|x| x.name() == "lazy-metric");

  assert!(metric.is_some());
  let metric = metric.unwrap();

  assert!(!metric.metric().is_enabled());
  LAZY_METRIC.insert(5);
  assert!(metric.metric().is_enabled());
}

#[test]
fn metric_names() {
  let test_metric_1 = metrics::metrics()
    .into_iter()
    .find(|x| x.name() == "basic::TEST_METRIC_1");

  let test_metric_2 = metrics::metrics()
    .into_iter()
    .find(|x| x.name() == "test-metric");

  assert!(test_metric_1.is_some());
  assert!(test_metric_2.is_some());
}

#[test]
fn metric_increment() {
  TEST_METRIC_1.add(4);
  TEST_METRIC_2.add(3);
  
  let test_metric_1 = metrics::metrics()
    .into_iter()
    .find(|x| x.name() == "basic::TEST_METRIC_1")
    .unwrap();

  let test_metric_2 = metrics::metrics()
    .into_iter()
    .find(|x| x.name() == "test-metric")
    .unwrap();

  let cnt1 = match test_metric_1.metric().value().unwrap() {
    Value::Signed(val) => val,
    _ => unreachable!()
  };
  let cnt2 = match test_metric_2.metric().value().unwrap() {
    Value::Unsigned(val) => val,
    _ => unreachable!()
  };

  assert_eq!(cnt1, 4);
  assert_eq!(cnt2, 3);
}

#[test]
fn num_metrics() {
  assert_eq!(metrics::metrics().len(), 3);
}
