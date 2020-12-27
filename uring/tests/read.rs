#![feature(test)]
extern crate test;

use std::fs::File;
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::PathBuf;

use uring::{sqes::*, IoUring};

const TEXT: &[u8] = b"I really wanna stop
But I just gotta taste for it
I feel like I could fly with the ball on the moon
So honey hold my hand you like making me wait for it
I feel like I could die walking up to the room, oh yeah

Late night watching television
But how we get in this position?
It's way too soon, I know this isn't love
But I need to tell you something

I really really really really really really like you";

#[test]
fn read_test() -> io::Result<()> {
  let mut io_uring = IoUring::new(32)?;

  let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  path.push("props");
  path.push("text.txt");
  let file = File::open(&path)?;
  let mut buf1 = [0; 4096];
  unsafe {
    prep(&mut io_uring, &mut buf1, file.as_raw_fd())?;
  }

  let dirt = dirty_stack();

  let n = {
    let mut cq = io_uring.cq();
    cq.wait()?;
    let cqe = cq.available()[0];
    assert_eq!(cqe.user_data(), 0xDEADBEEF);
    cqe.result()? as usize
  };

  assert_eq!(&TEXT[..n], &buf1[..n]);
  drop(dirt);

  Ok(())
}

#[inline(never)]
unsafe fn prep(ring: &mut IoUring, buf: &mut [u8], fd: RawFd) -> io::Result<()> {
  let mut sq = ring.sq();
  let sqe = sq.sqes().get_mut(0).unwrap();
  let mut bufs = [io::IoSliceMut::new(buf)];
  sqe.prepare(ReadVectored::new(
    Target::Fd(fd),
    bufs.as_mut_ptr() as *mut _,
    bufs.len() as _,
  ));
  sqe.set_user_data(0xDEADBEEF);
  sq.advance(1);
  sq.submit()?;
  Ok(())
}

#[inline(never)]
fn dirty_stack() -> [u8; 4096] {
  test::black_box([0; 4096])
}
