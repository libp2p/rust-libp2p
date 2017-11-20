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

mod query;
mod json_file;

pub use self::json_file::{JsonFileDatastore, JsonFileDatastoreEntry};
pub use self::query::{Query, Order, Filter, FilterTy, FilterOp};

/// Abstraction over any struct that can store `(key, value)` pairs.
pub trait Datastore<T> {
	/// Locked entry.
	type Entry: DerefMut<Target = T>;
	/// Output of a query.
	type QueryResult: Stream<Item = (String, T), Error = IoError>;

	/// Sets the value of a key.
	#[inline]
	fn put(self, key: Cow<str>, value: T)
		where Self: Sized
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
		where Self: Sized,
		      T: Clone
	{
		self.lock(key.into()).map(|v| v.clone())
	}

	/// Returns true if the datastore contains the given key.
	///
	/// > **Note**: Keep in mind that using this operation is probably racy. A secondary thread
	/// > 			can delete a key right after you called `has()`. In other words, this function
	/// >			returns whether an entry with that key existed in the short past.
	#[inline]
	fn has(self, key: &str) -> bool
		where Self: Sized
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
