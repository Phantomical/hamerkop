use cooked_waker::{ViaRawPointer, Wake, WakeRef};

#[derive(Clone)]
pub(super) struct EmptyWaker;

impl WakeRef for EmptyWaker {
  fn wake_by_ref(&self) {
    // TODO: Implement
  }
}
impl Wake for EmptyWaker {
  fn wake(self) {
    // TODO: Implement
  }
}

unsafe impl ViaRawPointer for EmptyWaker {
  type Target = ();

  fn into_raw(self) -> *mut Self::Target {
    std::ptr::null_mut()
  }

  unsafe fn from_raw(_: *mut Self::Target) -> Self {
    Self
  }
}
