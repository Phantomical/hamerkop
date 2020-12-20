use std::collections::VecDeque;
use std::future::Future;
use std::io;
use std::os::raw::c_int as RawFd;
use std::panic;
use std::pin::Pin;
use std::rc::Rc;
use std::{
  cell::{Cell, RefCell},
  task::{Context, Poll},
};

use crate::{FixedVec, StableSlotmap};
use cooked_waker::IntoWaker;
use uring::{
  sqes::{ProvideBuffers, Read, Target, Write},
  SubmissionQueueEvent,
};
use uring::{CompletionQueue, CompletionQueueEvent, SubmissionFlags, SubmissionQueue};

thread_local! {
  static CURRENT_TASK: Cell<Option<usize>> = Cell::new(None);
}

const NO_TASK_SENTINEL: usize = usize::MAX;

type FutureObj<'ring> = Pin<Box<dyn Future<Output = ()> + 'ring>>;

#[derive(Clone)]
struct EmptyWaker;

impl cooked_waker::WakeRef for EmptyWaker {
  fn wake_by_ref(&self) {
    // TODO: Implement
  }
}
impl cooked_waker::Wake for EmptyWaker {
  fn wake(self) {
    // TODO: Implement
  }
}

impl cooked_waker::ViaRawPointer for EmptyWaker {
  type Target = ();

  fn into_raw(self) -> *mut Self::Target {
    std::ptr::null_mut()
  }

  unsafe fn from_raw(_: *mut Self::Target) -> Self {
    Self
  }
}

struct CompletionEvent {
  cqe: Option<CompletionQueueEvent>,
  task: usize,
}

impl CompletionEvent {
  pub fn new(task: usize) -> Self {
    Self { task, cqe: None }
  }
}

pub(crate) struct IOContext<'ring> {
  sq: SubmissionQueue<'ring>,
  created: VecDeque<FutureObj<'ring>>,
  completions: StableSlotmap<CompletionEvent>,

  buffers: StableSlotmap<FixedVec<u8>>,
  bufsize: usize,
  /// The number of SQEs which are queued but have not been submitted
  queued: usize,
}

#[derive(Clone)]
pub struct IOHandle<'ring> {
  ctx: Rc<RefCell<IOContext<'ring>>>,
}

pub struct Runtime<'ring> {
  futures: StableSlotmap<FutureObj<'ring>>,
  cq: CompletionQueue<'ring>,
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
      created: VecDeque::new(),
      completions: StableSlotmap::new(),
      buffers: StableSlotmap::new(),
      bufsize,
      queued: 0,
    }));

    let rt = Self {
      futures: StableSlotmap::new(),
      cq,
      ctx: Rc::clone(&ctx),
      runqueue: Vec::with_capacity(128),
    };

    (IOHandle { ctx }, rt)
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
    while let Some(future) = ctx.created.pop_front() {
      let id = self.futures.insert(future);
      trace!("Spawned new future with id {}", id);
      self.runqueue.push(id);
    }

    if ctx.queued != 0 || (self.runqueue.is_empty() && !self.cq.has_next()) {
      let wait_for = match self.cq.has_next() || !self.runqueue.is_empty() {
        true => 0,
        false => 1,
      };

      trace!("Waiting for {} SQEs", wait_for);

      unsafe { ctx.sq.submit_and_wait(wait_for)? };
    }
    ctx.queued = 0;

    // Process all CQEs and queue up their corresponding futures.
    let available = self.cq.available();
    trace!("Got {} completions", available.len());
    trace!("CQIDs: {:?}", (&available).into_iter().map(|x| x.user_data()).collect::<Vec<_>>());
    for cqe in &available {
      let cqid = cqe.user_data() as usize;

      // Else condition happens if future gets dropped before completion
      if let Some(ce) = ctx.completions.get_mut(cqid) {
        ce.cqe = Some(*cqe);
        if ce.task != NO_TASK_SENTINEL {
          // trace!("Got completion for future with id {}", ce.task);
          self.runqueue.push(ce.task);
        }
      }
    }
    let count = available.len();
    self.cq.advance(count);

    drop(ctx);

    trace!("Polling {} futures", self.runqueue.len());

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
        },
      };
      let mut context = Context::from_waker(&waker);

      // trace!("Polling future with id {}", id);

      CURRENT_TASK.with(|task| task.set(Some(id)));
      let result = panic::catch_unwind(panic::AssertUnwindSafe(|| future.poll(&mut context)));
      CURRENT_TASK.with(|task| task.set(None));

      match result {
        Ok(Poll::Ready(())) => {
          trace!("Future with id {} returned ready", id);
          self.futures.remove(id);
        }
        Ok(Poll::Pending) => {}
        Err(e) => {
          self.futures.remove(id);
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

impl<'ring> IOHandle<'ring> {
  pub fn buffer_size(&self) -> usize {
    self.ctx.borrow().bufsize
  }

  pub fn spawn_dyn(&self, future: FutureObj<'ring>) {
    self.ctx.borrow_mut().created.push_back(future);
  }

  pub fn spawn<F: Future<Output = ()> + 'ring>(&self, future: F) {
    self.spawn_dyn(Box::pin(future))
  }

  /// Submit a custom-prepared SQE and return a future once it completes.
  ///
  /// # Panics
  /// This function panics if it is not run within a currently executing future
  /// context (i.e. no task is current).
  pub fn submit_sqe<F: FnOnce(&mut SubmissionQueueEvent)>(
    &self,
    func: F,
  ) -> io::Result<CompletionFuture<'ring>> {
    let (fut, cqid) = CompletionFuture::new(self);

    let mut ctx = self.ctx.borrow_mut();
    let sqe = loop {
      if let Some(sqe) = ctx.sq.sqes().get_mut(0) {
        break sqe;
      }

      unsafe { ctx.sq.submit()? };
    };

    func(sqe);
    sqe.set_user_data(cqid as u64);

    trace!("Submitted SQE with CQID {}", cqid);

    ctx.sq.advance(1);
    ctx.queued += 1;
    drop(ctx);

    Ok(fut)
  }

  pub async fn read(&self, fd: RawFd, size: usize) -> io::Result<FixedVec<u8>> {
    let cqe = self
      .submit_sqe(|sqe| {
        sqe.prepare(Read::new(Target::Fd(fd), std::ptr::null_mut(), size as u32));
        sqe.set_buf_group(0);
        sqe.set_flags(SubmissionFlags::BUFFER_SELECT);
      })?
      .await;

    let size = cqe.result()?;
    let bufid = cqe.buf_id().expect("Invalid CQE for read") as usize;

    let mut buf = self
      .ctx
      .borrow_mut()
      .buffers
      .remove(bufid)
      .expect("Buffer ID doesn't exist");

    // SAFETY: Safe since the kernel has filled in the buffer with the memory
    //         that has been read from fd.
    unsafe { buf.set_len(size) };

    Ok(buf)
  }

  /// Read from a file descriptor.
  ///
  /// # Safety
  /// UB occurs if this future is not polled to completion.
  pub async unsafe fn read_buf(&self, fd: RawFd, data: &mut [u8]) -> io::Result<usize> {
    assert!(data.len() <= u32::MAX as usize);

    let cqe = self
      .submit_sqe(|sqe| {
        sqe.prepare(Read::new(
          Target::Fd(fd),
          data.as_mut_ptr(),
          data.len() as u32,
        ));
      })?
      .await;

    cqe.result()
  }

  /// Write to a file descriptor.
  ///
  /// # Safety
  /// UB occurs if this future is not polled to completion.
  pub async unsafe fn write_buf(&self, fd: RawFd, data: &[u8]) -> io::Result<usize> {
    assert!(data.len() <= u32::MAX as usize);

    let cqe = self
      .submit_sqe(|sqe| {
        sqe.prepare(Write::new(Target::Fd(fd), data.as_ptr(), data.len() as u32));
      })?
      .await;

    cqe.result()
  }

  pub async fn provide_buffer(&self, mut buffer: FixedVec<u8>) -> io::Result<()> {
    assert_eq!(
      self.buffer_size(),
      buffer.capacity(),
      "Provided buffer had an incorrect capacity"
    );

    // If there's too many buffers then silently drop this one.
    if self.ctx.borrow().buffers.len() >= u16::MAX as usize {
      return Ok(());
    }

    buffer.clear();

    let bufptr = buffer.as_mut_ptr();
    let buflen = buffer.len();
    let bufid = self.ctx.borrow_mut().buffers.insert(buffer);

    self
      .submit_sqe(move |sqe| {
        sqe.prepare(ProvideBuffers::new(
          bufptr as _,
          buflen as u32,
          1,
          0,
          bufid as u16,
        ));
      })?
      .await
      .result()?;

    Ok(())
  }
}

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
}

impl<'ring> CompletionFuture<'ring> {
  pub fn new(handle: &IOHandle<'ring>) -> (Self, usize) {
    let mut ctx = handle.ctx.borrow_mut();
    let cqid = ctx
      .completions
      .insert(CompletionEvent::new(NO_TASK_SENTINEL));

    (
      Self {
        ctx: handle.ctx.clone(),
        cqid,
      },
      cqid,
    )
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
    self.ctx.borrow_mut().completions.remove(self.cqid);
  }
}
