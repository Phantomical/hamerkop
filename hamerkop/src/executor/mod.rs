//!

mod future;
mod handle;
mod runtime;
mod waker;

pub use self::future::CompletionFuture;
pub use self::handle::{IOHandle, LinkedSubmitter};
pub use self::runtime::Runtime;

use self::handle::IOContext;
use self::waker::EmptyWaker;

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;

use uring::CompletionQueueEvent;

thread_local! {
  static CURRENT_TASK: Cell<Option<usize>> = Cell::new(None);
}

const NO_TASK_SENTINEL: usize = usize::MAX;
const DEAD_TASK_SENTINEL: usize = usize::MAX - 1;

type FutureObj<'ring> = Pin<Box<dyn Future<Output = ()> + 'ring>>;

enum CleanupAction {
  None,
  DeleteBufferOnError(usize),
}

struct CompletionEvent {
  cqe: Option<CompletionQueueEvent>,
  task: usize,
  /// Action performed when this event must be cleaned up by the executor
  /// and the taskid is DEAD_TASK_SENTINEL
  action: CleanupAction,
}

impl CompletionEvent {
  pub fn new(task: usize) -> Self {
    Self {
      task,
      cqe: None,
      action: CleanupAction::None,
    }
  }
}
