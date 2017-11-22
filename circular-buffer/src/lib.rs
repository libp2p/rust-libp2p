#![warn(missing_docs)]

//! # `circular-buffer`
//!
//! An optimized FIFO queue that allows safe access to the internal storage as a slice (i.e. not
//! just element-by-element). This is useful for circular buffers of bytes. Since it uses
//! `smallvec`'s `Array` trait it can only be backed by an array of static size, this may change in
//! the future.

extern crate smallvec;

use std::ops::Drop;
use std::mem::ManuallyDrop;

use smallvec::Array;

use owned_slice::OwnedSlice;

/// A slice that owns its elements, but not their storage. This is useful for things like
/// `Vec::retain` and `CircularBuffer::pop_slice`, since these functions can return a slice but the
/// elements of these slices would be leaked after the slice goes out of scope. `OwnedSlice` simply
/// manually drops all its elements when it goes out of scope.
pub mod owned_slice {
    use std::ops::{Deref, DerefMut, Drop};
    use std::mem::ManuallyDrop;

    /// A slice that owns its elements, but not their storage. This is useful for things like
    /// `Vec::retain` and `CircularBuffer::pop_slice`, since these functions can return a slice but
    /// the elements of these slices would be leaked after the slice goes out of scope. `OwnedSlice`
    /// simply manually drops all its elements when it goes out of scope.
    #[derive(Debug, Eq, PartialEq)]
    pub struct OwnedSlice<'a, T: 'a>(&'a mut [T]);

    /// Owning iterator for `OwnedSlice`.
    pub struct IntoIter<'a, T: 'a> {
        slice: ManuallyDrop<OwnedSlice<'a, T>>,
        index: usize,
    }

    impl<'a, T> Iterator for IntoIter<'a, T> {
        type Item = T;

        fn next(&mut self) -> Option<Self::Item> {
            use std::ptr;

            let index = self.index;

            if index >= self.slice.len() {
                return None;
            }

            self.index += 1;

            unsafe { Some(ptr::read(&self.slice[index])) }
        }
    }

    impl<'a, T: 'a> IntoIterator for OwnedSlice<'a, T> {
        type Item = T;
        type IntoIter = IntoIter<'a, T>;

        fn into_iter(self) -> Self::IntoIter {
            IntoIter {
                slice: ManuallyDrop::new(self),
                index: 0,
            }
        }
    }

    impl<'a, T: 'a> OwnedSlice<'a, T> {
        /// Construct an owned slice from a mutable slice pointer.
        ///
        /// # Unsafety
        /// You must ensure that the memory pointed to by `inner` will not be accessible after the
        /// lifetime of the `OwnedSlice`.
        pub unsafe fn new(inner: &'a mut [T]) -> Self {
            OwnedSlice(inner)
        }
    }

    impl<'a, T> AsRef<[T]> for OwnedSlice<'a, T> {
        fn as_ref(&self) -> &[T] {
            self.0
        }
    }

    impl<'a, T> AsMut<[T]> for OwnedSlice<'a, T> {
        fn as_mut(&mut self) -> &mut [T] {
            self.0
        }
    }

    impl<'a, T> Deref for OwnedSlice<'a, T> {
        type Target = [T];

        fn deref(&self) -> &Self::Target {
            self.0
        }
    }

    impl<'a, T> DerefMut for OwnedSlice<'a, T> {
        fn deref_mut(&mut self) -> &mut Self::Target {
            self.0
        }
    }

    impl<'a, T> Drop for OwnedSlice<'a, T> {
        fn drop(&mut self) {
            use std::ptr;

            for element in self.iter_mut() {
                unsafe {
                    ptr::drop_in_place(element);
                }
            }
        }
    }
}

/// A fixed-size FIFO queue with safe access to the backing storage.
///
/// This type allows access to slices of the backing storage, for efficient, safe circular buffers
/// of bytes or other `Copy` types.
#[derive(Debug)]
pub struct CircularBuffer<B: Array> {
    // This must be manually dropped, as some or all of the elements may be uninitialized
    buffer: ManuallyDrop<B>,
    start: usize,
    len: usize,
}

impl<B: Array> Default for CircularBuffer<B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Array> PartialEq for CircularBuffer<B>
where
    B::Item: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }

        for (a, b) in self.iter().zip(other.iter()) {
            if a != b {
                return false;
            }
        }

        true
    }
}

impl<B: Array> CircularBuffer<B> {
    /// Create an empty `CircularBuffer`.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer = CircularBuffer::<[usize; 4]>::new();
    ///
    /// assert!(buffer.is_empty());
    /// ```
    pub fn new() -> Self {
        use std::mem;

        CircularBuffer {
            buffer: unsafe { mem::uninitialized() },
            start: 0,
            len: 0,
        }
    }

    /// Pop a slice containing the maximum possible contiguous number of elements. Since this buffer
    /// is circular it will take a maximum of two calls to this function to drain the buffer
    /// entirely.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer = CircularBuffer::from_array([1, 2, 3, 4]);
    ///
    /// assert_eq!(buffer.pop(), Some(1));
    /// assert!(buffer.push(1).is_none());
    ///
    /// assert_eq!(
    ///     buffer.pop_slice().as_ref().map(|s| &s[..]),
    ///     Some(&[2, 3, 4][..])
    /// );
    /// assert_eq!(buffer.pop_slice().as_ref().map(|s| &s[..]), Some(&[1][..]));
    /// assert!(buffer.pop_slice().is_none());
    ///
    /// assert_eq!(buffer.len(), 0);
    /// ```
    ///
    /// This returns an `OwnedSlice`, which owns the items but not the storage (you cannot have two
    /// slices returned from `pop_slice` alive at once, but the elements will be have `drop` called
    /// when the slice goes out of scope), if you're using non-`Drop` types you can use
    /// `pop_slice_leaky`.
    pub fn pop_slice(&mut self) -> Option<OwnedSlice<B::Item>> {
        self.pop_slice_leaky().map(
            |x| unsafe { OwnedSlice::new(x) },
        )
    }

    /// Pop a slice containing the maximum possible contiguous number of elements. Since this buffer
    /// is circular it will take a maximum of two calls to this function to drain the buffer
    /// entirely. This returns a slice and so any `Drop` types returned from this function will be
    /// leaked.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer = CircularBuffer::from_array([1, 2, 3, 4]);
    ///
    /// assert_eq!(buffer.pop(), Some(1));
    /// assert!(buffer.push(1).is_none());
    ///
    /// assert_eq!(
    ///     buffer.pop_slice_leaky(),
    ///     Some(&mut [2, 3, 4][..])
    /// );
    /// assert_eq!(buffer.pop_slice_leaky(), Some(&mut [1][..]));
    /// assert!(buffer.pop_slice_leaky().is_none());
    ///
    /// assert_eq!(buffer.len(), 0);
    /// ```
    pub fn pop_slice_leaky(&mut self) -> Option<&mut [B::Item]> {
        use std::slice;

        if self.is_empty() {
            None
        } else {
            let (start, out_length) = (self.start, self.len.min(B::size() - self.start));

            self.advance(out_length);

            unsafe {
                Some(slice::from_raw_parts_mut(
                    self.buffer.ptr_mut().offset(start as isize),
                    out_length,
                ))
            }
        }
    }

    /// A borrowed iterator of this buffer's elements
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// assert_eq!(
    ///     CircularBuffer::from_array([1, 2, 3, 4]).iter().cloned().collect::<Vec<_>>(),
    ///     vec![1, 2, 3, 4]
    /// );
    /// ```
    pub fn iter(&self) -> Iter<B> {
        self.into_iter()
    }

    /// Iterate over slices of the buffer (of arbitrary size, but in order).
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer = CircularBuffer::from_array([1, 2, 3, 4]);
    ///
    /// assert_eq!(buffer.pop(), Some(1));
    /// assert!(buffer.push(1).is_none());
    ///
    /// let mut iter = buffer.slices();
    ///
    /// assert_eq!(
    ///     iter.collect::<Vec<_>>(),
    ///     vec![&[2, 3, 4][..], &[1]]
    /// );
    /// ```
    pub fn slices(&self) -> SlicesIter<B> {
        SlicesIter {
            buffer: self,
            start: self.start,
            len: self.len,
        }
    }

    /// Whether the buffer is empty.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer = CircularBuffer::<[usize; 4]>::new();
    ///
    /// assert!(buffer.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the buffer is full (i.e. it is no longer possible to push new elements without
    /// popping old ones first).
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer = CircularBuffer::from_array([1, 2, 3, 4]);
    ///
    /// assert!(buffer.is_full());
    /// ```
    pub fn is_full(&self) -> bool {
        self.len == B::size()
    }

    /// The number of elements in the buffer.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer: CircularBuffer<[usize; 4]> = CircularBuffer::from_slice(&[1, 2]).unwrap();
    ///
    /// assert_eq!(buffer.len(), 2);
    ///
    /// assert!(buffer.push(1).is_none());
    /// assert!(buffer.push(2).is_none());
    ///
    /// assert_eq!(buffer.len(), 4);
    /// ```
    pub fn len(&self) -> usize {
        self.len
    }

    /// The maximum number of elements this buffer can take.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer: CircularBuffer<[usize; 4]> = CircularBuffer::new();
    ///
    /// assert_eq!(buffer.capacity(), 4);
    /// ```
    pub fn capacity(&self) -> usize {
        B::size()
    }

    /// Append a single element to the end of the buffer, returning it if it could not be added.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer: CircularBuffer<[usize; 4]> = CircularBuffer::new();
    ///
    /// assert_eq!(buffer.len(), 0);
    ///
    /// assert!(buffer.push(1).is_none());
    /// assert!(buffer.push(2).is_none());
    /// assert!(buffer.push(3).is_none());
    /// assert!(buffer.push(4).is_none());
    ///
    /// assert!(buffer.push(5).is_some());
    ///
    /// assert_eq!(buffer.len(), 4);
    /// ```
    pub fn push(&mut self, element: B::Item) -> Option<B::Item> {
        use std::ptr;

        debug_assert!(self.len <= B::size());

        if self.is_full() {
            Some(element)
        } else {
            let dest = (self.start + self.len) % B::size();

            unsafe {
                ptr::write(self.buffer.ptr_mut().offset(dest as isize), element);
            }
            self.len += 1;
            None
        }
    }

    /// Remove a single element from the start of the buffer.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer = CircularBuffer::from_array([1, 2, 3, 4]);
    ///
    /// assert_eq!(buffer.pop(), Some(1));
    /// ```
    pub fn pop(&mut self) -> Option<B::Item> {
        use std::ptr;

        if self.is_empty() {
            None
        } else {
            let offset = self.start;
            self.advance(1);

            unsafe { Some(ptr::read(self.buffer.ptr_mut().offset(offset as _))) }
        }
    }

    /// Get a borrow to an element at an index safely (if the index is out of bounds, return
    /// `None`).
    pub fn get(&self, index: usize) -> Option<&B::Item> {
        if index < self.len {
            unsafe { Some(self.get_unchecked(index)) }
        } else {
            None
        }
    }

    /// Get a borrow to an element at an index unsafely (behaviour is undefined if the index is out
    /// of bounds).
    pub unsafe fn get_unchecked(&self, index: usize) -> &B::Item {
        &*self.buffer.ptr().offset(
            ((index + self.start) % B::size()) as isize,
        )
    }

    /// Get a mutable borrow to an element at an index safely (if the index is out of bounds, return
    /// `None`).
    pub fn get_mut(&mut self, index: usize) -> Option<&mut B::Item> {
        if index < self.len {
            unsafe { Some(self.get_unchecked_mut(index)) }
        } else {
            None
        }
    }

    /// Get a mutable borrow to an element at an index unsafely (behaviour is undefined if the index
    /// is out of bounds).
    pub unsafe fn get_unchecked_mut(&mut self, index: usize) -> &mut B::Item {
        &mut *self.buffer.ptr_mut().offset(
            ((index + self.start) % B::size()) as
                isize,
        )
    }

    // This is not unsafe because it can only leak data, not cause uninit to be read.
    pub fn advance(&mut self, by: usize) {
        assert!(by <= self.len);

        self.start = (self.start + by) % B::size();
        self.len -= by;
    }
}

impl<B: Array> std::ops::Index<usize> for CircularBuffer<B> {
    type Output = B::Item;

    fn index(&self, index: usize) -> &Self::Output {
        if let Some(out) = self.get(index) {
            out
        } else {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                self.len,
                index
            );
        }
    }
}

impl<B: Array> std::ops::IndexMut<usize> for CircularBuffer<B> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        // We need to do this because borrowck isn't smart enough to understand enum variants
        let len = self.len;

        if let Some(out) = self.get_mut(index) {
            return out;
        } else {
            panic!(
                "index out of bounds: the len is {} but the index is {}",
                len,
                index
            );
        }
    }
}

impl<B: Array> Drop for CircularBuffer<B> {
    fn drop(&mut self) {
        while self.pop_slice().is_some() {}
    }
}

impl<B: Array> IntoIterator for CircularBuffer<B> {
    type Item = B::Item;
    type IntoIter = IntoIter<B>;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter { buffer: self }
    }
}

impl<'a, B: Array + 'a> IntoIterator for &'a CircularBuffer<B> {
    type Item = &'a B::Item;
    type IntoIter = Iter<'a, B>;

    fn into_iter(self) -> Self::IntoIter {
        Iter {
            buffer: self,
            remaining: self.len(),
        }
    }
}

/// The iteration type returning owned elements of the buffer
pub struct IntoIter<B: Array> {
    buffer: CircularBuffer<B>,
}

impl<B: Array> Iterator for IntoIter<B> {
    type Item = B::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.buffer.pop()
    }
}

/// The iteration type returning borrows to elements of the buffer
pub struct Iter<'a, B: Array + 'a> {
    buffer: &'a CircularBuffer<B>,
    remaining: usize,
}

impl<'a, B: Array + 'a> Iterator for Iter<'a, B> {
    type Item = &'a B::Item;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            None
        } else {
            let remaining = self.remaining;
            self.remaining -= 1;
            self.buffer.get(self.buffer.len() - remaining)
        }
    }
}

/// The iteration type for immutable slices of the circular buffer. See `CircularBuffer::slices`.
pub struct SlicesIter<'a, B: Array + 'a> {
    buffer: &'a CircularBuffer<B>,
    start: usize,
    len: usize,
}

impl<'a, B: Array + 'a> Iterator for SlicesIter<'a, B> {
    type Item = &'a [B::Item];

    fn next(&mut self) -> Option<Self::Item> {
        use std::slice;

        if self.len == 0 {
            None
        } else {
            let (start, out_length) = (self.start, self.len.min(B::size() - self.start));

            self.start = (self.start + out_length) % B::size();
            self.len -= out_length;

            unsafe {
                Some(slice::from_raw_parts(
                    self.buffer.buffer.ptr().offset(start as isize),
                    out_length,
                ))
            }
        }
    }
}

impl<B: Array> CircularBuffer<B>
where
    B::Item: Copy,
{
    /// Create a `CircularBuffer` from a slice of elements, returning `None` if not all the elements
    /// can fit in the buffer.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// assert!(CircularBuffer::<[usize; 5]>::from_slice(&[1, 2, 3, 4, 5, 6]).is_none());
    /// assert!(CircularBuffer::<[usize; 5]>::from_slice(&[1, 2, 3, 4]).is_some());
    /// ```
    pub fn from_slice(slice: &[B::Item]) -> Option<Self> {
        let mut out = Self::new();
        if out.extend_from_slice(slice) {
            Some(out)
        } else {
            None
        }
    }

    /// Create a `CircularBuffer` from a slice of elements, returning the number of elements copied.
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let result = CircularBuffer::<[usize; 5]>::from_slice_prefix(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 20]);
    /// assert_eq!(result, (CircularBuffer::from_array([1, 2, 3, 4, 5]), 5));
    /// ```
    pub fn from_slice_prefix(slice: &[B::Item]) -> (Self, usize) {
        let mut out = Self::new();
        let num_copied = out.extend_from_slice_prefix(slice);
        (out, num_copied)
    }

    /// Create a circular buffer from a fixed-size array
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let result = CircularBuffer::from_array([1, 2, 3, 4, 5]);
    /// assert_eq!(result.into_iter().collect::<Vec<_>>(), vec![1, 2, 3, 4, 5]);
    /// ```
    pub fn from_array(slice: B) -> Self {
        CircularBuffer {
            buffer: ManuallyDrop::new(slice),
            start: 0,
            len: B::size(),
        }
    }

    fn write_slice(&mut self, index: usize, slice: &[B::Item]) -> bool {
        use std::ptr;

        let mut offset = 0;

        assert!(index <= self.len);

        if slice.len() > self.capacity() - index {
            return false;
        }

        while offset < slice.len() {
            unsafe {
                let dest = (index + self.start + offset) % B::size();
                let copy_len = if dest < self.start {
                    self.start - dest
                } else {
                    B::size() - dest
                }.min(slice.len() - offset);

                let slice_ptr = slice.as_ptr().offset(offset as isize);
                let ptr = self.buffer.ptr_mut().offset(dest as isize);

                ptr::copy(slice_ptr, ptr, copy_len);

                self.len = self.len.max(index + offset + copy_len);
                offset += copy_len;
            }
        }

        true
    }

    fn write_slice_prefix(&mut self, index: usize, slice: &[B::Item]) -> usize {
        use std::ptr;

        let mut offset = 0;

        assert!(index <= self.len);

        while !self.is_full() && offset < slice.len() {
            unsafe {
                let dest = (index + self.start + offset) % B::size();
                let copy_len = if dest < self.start {
                    self.start - dest
                } else {
                    B::size() - dest
                }.min(slice.len() - offset);

                let slice_ptr = slice.as_ptr().offset(offset as isize);
                let ptr = self.buffer.ptr_mut().offset(dest as isize);

                ptr::copy(slice_ptr, ptr, copy_len);

                self.len = self.len.max(index + offset + copy_len);
                offset += copy_len;
            }
        }

        offset
    }

    /// Append the elements from a slice to the buffer, returning the number of elements copied
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer = CircularBuffer::<[usize; 5]>::from_slice(&[1, 2]).unwrap();
    ///
    /// assert_eq!(buffer.iter().cloned().collect::<Vec<_>>(), vec![1, 2]);
    ///
    /// let consumed = buffer.extend_from_slice_prefix(&[1, 2, 3, 4, 5]);
    ///
    /// assert_eq!(consumed, 3);
    /// assert_eq!(buffer.iter().cloned().collect::<Vec<_>>(), vec![1, 2, 1, 2, 3]);
    /// ```
    #[inline]
    pub fn extend_from_slice_prefix(&mut self, slice: &[B::Item]) -> usize {
        let len = self.len();
        self.write_slice_prefix(len, slice)
    }

    /// Append the elements from a slice to the buffer iff there is enough space for all the
    /// elements
    ///
    /// ```rust
    /// use circular_buffer::CircularBuffer;
    ///
    /// let mut buffer = CircularBuffer::<[usize; 5]>::from_slice(&[1, 2]).unwrap();
    ///
    /// assert_eq!(buffer.iter().cloned().collect::<Vec<_>>(), vec![1, 2]);
    ///
    /// assert!(!buffer.extend_from_slice(&[1, 2, 3, 4, 5]));
    /// assert!(buffer.extend_from_slice(&[1, 2, 3]));
    ///
    /// assert_eq!(buffer.iter().cloned().collect::<Vec<_>>(), vec![1, 2, 1, 2, 3]);
    /// ```
    #[inline]
    pub fn extend_from_slice(&mut self, slice: &[B::Item]) -> bool {
        let len = self.len();
        self.write_slice(len, slice)
    }
}

#[cfg(test)]
mod tests {
    use super::CircularBuffer;

    #[test]
    fn push_pop() {
        let mut buffer: CircularBuffer<[usize; 4]> = CircularBuffer::new();

        assert_eq!(buffer.len(), 0);

        assert!(buffer.push(1).is_none());
        assert!(buffer.push(2).is_none());
        assert!(buffer.push(3).is_none());
        assert!(buffer.push(4).is_none());

        assert!(buffer.push(5).is_some());

        assert_eq!(buffer.len(), 4);

        assert_eq!(buffer.pop(), Some(1));
        assert_eq!(buffer.pop(), Some(2));
        assert_eq!(buffer.pop(), Some(3));
        assert_eq!(buffer.pop(), Some(4));
        assert_eq!(buffer.pop(), None);

        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn pop_slice() {
        let mut buffer: CircularBuffer<[usize; 4]> = CircularBuffer::new();

        assert_eq!(buffer.len(), 0);

        assert!(buffer.push(1).is_none());
        assert!(buffer.push(2).is_none());
        assert!(buffer.push(3).is_none());
        assert!(buffer.push(4).is_none());

        assert!(buffer.push(5).is_some());

        assert_eq!(buffer.len(), 4);

        assert_eq!(buffer.pop(), Some(1));
        assert!(buffer.push(1).is_none());

        assert_eq!(
            buffer.pop_slice().as_ref().map(|s| &s[..]),
            Some(&[2, 3, 4][..])
        );
        assert_eq!(buffer.pop_slice().as_ref().map(|s| &s[..]), Some(&[1][..]));
        assert!(buffer.pop_slice().is_none());

        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn extend_from_slice() {
        let mut buffer: CircularBuffer<[usize; 4]> = CircularBuffer::from_slice(&[1, 2]).unwrap();

        assert_eq!(buffer.pop(), Some(1));
        assert_eq!(buffer.pop(), Some(2));

        assert!(buffer.extend_from_slice(&[1, 2, 3, 4]));

        assert_eq!(buffer.iter().cloned().collect::<Vec<_>>(), vec![1, 2, 3, 4])
    }
}
