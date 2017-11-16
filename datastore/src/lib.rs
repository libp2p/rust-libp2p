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
#[macro_use]
extern crate futures;
extern crate parking_lot;
extern crate serde;
extern crate serde_json;
extern crate tempfile;

use futures::Stream;
use std::borrow::Cow;
use std::io::Error as IoError;

mod query;
mod json_file;

pub use self::json_file::JsonFileDatastore;
pub use self::query::{Query, Order, Filter, FilterTy, FilterOp};

/// Abstraction over any struct that can store `(key, value)` pairs.
pub trait Datastore<T> {
	/// Sets the value of a key.
	fn put(&self, key: Cow<str>, value: T);

	/// Returns the value corresponding to this key.
	// TODO: use higher-kinded stuff once stable to provide a more generic "accessor" for the data
	fn get(&self, key: &str) -> Option<T>;

	/// Returns true if the datastore contains the given key.
	fn has(&self, key: &str) -> bool;

	/// Removes the given key from the datastore. Returns true if the key existed.
	fn delete(&self, key: &str) -> bool;

	/// Executes a query on the key-value store.
	///
	/// This operation is expensive on some implementations and cheap on others. It is your
	/// responsibility to pick the right implementation for the right job.
	fn query<'a>(
		&'a self,
		query: Query<T>,
	) -> Box<Stream<Item = (String, T), Error = IoError> + 'a>;
}
