// Copyright 2023 Protocol Labs.
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

#[deprecated(
    since = "0.44.0",
    note = "Use `libp2p::gossipsub::PublishError` instead, as the `error` module will become crate-private in the future."
)]
pub type PublishError = crate::error_priv::PublishError;

#[deprecated(
    since = "0.44.0",
    note = "Use `libp2p::gossipsub::SubscriptionError` instead, as the `error` module will become crate-private in the future."
)]
pub type SubscriptionError = crate::error_priv::SubscriptionError;

#[deprecated(note = "This error will no longer be emitted")]
pub type GossipsubHandlerError = crate::error_priv::HandlerError;

#[deprecated(note = "This error will no longer be emitted")]
pub type HandlerError = crate::error_priv::HandlerError;

#[deprecated(
    since = "0.44.0",
    note = "Use `libp2p::gossipsub::ValidationError` instead, as the `error` module will become crate-private in the future."
)]
pub type ValidationError = crate::error_priv::ValidationError;
