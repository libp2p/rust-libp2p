// Copyright 2017 Parity Technologies (UK) Ltd.
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

//! General-purpose key-value storage.
//! The keys are strings, and the values are of any type you want.
//!
//! > **Note**: This crate is meant to be a utility for the implementation of other crates ; it
//! >            does not directly participate in the stack of libp2p.
//!
//! This crate provides the `Datastore` trait, whose template parameter is the type of the value.
//! It is implemented on types that represent a key-value storage.
//! The only available implementation for now is `JsonFileDatastore`.
//!
//! # JSON file datastore
//!
//! The `JsonFileDatastore` can provide a key-value storage that loads and stores data in a single
//! JSON file. It is only available if the value implements the `Serialize`, `DeserializeOwned`
//! and `Clone` traits.
//!
//! The `JsonFileDatastore::new` method will attempt to load existing data from the path you pass
//! as parameter. This path is also where the data will be stored. The content of the store is
//! flushed on drop or if you call `flush()`.
//!
//! ```no_run
//! use datastore::Datastore;
//! use datastore::JsonFileDatastore;
//!
//! let datastore = JsonFileDatastore::<Vec<u8>>::new("/tmp/test.json").unwrap();
//! datastore.put("foo".into(), vec![1, 2, 3]);
//! datastore.put("bar".into(), vec![0, 255, 127]);
//! assert_eq!(datastore.get("foo").unwrap(), &[1, 2, 3]);
//! datastore.flush().unwrap();        // optional
//! ```
//!
//! # Query
//!
//! In addition to simple operations such as `get` or `put`, the `Datastore` trait also provides
//! a way to perform queries on the key-value storage, using the `query` method.
//!
//! The struct returned by the `query` method implements the `Stream` trait from `futures`,
//! meaning that the result is asynchronous.
//!
//! > **Note**: For now the API of the `get` and `has` methods makes them potentially blocking
//! >           operations, though the only available implementation doesn't block. The API of these
//! >           methods may become asynchronous in the future if deemed necessary.
//!
//! ```no_run
//! extern crate datastore;
//! extern crate futures;
//!
//! # fn main() {
//! use datastore::{Query, Order, Filter, FilterTy, FilterOp};
//! use datastore::Datastore;
//! use datastore::JsonFileDatastore;
//! use futures::{Future, Stream};
//!
//! let datastore = JsonFileDatastore::<Vec<u8>>::new("/tmp/test.json").unwrap();
//! let query = datastore.query(Query {
//!     // Only return the keys that start with this prefix.
//!     prefix: "fo".into(),
//!     // List of filters for the keys and/or values.
//!     filters: vec![
//!         Filter {
//!             ty: FilterTy::ValueCompare(&vec![6, 7, 8].into()),
//!             operation: FilterOp::NotEqual,
//!         },
//!     ],
//!     // Order in which to sort the results.
//!     orders: vec![Order::ByKeyDesc],
//!     // Number of entries to skip at the beginning of the results (after sorting).
//!     skip: 1,
//!     // Limit to the number of entries to return (use `u64::max_value()` for no limit).
//!     limit: 12,
//!     // If true, don't load the values. For optimization purposes.
//!     keys_only: false,
//! });
//!
//! let results = query.collect().wait().unwrap();
//! println!("{:?}", results);
//! # }
//! ```

extern crate base64;
extern crate chashmap;
#[macro_use]
extern crate futures;
extern crate serde;
extern crate serde_json;
extern crate tempfile;

use futures::Stream;
use std::borrow::Cow;
use std::io::Error as IoError;
use std::ops::DerefMut;

mod json_file;
mod query;

pub use self::json_file::{JsonFileDatastore, JsonFileDatastoreEntry};
pub use self::query::{Filter, FilterOp, FilterTy, Order, Query};

/// Abstraction over any struct that can store `(key, value)` pairs.
pub trait Datastore<T> {
    /// Locked entry.
    type Entry: DerefMut<Target = T>;
    /// Output of a query.
    type QueryResult: Stream<Item = (String, T), Error = IoError>;

    /// Sets the value of a key.
    #[inline]
    fn put(self, key: Cow<str>, value: T)
    where
        Self: Sized,
    {
        *self.lock_or_create(key) = value;
    }

    /// Checks if an entry exists, and if so locks it.
    ///
    /// Trying to lock a value that is already locked will block, therefore you should keep locks
    /// for a duration that is as short as possible.
    fn lock(self, key: Cow<str>) -> Option<Self::Entry>;

    /// Locks an entry if it exists, or creates it otherwise.
    ///
    /// Same as `put` followed with `lock`, except that it is atomic.
    fn lock_or_create(self, key: Cow<str>) -> Self::Entry;

    /// Returns the value corresponding to this key by cloning it.
    #[inline]
    fn get(self, key: &str) -> Option<T>
    where
        Self: Sized,
        T: Clone,
    {
        self.lock(key.into()).map(|v| v.clone())
    }

    /// Returns true if the datastore contains the given key.
    ///
    /// > **Note**: Keep in mind that using this operation is probably racy. A secondary thread
    /// >             can delete a key right after you called `has()`. In other words, this function
    /// >            returns whether an entry with that key existed in the short past.
    #[inline]
    fn has(self, key: &str) -> bool
    where
        Self: Sized,
    {
        self.lock(key.into()).is_some()
    }

    /// Removes the given key from the datastore. Returns the old value if the key existed.
    fn delete(self, key: &str) -> Option<T>;

    /// Executes a query on the key-value store.
    ///
    /// This operation is expensive on some implementations and cheap on others. It is your
    /// responsibility to pick the right implementation for the right job.
    fn query(self, query: Query<T>) -> Self::QueryResult;
}
