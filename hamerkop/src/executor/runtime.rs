use super::{EmptyWaker, FutureObj, IOContext, IOHandle, CURRENT_TASK};
use crate::StableSlotmap;

use cooked_waker::IntoWaker;
use metrics::{DDSketch, Gauge, LazyCell};
use uring::{CompletionQueue, SubmissionQueue};

use std::cell::RefCell;
use std::future::Future;
use std::io;
use std::panic;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

metric! {
  #[name = "hamerkop.rt.received_cqes"]
  pub(super) static RECV_CQE_DIST: LazyCell<DDSketch> = LazyCell::new(|| DDSketch::new(0.05));

  #[name = "hamerkop.rt.polled_futures"]
  static POLL_CNT_DIST: LazyCell<DDSketch> = LazyCell::new(|| DDSketch::new(0.01));

  #[name = "hamerkop.rt.active_tasks"]
  static ACTIVE_TASKS: Gauge = Gauge::new();
}

pub struct Runtime<'ring> {
  futures: StableSlotmap<FutureObj<'ring>>,
  ctx: Rc<RefCell<IOContext<'ring>>>,
  /// List of tasks to be run
  runqueue: Vec<usize>,
}

impl<'ring> Runtime<'ring> {
  pub fn new(
    sq: SubmissionQueue<'ring>,
    cq: CompletionQueue<'ring>,
    bufsize: usize,
  ) -> (IOHandle<'ring>, Self) {
    assert_ne!(bufsize, 0);

    let ctx = Rc::new(RefCell::new(IOContext {
      sq,
      cq,
      created: Vec::new(),
      woken: Vec::new(),
      completions: StableSlotmap::new(),
      buffers: StableSlotmap::new(),
      bufsize,
      queued: 0,
    }));

    let rt = Self {
      futures: StableSlotmap::new(),
      ctx: Rc::clone(&ctx),
      runqueue: Vec::with_capacity(128),
    };

    (IOHandle { ctx }, rt)
  }

  /// Process all CQEs and queue up their corresponding futures
  fn process_cqes(&mut self) {
    let mut ctx = self.ctx.borrow_mut();
    let runqueue = &mut self.runqueue;
    ctx.process_cqes(|_, ce| runqueue.push(ce.task));
  }

  /// Run one iteration of the event loop.
  ///
  /// ### Guarantees
  /// - No index will be reused until the next time turn is called.
  ///   This means that if a future completes it's index will remain unused
  ///   until turn is called again.
  fn turn(&mut self) -> Result<(), io::Error> {
    let mut ctx = self.ctx.borrow_mut();

    // Setup all newly-created futures
    for future in ctx.created.drain(..) {
      let id = self.futures.insert(future);
      ACTIVE_TASKS.increment();
      self.runqueue.push(id);
    }

    self.runqueue.append(&mut ctx.woken);

    if ctx.has_queued() || (self.runqueue.is_empty() && !ctx.cq.has_next()) {
      let wait_for = match ctx.cq.has_next() || !self.runqueue.is_empty() {
        true => 0,
        false => 1,
      };

      unsafe { ctx.submit(wait_for)? };
    }
    ctx.queued = 0;
    drop(ctx);

    self.process_cqes();

    // Don't actually need accurate deduplication here since polling the futures
    // multiple times unnecessarily isn't a problem, it's just inefficient.
    crate::util::dedup_partial(&mut self.runqueue);

    POLL_CNT_DIST.insert(self.runqueue.len() as _);

    // Run all queued up futures
    let waker = EmptyWaker.into_waker();
    for id in self.runqueue.drain(..) {
      let future = match self.futures.get_mut(id) {
        Some(future) => Pin::as_mut(future),
        // This case can happen if the same future has been queued to be woken
        // up multiple times and completed somewhere in the middle of that.
        //
        // We could prevent duplicates but that's probably not needed right now.
        None => {
          debug!("Unable to find future with id {}", id);
          continue;
        }
      };
      let mut context = Context::from_waker(&waker);

      // trace!("Polling future with id {}", id);

      CURRENT_TASK.with(|task| task.set(Some(id)));
      let result = panic::catch_unwind(panic::AssertUnwindSafe(|| future.poll(&mut context)));
      CURRENT_TASK.with(|task| task.set(None));

      match result {
        Ok(Poll::Ready(())) => {
          self.futures.remove(id);
          ACTIVE_TASKS.decrement();
        }
        Ok(Poll::Pending) => {}
        Err(e) => {
          self.futures.remove(id);
          ACTIVE_TASKS.decrement();
          panic::resume_unwind(e);
        }
      }
    }

    Ok(())
  }

  fn is_empty(&self) -> bool {
    self.futures.is_empty() && self.ctx.borrow().created.is_empty()
  }

  pub fn spawn_dyn(&mut self, future: FutureObj<'ring>) -> usize {
    let id = self.futures.insert(future);
    self.runqueue.push(id);
    id
  }

  /// Spawn a new future onto the runtime.
  pub fn spawn<F: Future<Output = ()> + 'ring>(&mut self, future: F) -> usize {
    self.spawn_dyn(Box::pin(future))
  }

  /// Run until the provided future completes.
  pub fn block_on<T, F: Future<Output = T>>(&mut self, future: F) -> Result<T, io::Error> {
    let mut output = None;

    let ptr = &mut output;
    let future: FutureObj = Box::pin(async move {
      let x = future.await;
      *ptr = Some(x);
    });

    // This forces the lifetime of the future to be static.
    // This is only safe if we ensure that the future is no longer stored
    // within the runtime when this method completes.
    let id = self.spawn_dyn(unsafe { std::mem::transmute(future) });

    let res = panic::catch_unwind(panic::AssertUnwindSafe(|| {
      while output.is_none() {
        self.turn()?;
      }

      Ok(output.unwrap())
    }));

    match res {
      Ok(x) => x,
      Err(e) => {
        // This ensures that the future we created is no longer present even
        // in the case where a panic occurs.
        self.futures.remove(id);
        ACTIVE_TASKS.decrement();
        panic::resume_unwind(e);
      }
    }
  }

  /// Run until all futures in the runtime have completed.
  pub fn run(&mut self) -> Result<(), io::Error> {
    while !self.is_empty() {
      self.turn()?;
    }

    Ok(())
  }
}

impl Drop for Runtime<'_> {
  fn drop(&mut self) {
    ACTIVE_TASKS.sub(self.futures.len() as _)
  }
}
