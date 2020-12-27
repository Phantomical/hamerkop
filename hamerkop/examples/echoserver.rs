#![feature(new_uninit, maybe_uninit_extra)]

#[macro_use]
extern crate log;

use std::{
  future::Future,
  io,
  mem::MaybeUninit,
  net::{TcpListener, TcpStream, ToSocketAddrs},
  os::unix::prelude::{AsRawFd, RawFd},
  pin::Pin,
  sync::mpsc::{channel, Receiver, Sender},
};

use hamerkop::{
  uring::{IoUring, SetupFlags},
  util::Join,
  FixedVec, IOHandle, Runtime,
};

const STARTING_BUFFERS: usize = 256;
const BUFFER_SIZE: usize = 1 << 16;

struct SizedBuffer<T> {
  inner: MaybeUninit<T>,
}

impl<T> SizedBuffer<T> {
  pub fn boxed() -> Box<Self> {
    unsafe { Box::<Self>::new_uninit().assume_init() }
  }
}

unsafe impl<T> Send for SizedBuffer<T> {}
unsafe impl<T> Sync for SizedBuffer<T> {}

struct ConnSetup<F> {
  mem: Box<SizedBuffer<F>>,
  conn: TcpStream,
}

fn acceptor_thread<F, A: ToSocketAddrs>(
  addr: A,
  tx: Sender<ConnSetup<F>>,
  fd: RawFd,
) -> io::Result<()> {
  let listener = TcpListener::bind(addr)?;

  loop {
    let (conn, addr) = listener.accept()?;

    let setup = ConnSetup {
      mem: SizedBuffer::boxed(),
      conn,
    };

    info!("Accepted new connection from {}", addr);

    let bytes = 1u64.to_ne_bytes();
    let ret = unsafe { libc::write(fd, &bytes as *const _ as _, bytes.len() as _) };
    if ret < 0 {
      return Err(io::Error::from_raw_os_error(-ret as i32));
    }

    if let Err(e) = tx.send(setup) {
      warn!("Got send error {}", e);
      break;
    }
  }

  Ok(())
}

async fn main_task<
  'ring,
  F: Future<Output = ()> + 'ring,
  B: Fn(ConnSetup<F>, IOHandle<'ring>) -> Pin<Box<F>>,
>(
  handle: IOHandle<'ring>,
  fd: RawFd,
  func: B,
  channel: Receiver<ConnSetup<F>>,
) -> io::Result<()> {
  {
    let mut futures = Vec::with_capacity(STARTING_BUFFERS);
    for _ in 0..STARTING_BUFFERS {
      futures.push(handle.provide_buffer(FixedVec::with_capacity(BUFFER_SIZE)));
    }

    let mut join = Join::new(futures);
    while let Some(res) = join.next().await {
      res?;
    }
  }

  info!("Registered {} buffers!", STARTING_BUFFERS);

  let mut buf = 0u64.to_ne_bytes();

  loop {
    unsafe { handle.read_buf(fd, &mut buf).await? };

    // info!("Got conn notification");

    for conn in channel.try_iter() {
      // info!("Got new connection");
      handle.spawn_dyn(func(conn, handle.clone()));
    }
  }
}

async fn conn_task_<'ring>(mut handle: IOHandle<'ring>, conn: TcpStream) -> io::Result<()> {
  info!("listening on {}", conn.as_raw_fd());

  let mut rbuf = handle.read(conn.as_raw_fd(), BUFFER_SIZE).await?;

  loop {
    if rbuf.len() == 0 {
      handle.provide_buffer(rbuf).await?;
      break;
    }

    let len = rbuf.len();
    let mut submitter = handle.submit_linked();

    let wfut = unsafe {
      // SAFETY: The buffer will not be reused until provide_buffer is completed
      //         which can only happen after the write future completes (due to
      //         linked submission)
      let slice = std::slice::from_raw_parts(rbuf.as_ptr(), rbuf.len());
      submitter.write_buf(conn.as_raw_fd(), slice)
    };
    let pfut = submitter.provide_buffer(rbuf);
    let rfut = submitter.read(conn.as_raw_fd(), BUFFER_SIZE);

    submitter.submit();

    let amount = wfut.await?;
    if amount < len {
      rbuf = pfut
        .await
        .expect_err("Linked SQE not cancelled?")
        .into_buffer();
      let _ = rfut.await;

      write_all(&handle, conn.as_raw_fd(), &rbuf[amount..]).await?;

      handle.provide_buffer(rbuf).await?;
      rbuf = handle.read(conn.as_raw_fd(), BUFFER_SIZE).await?;
    } else {
      rbuf = rfut.await?;
    }
  }

  Ok(())
}

async fn write_all(handle: &IOHandle<'_>, fd: RawFd, mut slice: &[u8]) -> io::Result<()> {
  while !slice.is_empty() {
    let count = unsafe { handle.write_buf(fd, slice).await? };
    slice = &slice[count..];

    if count == 0 {
      return Err(io::Error::new(
        io::ErrorKind::WriteZero,
        "unable to write entire buffer",
      ));
    }
  }

  Ok(())
}

async fn conn_task<'ring>(handle: IOHandle<'ring>, conn: TcpStream) {
  let fd = conn.as_raw_fd();
  if let Err(e) = conn_task_(handle, conn).await {
    warn!("Connection task exited with error {}", e);
  } else {
    info!("Closed connection {}", fd);
  }
}

fn main() -> io::Result<()> {
  if std::env::var_os("PINGSERVER_LOG").is_none() {
    std::env::set_var("PINGSERVER_LOG", "info");
  }
  std::env::set_var("RUST_BACKTRACE", "1");

  env_logger::init_from_env("PINGSERVER_LOG");

  let mut ring = IoUring::with_flags(512, SetupFlags::empty()).expect("Unable to allocate uring");

  let (sq, cq, _) = ring.split();

  let (handle, mut rt) = Runtime::new(sq, cq, BUFFER_SIZE);
  let (tx, rx) = channel();

  let fd = unsafe { libc::eventfd(0, libc::FD_CLOEXEC) };
  if fd < 0 {
    return Err(io::Error::from_raw_os_error(-fd));
  }

  info!("Listening on {}", "0.0.0.0:8000");

  let _ = crossbeam::scope(|s| -> io::Result<()> {
    s.spawn(move |_| {
      let res = acceptor_thread("0.0.0.0:8000", tx, fd);

      if let Err(e) = res {
        error!("Acceptor thread finished with error: {}", e);
      } else {
        info!("Acceptor thread terminated");
      }
    });

    let res = rt.block_on(async {
      let res = main_task(
        handle.clone(),
        fd,
        |mut data: ConnSetup<_>, handle| {
          data.mem.inner.write(conn_task(handle, data.conn));
          unsafe { std::mem::transmute(data.mem) }
        },
        rx,
      )
      .await;

      match res {
        Ok(_) => warn!("Main task completed"),
        Err(e) => error!("Main task exited with error {}", e),
      }
    });

    unsafe { libc::close(fd) };

    if let Err(e) = res {
      error!("Runtime exited with error {}", e);
    }

    Ok(())
  });

  Ok(())
}
