use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;
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
fn write_test() -> io::Result<()> {
  let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  path.push("props");
  path.push("text.tmp");

  let _ = fs::remove_file(&path);
  let n = {
    let mut io_uring = IoUring::new(32)?;

    let file = File::create(&path)?;
    unsafe {
      let mut sq = io_uring.sq();
      let sqe = sq.sqes().get_mut(0).unwrap();
      let bufs = [io::IoSlice::new(TEXT)];
      sqe.prepare(WriteVectored::new(
        Target::Fd(file.as_raw_fd()),
        bufs.as_ptr() as _,
        bufs.len() as _,
      ));
      sqe.set_user_data(0xDEADBEEF);
      sq.advance(1);
      sq.submit()?;
    }

    let mut cq = io_uring.cq();
    cq.wait()?;
    let cqe = cq.available()[0];
    assert_eq!(cqe.user_data(), 0xDEADBEEF);
    cqe.result()? as usize
  };

  let mut file = File::open(&path)?;
  let mut buf = vec![];
  file.read_to_end(&mut buf)?;
  assert_eq!(&TEXT[..n], &buf[..n]);
  let _ = fs::remove_file(&path);

  Ok(())
}
