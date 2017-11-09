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

use futures::{Stream, Future, Async, Poll};
use futures::stream::{iter_ok, Take as StreamTake, Skip as StreamSkip};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::io::Error as IoError;
use std::marker::PhantomData;
use std::vec::IntoIter as VecIntoIter;

/// Description of a query to apply on a datastore.
///
/// The various modifications of the dataset are applied in the same order as the fields (prefix,
/// filters, orders, skip, limit).
#[derive(Debug, Clone)]
pub struct Query<'a> {
    /// Only the keys that start with `prefix` will be returned.
    pub prefix: Cow<'a, str>,
    /// Filters to apply on the results.
    pub filters: Vec<Filter<'a>>,
    /// How to order the keys. Applied sequentially.
    pub orders: Vec<Order>,
    /// Number of elements to skip from at the start of the results.
    pub skip: u64,
    /// Maximum number of elements in the results.
    pub limit: u64,
    /// Only return keys. If true, then all the `Vec`s of the data will be empty.
    pub keys_only: bool,
}

/// A filter to apply to the results set.
#[derive(Debug, Clone)]
pub struct Filter<'a> {
    /// Type of filter and value to compare with.
    pub ty: FilterTy<'a>,
    /// Comparison operation.
    pub operation: FilterOp,
}

/// Type of filter and value to compare with.
#[derive(Debug, Clone)]
pub enum FilterTy<'a> {
    /// Compare the key with a reference value.
    KeyCompare(Cow<'a, str>),
    /// Compare the value with a reference value.
    ValueCompare(Cow<'a, [u8]>),
}

/// Filtering operation. Keep in mind that anything else than `Equal` and `NotEqual` is a bit
/// blurry.
#[derive(Debug, Copy, Clone)]
pub enum FilterOp {
    Less,
    LessOrEqual,
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
}

/// Order in which to sort the results of a query.
#[derive(Debug, Copy, Clone)]
pub enum Order {
    /// Put the values in ascending order.
    ByValueAsc,
    /// Put the values in descending order.
    ByValueDesc,
    /// Put the keys in ascending order.
    ByKeyAsc,
    /// Put the keys in descending order.
    ByKeyDesc,
}

/// Naively applies a query on a set of results.
pub fn naive_apply_query<'a, S>(stream: S, query: Query<'a>)
        -> StreamTake<StreamSkip<NaiveKeysOnlyApply<NaiveApplyOrdered<NaiveFiltersApply<'a, NaivePrefixApply<'a, S>, VecIntoIter<Filter<'a>>>>>>>
    where S: Stream<Item = (String, Vec<u8>), Error = IoError> + 'a
{
    let prefixed = naive_apply_prefix(stream, query.prefix);
    let filtered = naive_apply_filters(prefixed, query.filters.into_iter());
    let ordered = naive_apply_ordered(filtered, query.orders);
    let keys_only = naive_apply_keys_only(ordered, query.keys_only);
    naive_apply_skip_limit(keys_only, query.skip, query.limit)
}

/// Skips the `skip` first element of a stream and only returns `limit` elements.
#[inline]
pub fn naive_apply_skip_limit<S>(stream: S, skip: u64, limit: u64) -> StreamTake<StreamSkip<S>>
where
    S: Stream<Item = (String, Vec<u8>), Error = IoError>,
{
    stream.skip(skip).take(limit)
}

/// Filters the result of a stream to empty values if `keys_only` is true.
#[inline]
pub fn naive_apply_keys_only<S>(stream: S, keys_only: bool) -> NaiveKeysOnlyApply<S>
where
    S: Stream<Item = (String, Vec<u8>), Error = IoError>,
{
    NaiveKeysOnlyApply {
        keys_only: keys_only,
        stream: stream,
    }
}

/// Returned by `naive_apply_keys_only`.
#[derive(Debug, Clone)]
pub struct NaiveKeysOnlyApply<S> {
    keys_only: bool,
    stream: S,
}

impl<S> Stream for NaiveKeysOnlyApply<S>
where
    S: Stream<Item = (String, Vec<u8>), Error = IoError>,
{
    type Item = (String, Vec<u8>);
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if self.keys_only {
            Ok(Async::Ready(try_ready!(self.stream.poll()).map(|mut v| {
                v.1 = Vec::new();
                v
            })))
        } else {
            self.stream.poll()
        }
    }
}

/// Filters the result of a stream to only keep the results with a prefix.
#[inline]
pub fn naive_apply_prefix<'a, S>(stream: S, prefix: Cow<'a, str>) -> NaivePrefixApply<'a, S>
where
    S: Stream<Item = (String, Vec<u8>), Error = IoError>,
{
    NaivePrefixApply {
        prefix: prefix,
        stream: stream,
    }
}

/// Returned by `naive_apply_prefix`.
#[derive(Debug, Clone)]
pub struct NaivePrefixApply<'a, S> {
    prefix: Cow<'a, str>,
    stream: S,
}

impl<'a, S> Stream for NaivePrefixApply<'a, S>
where
    S: Stream<Item = (String, Vec<u8>), Error = IoError>,
{
    type Item = (String, Vec<u8>);
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        loop {
            let item = try_ready!(self.stream.poll());
            match item {
                Some(i) => {
                    if i.0.starts_with(&*self.prefix) {
                        return Ok(Async::Ready(Some(i)));
                    }
                }
                None => return Ok(Async::Ready(None)),
            }
        }
    }
}

/// Applies orderings on the stream data. Will simply pass data through if the list of orderings
/// is empty. Otherwise will need to collect.
pub fn naive_apply_ordered<'a, S, I>(stream: S, orders_iter: I) -> NaiveApplyOrdered<'a, S>
where
    S: Stream<Item = (String, Vec<u8>), Error = IoError> + 'a,
    I: IntoIterator<Item = Order>,
    I::IntoIter: 'a,
{
    let orders_iter = orders_iter.into_iter();
    if orders_iter.size_hint().1 == Some(0) {
        return NaiveApplyOrdered { inner: NaiveApplyOrderedInner::PassThrough(stream) };
    }

    let collected = stream
        .collect()
        .and_then(move |mut collected| {
            for order in orders_iter {
                match order {
                    Order::ByValueAsc => {
                        collected.sort_by(|a, b| a.1.cmp(&b.1));
                    }
                    Order::ByValueDesc => {
                        collected.sort_by(|a, b| b.1.cmp(&a.1));
                    }
                    Order::ByKeyAsc => {
                        collected.sort_by(|a, b| a.0.cmp(&b.0));
                    }
                    Order::ByKeyDesc => {
                        collected.sort_by(|a, b| b.0.cmp(&a.0));
                    }
                }
            }
            Ok(iter_ok(collected.into_iter()))
        })
        .flatten_stream();

    NaiveApplyOrdered { inner: NaiveApplyOrderedInner::Collected(Box::new(collected)) }
}

/// Returned by `naive_apply_ordered`.
pub struct NaiveApplyOrdered<'a, S> {
    inner: NaiveApplyOrderedInner<'a, S>,
}

enum NaiveApplyOrderedInner<'a, S> {
    PassThrough(S),
    Collected(Box<Stream<Item = (String, Vec<u8>), Error = IoError> + 'a>),
}

impl<'a, S> Stream for NaiveApplyOrdered<'a, S>
where
    S: Stream<Item = (String, Vec<u8>), Error = IoError>,
{
    type Item = (String, Vec<u8>);
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.inner {
            NaiveApplyOrderedInner::PassThrough(ref mut s) => s.poll(),
            NaiveApplyOrderedInner::Collected(ref mut s) => s.poll(),
        }
    }
}

/// Filters the result of a stream to apply a set of filters.
#[inline]
pub fn naive_apply_filters<'a, S, I>(stream: S, filters: I) -> NaiveFiltersApply<'a, S, I>
where
    S: Stream<Item = (String, Vec<u8>), Error = IoError>,
    I: Iterator<Item = Filter<'a>> + Clone,
{
    NaiveFiltersApply {
        filters: filters,
        stream: stream,
        marker: PhantomData,
    }
}

/// Returned by `naive_apply_prefix`.
#[derive(Debug, Clone)]
pub struct NaiveFiltersApply<'a, S, I> {
    filters: I,
    stream: S,
    marker: PhantomData<&'a ()>,
}

impl<'a, S, I> Stream for NaiveFiltersApply<'a, S, I>
where
    S: Stream<
        Item = (String, Vec<u8>),
        Error = IoError,
    >,
    I: Iterator<Item = Filter<'a>> + Clone,
{
    type Item = (String, Vec<u8>);
    type Error = IoError;

    #[inline]
    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        'outer: loop {
            let item = try_ready!(self.stream.poll());
            match item {
                Some(i) => {
                    for filter in self.filters.clone() {
                        if !naive_filter_test(&i, &filter) {
                            continue 'outer;
                        }
                    }
                    return Ok(Async::Ready(Some(i)));
                }
                None => return Ok(Async::Ready(None)),
            }
        }
    }
}

#[inline]
fn naive_filter_test(entry: &(String, Vec<u8>), filter: &Filter) -> bool {
    let (expected_ordering, revert_expected) = match filter.operation {
        FilterOp::Less => (Ordering::Less, false),
        FilterOp::LessOrEqual => (Ordering::Greater, true),
        FilterOp::Equal => (Ordering::Less, false),
        FilterOp::NotEqual => (Ordering::Less, true),
        FilterOp::Greater => (Ordering::Greater, false),
        FilterOp::GreaterOrEqual => (Ordering::Less, true),
    };

    match filter.ty {
        FilterTy::KeyCompare(ref ref_value) => {
            ((&*entry.0).cmp(&**ref_value) == expected_ordering) != revert_expected
        }
        FilterTy::ValueCompare(ref ref_value) => {
            ((&*entry.1).cmp(&**ref_value) == expected_ordering) != revert_expected
        }
    }
}
