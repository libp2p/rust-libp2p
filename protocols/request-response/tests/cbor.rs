// Copyright 2020 Parity Technologies (UK) Ltd.
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

//! Integration tests for the `cbor::Behaviour`.

use std::iter;
use libp2p_swarm::{StreamProtocol, Swarm};
use libp2p_request_response::ProtocolSupport;
use libp2p_request_response::cbor::Behaviour;
use libp2p_swarm_test::SwarmExt;

#[async_std::test]
async fn cbor() {
    let protocols = iter::once((StreamProtocol::new("/test_cbor/1"), ProtocolSupport::Full));
    let cfg = request_response::Config::default();

    let behaviour_1: Behaviour<TestRequest, TestResponse> =
        request_response::cbor::new_behaviour(protocols.clone(), cfg.clone());

    let mut swarm1 = Swarm::new_ephemeral(|_| behaviour_1);
    let peer1_id = *swarm1.local_peer_id();

    let behaviour_2: Behaviour<TestRequest, TestResponse> =
        request_response::cbor::new_behaviour(protocols.clone(), cfg.clone());
    let mut swarm2 = Swarm::new_ephemeral(|_| behaviour_2);
    let peer2_id = *swarm2.local_peer_id();

    swarm1.listen().await;
    swarm2.connect(&mut swarm1).await;

    let test_req = TestRequest {
        payload: "test_request".to_string(),
    };
    let test_resp = TestResponse {
        payload: "test_response".to_string(),
    };

    let expected_req = test_req.clone();
    let expected_resp = test_resp.clone();

    let peer1 = async move {
        loop {
            match swarm1.next_swarm_event().await.try_into_behaviour_event() {
                Ok(request_response::Event::Message {
                       peer,
                       message:
                       request_response::Message::Request {
                           request, channel, ..
                       },
                   }) => {
                    assert_eq!(&request, &expected_req);
                    assert_eq!(&peer, &peer2_id);
                    swarm1
                        .behaviour_mut()
                        .send_response(channel, test_resp.clone())
                        .unwrap();
                }
                Ok(request_response::Event::ResponseSent { peer, .. }) => {
                    assert_eq!(&peer, &peer2_id);
                }
                Ok(e) => {
                    panic!("Peer1: Unexpected event: {e:?}")
                }
                Err(..) => {}
            }
        }
    };

    let num_requests: u8 = rand::thread_rng().gen_range(1..100);

    let peer2 = async {
        let mut count = 0;

        let mut req_id = swarm2
            .behaviour_mut()
            .send_request(&peer1_id, test_req.clone());
        assert!(swarm2.behaviour().is_pending_outbound(&peer1_id, &req_id));

        loop {
            match swarm2
                .next_swarm_event()
                .await
                .try_into_behaviour_event()
                .unwrap()
            {
                request_response::Event::Message {
                    peer,
                    message:
                    request_response::Message::Response {
                        request_id,
                        response,
                    },
                } => {
                    count += 1;
                    assert_eq!(&response, &expected_resp);
                    assert_eq!(&peer, &peer1_id);
                    assert_eq!(req_id, request_id);
                    if count >= num_requests {
                        return;
                    } else {
                        req_id = swarm2
                            .behaviour_mut()
                            .send_request(&peer1_id, test_req.clone());
                    }
                }
                e => panic!("Peer2: Unexpected event: {e:?}"),
            }
        }
    };

    async_std::task::spawn(Box::pin(peer1));
    peer2.await;
}
