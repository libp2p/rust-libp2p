pub use quickcheck::*;

use core::ops::Range;
use num_traits::sign::Unsigned;

pub trait GenRange {
    fn gen_range<T: Unsigned + Arbitrary + Copy>(&mut self, _range: Range<T>) -> T;
}

impl GenRange for Gen {
    fn gen_range<T: Unsigned + Arbitrary + Copy>(&mut self, range: Range<T>) -> T {
        <T as Arbitrary>::arbitrary(self) % (range.end - range.start) + range.start
    }
}
