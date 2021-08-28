/// Round x up to the next multiple of d
pub(crate) const fn round_up(x: usize, d: usize) -> usize {
  x + (d - x % d) % d
}
