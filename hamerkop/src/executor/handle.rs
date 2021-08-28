use super::runtime::RECV_CQE_DIST;
use super::{
  CleanupAction, CompletionEvent, CompletionFuture, FutureObj, DEAD_TASK_SENTINEL, NO_TASK_SENTINEL,
};
use crate::{FixedVec, ProvideBuffersError, StableSlotmap};

use uring::{
  sqes::{ProvideBuffers, Read, Target, Write},
  CompletionQueueEvent,
};
use uring::{CompletionQueue, SubmissionFlags, SubmissionQueue, SubmissionQueueEvent};

use std::cell::{RefCell, RefMut};
use std::future::Future;
use std::io;
use std::rc::Rc;

use std::os::raw::c_int as RawFd;

pub(crate) struct IOContext<'ring> {
  pub(super) sq: SubmissionQueue<'ring>,
  pub(super) cq: CompletionQueue<'ring>,
  pub(super) created: Vec<FutureObj<'ring>>,
  pub(super) completions: StableSlotmap<CompletionEvent>,
  pub(super) woken: Vec<usize>,

  pub(super) buffers: StableSlotmap<FixedVec<u8>>,
  pub(super) bufsize: usize,
  /// The number of SQEs which are queued but have not been submitted
  pub(super) queued: usize,
}

impl<'ring> IOContext<'ring> {
  pub fn has_queued(&self) -> bool {
    self.queued != 0
  }

  pub unsafe fn submit(&mut self, wait_for: u32) -> io::Result<()> {
    loop {
      match self.sq.submit_and_wait(wait_for) {
        Err(e) if e.raw_os_error() == Some(libc::EBUSY) => (),
        Err(e) if e.raw_os_error() == Some(libc::EAGAIN) => (),
        res => break res.map(|_| ()),
      };

      self.process_cqes(|woken, ce| woken.push(ce.task));
    }
  }

  /// Process all CQEs currently wihin the completion queue.
  ///
  /// Due to the different processing needs for different users
  /// it allows a custom function to handle enqueuing to the
  /// correct runqueue.
  pub(super) fn process_cqes<F>(&mut self, mut queue_func: F)
  where
    F: FnMut(&mut Vec<usize>, &CompletionEvent),
  {
    let available = self.cq.available();
    let count = available.len();

    RECV_CQE_DIST.insert(count as _);

    for cqe in &available {
      let cqid = cqe.user_data() as usize;
      let ce = match self.completions.get_mut(cqid) {
        Some(ce) => ce,
        None => continue,
      };

      ce.cqe = Some(*cqe);

      if ce.task == NO_TASK_SENTINEL {
        // Do nothing, the correct task should already be on the runqueue
      } else if ce.task == DEAD_TASK_SENTINEL {
        // The task that was waiting for this completion is dead and gone.
        // Need to clean it up to avoid leaking completions.
        let ce = self.completions.remove(cqid).unwrap();

        match ce.action {
          CleanupAction::None => (),
          CleanupAction::DeleteBufferOnError(bufid) => {
            if cqe.result().is_err() {
              self.buffers.remove(bufid);
            }
          }
        }
      } else {
        queue_func(&mut self.woken, ce);
      }
    }

    self.cq.advance(count);
  }
}

#[derive(Clone)]
pub struct IOHandle<'ring> {
  pub(super) ctx: Rc<RefCell<IOContext<'ring>>>,
}

impl<'ring> IOHandle<'ring> {
  pub fn buffer_size(&self) -> usize {
    self.ctx.borrow().bufsize
  }

  pub fn buffer_count(&self) -> usize {
    self.ctx.borrow().buffers.len()
  }

  pub fn spawn_dyn(&self, future: FutureObj<'ring>) {
    self.ctx.borrow_mut().created.push(future);
  }

  pub fn spawn<F: Future<Output = ()> + 'ring>(&self, future: F) {
    self.spawn_dyn(Box::pin(future))
  }

  pub fn submit_linked<'h>(&'h mut self) -> LinkedSubmitter<'h, 'ring> {
    LinkedSubmitter::new(self)
  }

  /// Submit a custom-prepared SQE and return a future once it completes.
  ///
  /// # Safety
  /// The safety invariants for this function are somewhat complex and not
  /// entirely well-defined. They boil down to ensuring that any memory
  /// pointers that are passed to the kernel live until the corresponding
  /// request completes (whether successfully or otherwise).
  pub unsafe fn submit_sqe<F: FnOnce(&mut SubmissionQueueEvent)>(
    &self,
    func: F,
  ) -> impl Future<Output = io::Result<CompletionQueueEvent>> + 'ring {
    let mut submitter = LinkedSubmitter::new(self);
    let fut = submitter.submit_sqe(func);
    submitter.submit();

    async move { Ok(fut?.await) }
  }

  /// Read from a file descriptor to a buffer within the buffer pool.
  pub fn read(
    &self,
    fd: RawFd,
    size: usize,
  ) -> impl Future<Output = io::Result<FixedVec<u8>>> + 'ring {
    let mut submitter = LinkedSubmitter::new(self);
    let fut = submitter.read(fd, size);
    submitter.submit();

    fut
  }

  /// Read from a file descriptor.
  ///
  /// # Safety
  /// UB occurs if this future is not polled to completion.
  pub unsafe fn read_buf<'a>(
    &self,
    fd: RawFd,
    data: &'a mut [u8],
  ) -> impl Future<Output = io::Result<usize>> + 'a + 'ring
  where
    'ring: 'a,
  {
    use std::slice::from_raw_parts_mut;

    let mut submitter = LinkedSubmitter::new(self);
    let fut = submitter.read_buf(fd, from_raw_parts_mut(data.as_mut_ptr(), data.len()));
    submitter.submit();

    fut
  }

  /// Write to a file descriptor.
  ///
  /// # Safety
  /// UB occurs if this future is not polled to completion.
  pub unsafe fn write_buf<'a>(
    &self,
    fd: RawFd,
    data: &'a [u8],
  ) -> impl Future<Output = io::Result<usize>> + 'a + 'ring
  where
    'ring: 'a,
  {
    use std::slice::from_raw_parts;

    let mut submitter = LinkedSubmitter::new(self);
    let fut = submitter.write_buf(fd, from_raw_parts(data.as_ptr(), data.len()));
    submitter.submit();

    fut
  }

  pub fn provide_buffer(
    &self,
    buffer: FixedVec<u8>,
  ) -> impl Future<Output = Result<(), ProvideBuffersError>> + 'ring {
    assert_eq!(
      self.buffer_size(),
      buffer.capacity(),
      "Provided buffer had an incorrect capacity"
    );

    let mut submitter = LinkedSubmitter::new(self);
    let fut = submitter.provide_buffer(buffer);
    submitter.submit();

    fut
  }
}

pub struct LinkedSubmitter<'handle, 'ring> {
  handle: &'handle IOHandle<'ring>,
  ctx: RefMut<'handle, IOContext<'ring>>,
  count: usize,
}

impl<'handle, 'ring> LinkedSubmitter<'handle, 'ring> {
  fn new(handle: &'handle IOHandle<'ring>) -> Self {
    Self {
      handle,
      ctx: handle.ctx.borrow_mut(),
      count: 0,
    }
  }

  /// Submit a custom-prepared SQE and return a future once it completes.
  ///
  /// # Panics
  /// This function panics if it is used to try and link more SQEs than there
  /// are possible entries within the uring.
  unsafe fn submit_sqe<F>(&mut self, func: F) -> io::Result<CompletionFuture<'ring>>
  where
    F: FnOnce(&mut SubmissionQueueEvent),
  {
    let (fut, cqid) = CompletionFuture::from_ctx(&mut self.ctx, self.handle);

    assert_ne!(
      self.count,
      self.ctx.sq.queue_size(),
      "Cannot link more SQEs than there are room for in the queue"
    );

    let sqe = loop {
      if let Some(sqe) = self.ctx.sq.sqes().get_mut(self.count) {
        break sqe;
      }

      self.ctx.submit(0)?;
    };

    func(sqe);
    sqe.set_user_data(cqid as u64);
    sqe.add_flags(SubmissionFlags::IO_LINK);

    self.count += 1;

    Ok(fut)
  }

  pub fn read(
    &mut self,
    fd: RawFd,
    size: usize,
  ) -> impl Future<Output = io::Result<FixedVec<u8>>> + 'ring {
    let cqe = unsafe {
      self.submit_sqe(|sqe| {
        sqe.prepare(Read::new(Target::Fd(fd), std::ptr::null_mut(), size as u32));
        sqe.set_buf_group(0);
        sqe.set_flags(SubmissionFlags::BUFFER_SELECT | SubmissionFlags::ASYNC);
      })
    };
    let handle = self.handle.clone();

    async move {
      let cqe = cqe?.await;

      let size = cqe.result()?;
      let bufid = cqe.buf_id().expect("Invalid CQE for read") as usize;

      let mut buf = handle
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
  }

  pub unsafe fn read_buf<'a>(
    &mut self,
    fd: RawFd,
    data: &'a mut [u8],
  ) -> impl Future<Output = io::Result<usize>> + 'a + 'ring
  where
    'ring: 'a,
  {
    assert!(data.len() <= u32::MAX as usize);

    let mut cqe = self.submit_sqe(|sqe| {
      sqe.prepare(Read::new(
        Target::Fd(fd),
        data.as_mut_ptr(),
        data.len() as u32,
      ));
    });

    if let Ok(fut) = &mut cqe {
      fut.set_must_wait(true);
    }

    async move { cqe?.await.result() }
  }

  pub unsafe fn write_buf<'a>(
    &mut self,
    fd: RawFd,
    data: &'a [u8],
  ) -> impl Future<Output = io::Result<usize>> + 'a + 'ring
  where
    'ring: 'a,
  {
    assert!(data.len() <= u32::MAX as usize);

    let mut cqe = self.submit_sqe(|sqe| {
      sqe.prepare(Write::new(Target::Fd(fd), data.as_ptr(), data.len() as u32));
    });

    if let Ok(fut) = &mut cqe {
      fut.set_must_wait(true);
    }

    async move { cqe?.await.result() }
  }

  pub fn provide_buffer(
    &mut self,
    mut buffer: FixedVec<u8>,
  ) -> impl Future<Output = Result<(), ProvideBuffersError>> + 'ring {
    assert_eq!(
      self.ctx.bufsize,
      buffer.capacity(),
      "Provided buffer had an incorrect capacity"
    );

    let len = buffer.len();

    let res = if self.ctx.buffers.len() >= u16::MAX as usize {
      Err(ProvideBuffersError::new(
        io::Error::new(io::ErrorKind::Other, "No more room for buffers"),
        buffer,
      ))
    } else {
      buffer.clear();

      let bufptr = buffer.as_mut_ptr();
      let buflen = buffer.capacity();
      let bufid = self.ctx.buffers.insert(buffer);

      let res = unsafe {
        self.submit_sqe(move |sqe| {
          sqe.prepare(ProvideBuffers::new(
            bufptr as _,
            buflen as u32,
            1,
            0,
            bufid as u16,
          ));
        })
      };

      match res {
        Ok(fut) => {
          self.ctx.completions[fut.cqid()].action = CleanupAction::DeleteBufferOnError(bufid);

          Ok((fut, bufid))
        }
        Err(e) => {
          let buffer = self.ctx.buffers.remove(bufid).unwrap();
          Err(ProvideBuffersError::new(e, buffer))
        }
      }
    };

    let handle = self.handle.clone();

    async move {
      let (fut, bufid) = res?;
      let cqe = fut.await;

      match cqe.result() {
        Ok(_) => Ok(()),
        Err(e) => {
          let mut buffer = handle.ctx.borrow_mut().buffers.remove(bufid).unwrap();
          // SAFETY: The buffer was originally this length, none of the data in it has
          //         changed, so it is safe to set the length back to what it originally
          //         was.
          unsafe { buffer.set_len(len) };
          Err(ProvideBuffersError::new(e, buffer))
        }
      }
    }
  }

  pub fn submit(mut self) {
    if self.count != 0 {
      let last = self.ctx.sq.sqes().get_mut(self.count - 1).unwrap();
      last.set_flags(last.flags() & !SubmissionFlags::IO_LINK);

      self.ctx.sq.advance(self.count as u32);
      self.ctx.queued += self.count;
    }
  }
}
