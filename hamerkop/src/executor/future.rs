use super::{
  CompletionEvent, IOContext, IOHandle, CURRENT_TASK, DEAD_TASK_SENTINEL, NO_TASK_SENTINEL,
};

use uring::{sqes::AsyncCancel, CompletionQueueEvent};

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

/// A future that waits for a CompletionQueueEvent.
///
/// This will always regiester the last future that polled it as the one
/// to be woken up.
///
/// # Panics
/// This future will panic if poll is called on a non-uring executor.
pub struct CompletionFuture<'ring> {
  ctx: Rc<RefCell<IOContext<'ring>>>,
  cqid: usize,
  must_wait: bool,
}

impl<'ring> CompletionFuture<'ring> {
  /// Create a completion future when there's already a reference to the
  /// internal IO context.
  ///
  /// This is meant to be used internally when there's already a reference
  /// to the internal context.
  pub(crate) fn from_ctx(ctx: &mut IOContext<'ring>, handle: &IOHandle<'ring>) -> (Self, usize) {
    let cqid = ctx
      .completions
      .insert(CompletionEvent::new(NO_TASK_SENTINEL));

    (
      Self {
        ctx: handle.ctx.clone(),
        cqid,
        must_wait: false,
      },
      cqid,
    )
  }

  pub fn cqid(&self) -> usize {
    self.cqid
  }

  /// Set whether dropping this future should block until this request has
  /// completed.
  ///
  /// This should be set when the corresponding SQE contains a reference to
  /// memory not managed by the executor (e.g. it's not necessary for a buffer
  /// within the buffer pool).
  pub fn set_must_wait(&mut self, must_wait: bool) {
    self.must_wait = must_wait;
  }

  pub fn new(handle: &IOHandle<'ring>) -> (Self, usize) {
    let mut ctx = handle.ctx.borrow_mut();
    Self::from_ctx(&mut ctx, handle)
  }
}

impl<'ring> Future for CompletionFuture<'ring> {
  type Output = CompletionQueueEvent;

  fn poll(self: Pin<&mut Self>, _: &mut Context) -> Poll<Self::Output> {
    let mut ctx = self.ctx.borrow_mut();

    let cq = match ctx.completions.get_mut(self.cqid) {
      Some(cq) => cq,
      // If this happens then it's most likely a bug.
      None => panic!("Corrupt CompletionFuture: CQID was invalid."),
    };

    cq.task = CURRENT_TASK
      .with(Cell::get)
      .expect("Polled CompletionFuture outside of a uring context");

    match cq.cqe {
      Some(cqe) => Poll::Ready(cqe),
      None => Poll::Pending,
    }
  }
}

impl<'ring> Drop for CompletionFuture<'ring> {
  fn drop(&mut self) {
    let mut ctx = self.ctx.borrow_mut();

    let ce = match ctx.completions.get_mut(self.cqid) {
      Some(cq) => cq,
      // If this happens then it's most likely a bug.
      None => panic!("Corrupt CompletionFuture: CQID {} was invalid.", self.cqid),
    };

    if ce.cqe.is_some() {
      ctx.completions.remove(self.cqid);
    } else if !self.must_wait {
      ce.task = DEAD_TASK_SENTINEL;
    } else {
      // Basic steps that need to be taken when dropping such a task:
      // 1. Cancel the existing task (mark its CompletionEvent with
      //    DEAD_TASK_SENTINEL and submit an ASYNC_CANCEL SQE)
      // 2. Spin on the completion queue until the right CompletionQueueEvent
      //    is received.

      // We don't want to enqueue the current task into the queue while busy-waiting
      // since we're already running.
      ce.task = NO_TASK_SENTINEL;

      // TODO: Can this potentially loop forever in certain error conditions?
      let sqe = loop {
        if let Some(sqe) = ctx.sq.sqes().get_mut(0) {
          break sqe;
        }

        // TODO: May have to actually abort if this fails. Otherwise we may end up
        //       spinning indefinitely.
        let _ = unsafe { ctx.submit(0) };
      };

      sqe.prepare(AsyncCancel::new(self.cqid as _));
      // We don't bother allocating a CompletionEvent for the cancellation task
      // so we explicitly set its userdata as invalid userdata.
      sqe.set_user_data(u64::MAX);

      loop {
        // TODO: Not sure what to do if this errors...
        let _ = unsafe { ctx.submit(1) };

        let ce = ctx.completions.get_mut(self.cqid).unwrap();
        if ce.cqe.is_some() {
          break;
        }
      }
    }
  }
}
