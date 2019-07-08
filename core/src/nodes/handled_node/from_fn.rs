// Copyright 2019 Parity Technologies (UK) Ltd.
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

#![cfg(feature = "async-await")]

pub struct FromFn<THandler> {
    handler: THandler,
}

/*impl<THandler> NodeHandler for FromFn<THandler> {
    type InEvent;
    type OutEvent;
    type Error;
    type Substream;
    type OutboundOpenInfo;

    fn inject_substream(&mut self, substream: Self::Substream, endpoint: NodeHandlerEndpoint<Self::OutboundOpenInfo>) {

    }

    fn inject_event(&mut self, event: Self::InEvent) {

    }

    fn poll(&mut self, cx: &mut Context)
        -> Poll<Result<NodeHandlerEvent<Self::OutboundOpenInfo, Self::OutEvent>, Self::Error>>
    {

    }
}*/

impl<THandler> NodeInterface<THandler> {
    pub fn new<TInEvent, TOutEvent, TSubstream>(builder: impl FnOnce(NodeInterface<TInEvent, TOutEvent, TSubstream>) -> THandler) -> FromFn<THandler>
    where
        THandler: Future<Output = ()>
    {
        let interface = NodeInterface {

        };

        NodeInterface {
            handler: builder(interface),
        }
    }
}

pub struct NodeInterface<TInEvent, TOutEvent, TSubstream> {
}

impl<TOutEvent> NodeInterface<TOutEvent> {
    pub async fn open_substream(&self) -> Substream {
        panic!()//futures::future::ready()
    }

    pub async fn send_event(&self, event: TOutEvent) {

    }

    pub async fn next_event(&self) -> TInEvent {

    }
}
