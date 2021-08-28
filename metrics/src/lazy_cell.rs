use std::any::Any;
use std::cell::UnsafeCell;
use std::mem::{self, ManuallyDrop, MaybeUninit};
use std::ops::{Deref, DerefMut};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU8, Ordering};

use crate::{Metric, Value};

const UNINIT: u8 = 0;
const INIT_IN_PROGRESS: u8 = 1;
const INIT: u8 = 2;
const POISONED: u8 = 3;

pub struct LazyCell<T, F = fn() -> T> {
  func: ManuallyDrop<F>,
  value: UnsafeCell<MaybeUninit<T>>,
  state: AtomicU8,
}

impl<T, F> LazyCell<T, F> {
  pub const fn new(func: F) -> Self {
    Self {
      value: UnsafeCell::new(MaybeUninit::uninit()),
      state: AtomicU8::new(UNINIT),
      func: ManuallyDrop::new(func),
    }
  }

  /// Get a reference to the inner value.
  ///
  /// # Safety
  /// It must be that `state == INIT` (and therefore self.value is initialized).
  unsafe fn inner(&self) -> &T {
    &*(*self.value.get()).as_ptr()
  }

  /// Get a mutable reference to the inner value.
  ///
  /// # Safety
  /// It must be that `state == INIT` (and therefore self.value is initialized).
  unsafe fn inner_mut(&mut self) -> &mut T {
    &mut *self.value.get_mut().as_mut_ptr()
  }
}

impl<T, F> LazyCell<T, F>
where
  F: FnOnce() -> T,
{
  pub fn get(this: &Self) -> Option<&T> {
    let state = this.state.load(Ordering::Acquire);

    match state {
      INIT => unsafe { Some(this.inner()) },
      _ => None,
    }
  }

  pub fn get_mut(this: &mut Self) -> Option<&mut T> {
    match *this.state.get_mut() {
      INIT => unsafe { Some(this.inner_mut()) },
      _ => None,
    }
  }

  pub fn force(this: &Self) -> &T {
    let state = this.state.load(Ordering::Acquire);

    match state {
      INIT => unsafe { this.inner() },
      POISONED => Self::panic_poisoned(),
      INIT_IN_PROGRESS | UNINIT => Self::init(this),
      _ => unreachable!(),
    }
  }

  pub fn force_mut(this: &mut Self) -> &mut T {
    let state = this.state.get_mut();

    match *state {
      INIT => unsafe { this.inner_mut() },
      UNINIT => Self::init_mut(this),
      POISONED => Self::panic_poisoned(),
      _ => unreachable!(),
    }
  }

  pub fn into_inner(mut this: Self) -> Option<T> {
    let state = this.state.get_mut();

    match *state {
      INIT => {
        // Need to avoid dropping the value since it's no longer live.
        *state = POISONED;
        unsafe { Some(mem::replace(this.value.get_mut(), MaybeUninit::uninit()).assume_init()) }
      }
      _ => None,
    }
  }

  #[cold]
  #[inline(never)]
  fn init(&self) -> &T {
    let res = self.state.compare_exchange(
      UNINIT,
      INIT_IN_PROGRESS,
      Ordering::Acquire,
      Ordering::Acquire,
    );

    match res {
      Ok(_) => {
        // SAFETY: Since we've transitioned out of UNINIT self.func will
        //         never be dropped. Furthermore, since we're the one who
        //         did the transition we're responsible for dropping func.
        //         So it's safe to move func out of self.func.
        let func = unsafe { std::ptr::read(&*self.func) };

        let value = match catch_unwind(AssertUnwindSafe(move || func())) {
          Ok(value) => value,
          Err(e) => {
            self.state.store(POISONED, Ordering::Release);
            std::panic::resume_unwind(e);
          }
        };

        // SAFETY: While the current state is INIT_IN_PROGRESS no other task
        //         can enter within the LazyCell. So we have exclusive access
        //         to self.value. Furthermore, the following store to state has
        //         release ordering and all loads of state have acquire ordering
        //         so all other threads will see this write before reading from
        //         self.value.
        unsafe { *self.value.get() = MaybeUninit::new(value) };
        self.state.store(INIT, Ordering::Release);

        unsafe { self.inner() }
      }
      Err(INIT) => return unsafe { self.inner() },
      Err(INIT_IN_PROGRESS) => loop {
        match self.state.load(Ordering::Acquire) {
          INIT_IN_PROGRESS => (),
          INIT => break unsafe { self.inner() },
          POISONED => Self::panic_poisoned(),
          _ => unreachable!(),
        }
      },
      Err(POISONED) => Self::panic_poisoned(),
      Err(UNINIT) => unreachable!("LazyCell went back to uninit"),
      Err(_) => unreachable!(),
    }
  }

  #[cold]
  #[inline(never)]
  fn init_mut(&mut self) -> &mut T {
    let state = self.state.get_mut();

    assert_eq!(*state, UNINIT);

    // SAFETY: The current state is UNINIT and we have a mutable reference
    //         so the function is still contained within self.func and we
    //         can take it and nobody else will look at it until after the
    //         current method call completes.
    let func = unsafe { ManuallyDrop::take(&mut self.func) };

    let value = match catch_unwind(AssertUnwindSafe(move || func())) {
      Ok(value) => value,
      Err(e) => {
        *state = POISONED;
        std::panic::resume_unwind(e);
      }
    };

    *self.value.get_mut() = MaybeUninit::new(value);
    *state = INIT;

    // SAFETY: We're now in the INIT state since we just set it.
    unsafe { self.inner_mut() }
  }

  #[cold]
  #[inline(never)]
  fn panic_poisoned() -> ! {
    panic!("LazyCell is poisoned!")
  }
}

impl<T, F> Deref for LazyCell<T, F>
where
  F: FnOnce() -> T,
{
  type Target = T;

  fn deref(&self) -> &Self::Target {
    Self::force(self)
  }
}

impl<T, F> DerefMut for LazyCell<T, F>
where
  F: FnOnce() -> T,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    Self::force_mut(self)
  }
}

unsafe impl<T: Send, F: Send> Send for LazyCell<T, F> {}
unsafe impl<T: Sync, F: Sync> Sync for LazyCell<T, F> {}

impl<T, F> Drop for LazyCell<T, F> {
  fn drop(&mut self) {
    match *self.state.get_mut() {
      UNINIT => unsafe { ManuallyDrop::drop(&mut self.func) },
      INIT => unsafe { std::ptr::drop_in_place(self.inner_mut()) },
      _ => (),
    }
  }
}

impl<T, F> Metric for LazyCell<T, F>
where
  T: Metric,
  F: Sync + FnOnce() -> T,
{
  fn is_enabled(&self) -> bool {
    Self::get(self).map(Metric::is_enabled).unwrap_or(false)
  }

  fn value(&self) -> Option<Value> {
    Self::get(self).and_then(Metric::value)
  }

  fn as_any(&self) -> Option<&dyn Any> {
    Self::get(self).and_then(Metric::as_any)
  }
}
