fn main() {
  cc::Build::new()
    .file("src/syscall.c")
    .compile("uring-syscall");
}
