pub use quickcheck::*;

use core::ops::{Add, Range, Rem, Sub};

pub trait GenRange {
    fn gen_range<T>(&mut self, _range: Range<T>) -> T
    where
        T: Arbitrary + Copy + Add<Output = T> + Rem<Output = T> + Sub<Output = T>;
}

impl GenRange for Gen {
    fn gen_range<T>(&mut self, range: Range<T>) -> T
    where
        T: Arbitrary + Copy + Add<Output = T> + Rem<Output = T> + Sub<Output = T>,
    {
        <T as Arbitrary>::arbitrary(self) % (range.end - range.start) + range.start
    }
}
