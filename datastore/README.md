General-purpose key-value storage.
The keys are strings, and the values are of any type you want.

> **Note**: This crate is meant to be a utility for the implementation of other crates ; it
>			does not directly participate in the stack of libp2p.

This crate provides the `Datastore` trait, whose template parameter is the type of the value.
It is implemented on types that represent a key-value storage.
The only available implementation for now is `JsonFileDatastore`.

# JSON file datastore

The `JsonFileDatastore` can provide a key-value storage that loads and stores data in a single
JSON file. It is only available if the value implements the `Serialize`, `DeserializeOwned`
and `Clone` traits.

The `JsonFileDatastore::new` method will attempt to load existing data from the path you pass
as parameter. This path is also where the data will be stored. The content of the store is
flushed on destruction or if you call `flush()`.

```rust
use datastore::Datastore;
use datastore::JsonFileDatastore;

let datastore = JsonFileDatastore::<Vec<u8>>::new("/tmp/test.json").unwrap();
datastore.put("foo".into(), vec![1, 2, 3]);
datastore.put("bar".into(), vec![0, 255, 127]);
assert_eq!(datastore.get("foo").unwrap(), &[1, 2, 3]);
datastore.flush().unwrap();		// optional
```

# Query

In addition to simple operations such as `get` or `put`, the `Datastore` trait also provides
a way to perform queries on the key-value storage, using the `query` method.

The struct returned by the `query` method implements the `Stream` trait from `futures`,
meaning that the result is asynchronous.

> **Note**: For now the `get` and `has` methods are theoretically blocking, but the only
>			available implementation doesn't do any I/O. Maybe these methods will be made
> 			asynchronous in the future, if deemed necessary.

```rust
extern crate datastore;
extern crate futures;

use datastore::{Query, Order, Filter, FilterTy, FilterOp};
use datastore::Datastore;
use datastore::JsonFileDatastore;
use futures::{Future, Stream};

let datastore = JsonFileDatastore::<Vec<u8>>::new("/tmp/test.json").unwrap();
let query = datastore.query(Query {
    // Only return the keys that start with this prefix.
    prefix: "fo".into(),
    // List of filters for the keys and/or values.
    filters: vec![
        Filter {
            ty: FilterTy::ValueCompare(&vec![6, 7, 8].into()),
            operation: FilterOp::NotEqual,
        },
    ],
    // Order in which to sort the results.
    orders: vec![Order::ByKeyDesc],
    // Number of entries to skip at the beginning of the results (after sorting).
    skip: 1,
    // Limit to the number of entries to return (use `u64::max_value()` for no limit).
    limit: 12,
    // If true, don't load the values. For optimization purposes.
    keys_only: false,
});

let results = query.collect().wait().unwrap();
println!("{:?}", results);
```