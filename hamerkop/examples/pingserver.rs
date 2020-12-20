#![feature(new_uninit, maybe_uninit_extra)]

#[macro_use]
extern crate log;

use std::{
  future::Future,
  io::{self, Write},
  mem::MaybeUninit,
  net::{TcpListener, TcpStream, ToSocketAddrs},
  os::unix::prelude::{AsRawFd, RawFd},
  pin::Pin,
  sync::mpsc::{channel, Receiver, Sender},
  task::{Context, Poll},
};

use hamerkop::{uring::IoUring, FixedVec, IOHandle, Runtime};

const STARTING_BUFFERS: usize = 8; //128;
const BUFFER_SIZE: usize = 1 << 14;

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
  wbuf: FixedVec<u8>,
  tbuf: FixedVec<u8>,
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
      wbuf: FixedVec::with_capacity(BUFFER_SIZE),
      tbuf: FixedVec::with_capacity(BUFFER_SIZE),
    };

    info!("Accepted new connection from {}", addr);

    let bytes = 1u64.to_ne_bytes();
    let ret = unsafe { libc::write(fd, &bytes as *const _ as _, bytes.len() as _) };
    if ret < 0 {
      return Err(io::Error::from_raw_os_error(-ret as i32));
    }

    if let Err(_) = tx.send(setup) {
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

    info!("Got conn notification");

    for conn in channel.try_iter() {
      info!("Got new connection");
      handle.spawn_dyn(func(conn, handle.clone()));
    }
  }
}

async fn conn_task_<'ring>(
  handle: IOHandle<'ring>,
  conn: TcpStream,
  mut wbuf: FixedVec<u8>,
  mut tbuf: FixedVec<u8>,
) -> io::Result<()> {
  info!("listening on {}", conn.as_raw_fd());

  while let Ok(mut rbuf) = handle.read(conn.as_raw_fd(), 6).await {
    info!("read {} bytes", rbuf.len());

    if rbuf.len() == 0 {
      handle.provide_buffer(rbuf).await?;
      break;
    }

    tbuf.append(&mut rbuf);

    let mut count = 0;
    for chunk in tbuf.chunks_mut(6) {
      if chunk == b"PING\r\n" {
        wbuf.write_all(b"PONG\r\n")?;
      } else {
        return Ok(());
      }
      count += 4;
    }

    tbuf.drain(..count);
    tbuf.append(&mut rbuf);

    while !wbuf.is_empty() {
      let amount = unsafe { handle.write_buf(conn.as_raw_fd(), &wbuf).await? };
      wbuf.drain(..amount);
    }

    handle.provide_buffer(rbuf).await?;
  }

  Ok(())
}

async fn conn_task<'ring>(
  handle: IOHandle<'ring>,
  conn: TcpStream,
  wbuf: FixedVec<u8>,
  tbuf: FixedVec<u8>,
) {
  if let Err(e) = conn_task_(handle, conn, wbuf, tbuf).await {
    warn!("Connection task exited with error {}", e);
  };
}

fn main() -> io::Result<()> {
  if std::env::var_os("PINGSERVER_LOG").is_none() {
    std::env::set_var("PINGSERVER_LOG", "trace");
  }
  std::env::set_var("RUST_BACKTRACE", "1");

  env_logger::init_from_env("PINGSERVER_LOG");

  let mut ring = IoUring::new(512).expect("Unable to allocate uring");

  let (sq, cq, _) = ring.split();

  let (handle, mut rt) = Runtime::new(sq, cq, BUFFER_SIZE);
  let (tx, rx) = channel();

  let fd = unsafe { libc::eventfd(0, libc::FD_CLOEXEC) };
  if fd < 0 {
    return Err(io::Error::from_raw_os_error(-fd));
  }

  info!("Listening on {}", "0.0.0.0:8000");

  let res = crossbeam::scope(|s| -> io::Result<()> {
    s.spawn(move |_| {
      let res = acceptor_thread("0.0.0.0:8000", tx, fd);

      if let Err(e) = res {
        error!("Acceptor thread finished with error: {}", e);
      }
    });

    let res = rt.block_on(main_task(
      handle.clone(),
      fd,
      |mut data: ConnSetup<_>, handle| {
        data
          .mem
          .inner
          .write(conn_task(handle, data.conn, data.wbuf, data.tbuf));
        unsafe { std::mem::transmute(data.mem) }
      },
      rx,
    ));

    unsafe { libc::close(fd) };

    match res {
      Ok(r) => r,
      Err(e) => {
        error!("Runtime exited with error {}", e);
        Ok(())
      }
    }
  });

  if let Err(e) = res.unwrap() {
    error!("Main task exited with error {}", e);
  }

  Ok(())
}

pub struct Join<F> {
  futures: Vec<F>,
  state: Vec<bool>,
}

pub struct JoinNext<'j, F> {
  join: &'j mut Join<F>
}

impl<F: Future> Join<F> {
  pub fn new(futures: Vec<F>) -> Self {
    Self {
      state: vec![false; futures.len()],
      futures
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

    let iter = join.futures.iter_mut().zip(join.state.iter_mut()).filter(|(_, state)| !**state);
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
