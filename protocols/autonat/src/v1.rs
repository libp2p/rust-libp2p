// Copyright 2021 Protocol Labs.
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

//! Implementation of the [AutoNAT](https://github.com/libp2p/specs/blob/master/autonat/README.md) protocol.
//!
//! ## Eventual Deprecation
//! This version of the protocol will eventually be deprecated in favor of [v2](crate::v2).
//! We recommend using v2 for new projects.

#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

pub(crate) mod behaviour;
pub(crate) mod protocol;

pub use libp2p_request_response::{InboundFailure, OutboundFailure};

pub use self::{
    behaviour::{
        Behaviour, Config, Event, InboundProbeError, InboundProbeEvent, NatStatus,
        OutboundProbeError, OutboundProbeEvent, ProbeId,
    },
    protocol::{ResponseError, DEFAULT_PROTOCOL_NAME},
};

pub(crate) mod proto {
    #![allow(unreachable_pub)]
    include!("v1/generated/mod.rs");
    pub(crate) use self::structs::{mod_Message::*, Message};
}
