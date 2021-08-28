use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct Join<F> {
  futures: Vec<F>,
  state: Vec<bool>,
}

pub struct JoinNext<'j, F> {
  join: &'j mut Join<F>,
}

impl<F: Future> Join<F> {
  pub fn new(futures: Vec<F>) -> Self {
    Self {
      state: vec![false; futures.len()],
      futures,
    }
  }

  pub fn next(&mut self) -> JoinNext<F> {
    JoinNext { join: self }
  }
}

impl<F: Future> Future for JoinNext<'_, F> {
  type Output = Option<F::Output>;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let join = unsafe { &mut *self.get_unchecked_mut().join };

    let iter = join
      .futures
      .iter_mut()
      .zip(join.state.iter_mut())
      .filter(|(_, state)| !**state);
    let mut polled = false;
    for (fut, state) in iter {
      let fut = unsafe { Pin::new_unchecked(fut) };
      polled = true;

      if let Poll::Ready(res) = fut.poll(cx) {
        *state = true;

        return Poll::Ready(Some(res));
      }
    }

    if polled {
      Poll::Pending
    } else {
      Poll::Ready(None)
    }
  }
}
