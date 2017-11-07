extern crate base64;
#[macro_use]
extern crate futures;
extern crate parking_lot;
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
pub trait Datastore {
    /// Sets the value of a key.
    fn put(&self, key: Cow<str>, value: Vec<u8>);

    /// Returns the value corresponding to this key.
    // TODO: use higher-kinded stuff once stable to provide a more generic "accessor" for the data
    fn get(&self, key: &str) -> Option<Vec<u8>>;

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
        query: Query,
    ) -> Box<Stream<Item = (String, Vec<u8>), Error = IoError> + 'a>;
}
