macro_rules! to_buf {
  ($e: expr, $i: ident) => {
      for b in $e.to_be_bytes() {
          $i.push(b);
      }
  };
}
pub(crate) use to_buf;

macro_rules! vec_to_buf {
  ($e: expr, $i: ident) => {
      for b in $e {
          $i.push(*b);
      }
  };
}
pub(crate) use vec_to_buf;
