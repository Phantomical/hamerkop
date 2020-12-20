use std::io;
use uring::sqes::Nop;
use uring::IoUring;

#[test]
fn noop_test() -> io::Result<()> {
  // confirm that setup and mmap work
  let mut io_uring = IoUring::new(32)?;

  // confirm that submit and enter work
  {
    let mut sq = io_uring.sq();
    let sqe = sq.next_sqe().unwrap();
    sqe.prepare(Nop::new());
    sqe.set_user_data(0xDEADBEEF);
  }
  unsafe { io_uring.sq().submit()? };

  // confirm that cq reading works
  {
    let mut cq = io_uring.cq();
    cq.wait()?;
    let cqe = cq.available()[0];
    assert_eq!(cqe.user_data(), 0xDEADBEEF);
  }

  Ok(())
}

#[test]
fn multi_noop() -> io::Result<()> {
  let mut io_uring = IoUring::new(32)?;
  let (mut sq, mut cq, _) = io_uring.split();

  // assert_eq!(sq.sqes().len(), sq.free_sqes());
  let mut sqes = sq.sqes();

  let sqe = sqes.get_mut(0).unwrap();
  sqe.prepare(Nop::new());
  sqe.set_user_data(0xBAADBEEF);
  let sqe = sqes.get_mut(1).unwrap();
  sqe.prepare(Nop::new());
  sqe.set_user_data(0xDEADBEEF);

  sq.advance(2);
  unsafe { sq.submit_and_wait(2)? };

  let available = cq.available();

  assert_eq!(available.len(), 2);
  assert_eq!(available[0].user_data(), 0xBAADBEEF);
  assert_eq!(available[1].user_data(), 0xDEADBEEF);

  Ok(())
}
