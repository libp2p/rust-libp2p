// Copyright 2018 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use multihash::Multihash;

pub enum RecordStorageError {
    AtCapacity,
    ValueTooLarge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    key: Multihash,
    value: Vec<u8>,
}

impl Record {
    pub fn new(key: Multihash, value: Vec<u8>) -> Self { Record { key, value } }
    pub fn key(&self) -> &Multihash { &self.key }
    pub fn value(&self) -> &Vec<u8> { &self.value }
}

pub trait RecordStore {
    fn get(&self, k: &Multihash) -> Option<&Record>;
    fn put(&mut self, k: Multihash, r: Record) -> Result<(), RecordStorageError>;
}
