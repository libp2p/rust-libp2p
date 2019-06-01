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

use fnv::FnvHashMap;
use multihash::Multihash;
use std::borrow::{Cow, ToOwned};

#[derive(Debug)]
pub enum RecordStorageError {
    AtCapacity,
    ValueTooLarge,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Record {
    pub key: Multihash,
    pub value: Vec<u8>,
}

impl Record {
    pub fn new(key: Multihash, value: Vec<u8>) -> Self { Record { key, value } }
}

pub trait RecordStore {
    fn get(&self, k: &Multihash) -> Option<Cow<Record>>;
    fn put(&mut self, r: Record) -> Result<(), RecordStorageError>;
}

pub struct MemoryRecordStorage{
    max_records: usize,
    max_record_size: usize,
    records: FnvHashMap<Multihash, Record>
}

impl MemoryRecordStorage {
    const MAX_RECORDS: usize = 1024;
    const MAX_RECORD_SIZE: usize = 65535;

    pub fn new(max_records: usize, max_record_size: usize) -> Self {
        MemoryRecordStorage{
            max_records,
            max_record_size,
            records: FnvHashMap::default()
        }
    }
}

impl Default for MemoryRecordStorage {
    fn default() -> Self {
        MemoryRecordStorage::new(Self::MAX_RECORDS, Self::MAX_RECORD_SIZE)
    }
}

impl RecordStore for MemoryRecordStorage {
    fn get(&self, k: &Multihash) -> Option<Cow<Record>> {
        match self.records.get(k) {
            Some(rec) => Some(Cow::Owned(rec.to_owned())),
            None => None,
        }
    }

    fn put(&mut self, r: Record) -> Result<(), RecordStorageError> {
        if self.records.len() >= self.max_records {
            return Err(RecordStorageError::AtCapacity);
        }

        if r.value.len() >= self.max_record_size {
            return Err(RecordStorageError::ValueTooLarge)
        }

        self.records.insert(r.key.clone(), r);

        Ok(())
    }
}
