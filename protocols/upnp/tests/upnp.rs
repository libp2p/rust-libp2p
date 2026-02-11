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

//! Integration tests for the `UPnP` network behaviour.
//!
//! NOTE: If an IGD device exists in the test environment, results may be unintended.

use std::time::Duration;

use libp2p_swarm::{Swarm, SwarmEvent};
use libp2p_swarm_test::SwarmExt;
use libp2p_upnp as upnp;
use mock_igd::{Action, MockIgdServer, Protocol, Responder};

/// Mock external IP address used for testing.
/// This address is from the TEST-NET-3 range (203.0.113.0/24) reserved for testing (RFC 5737).
const MOCK_EXTERNAL_IP: &str = "203.0.113.42";

/// Test that port mapping succeeds when the gateway responds successfully.
#[tokio::test]
async fn port_mapping_success() {
    // Start the mock IGD server with SSDP enabled
    let igd_server = MockIgdServer::builder()
        .with_ssdp()
        .start()
        .await
        .expect("Failed to start mock IGD server");

    // Register mock behaviors
    igd_server
        .mock(
            Action::GetExternalIPAddress,
            Responder::success().with_external_ip(MOCK_EXTERNAL_IP.parse().unwrap()),
        )
        .await;

    igd_server
        .mock(
            Action::add_port_mapping().with_protocol(Protocol::TCP),
            Responder::success(),
        )
        .await;

    // Use test_mode to bypass IP address validations for testing.
    let mut swarm = Swarm::new_ephemeral_tokio(|_| upnp::tokio::Behaviour::new_with_test_mode());

    swarm.listen().await;

    // Wait for UPnP events
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SwarmEvent::Behaviour(upnp_event) = swarm.next_swarm_event().await {
                match upnp_event {
                    upnp::Event::NewExternalAddr { external_addr, .. } => {
                        assert!(
                            external_addr.to_string().contains(MOCK_EXTERNAL_IP),
                            "External address should contain the mock external IP"
                        );
                        break;
                    }
                    event => panic!("Unexpected event: {:?}", event),
                }
            }
        }
    })
    .await
    .unwrap();

    // Verify that the IGD server received the expected requests
    let requests = igd_server.received_requests().await;
    assert!(
        requests
            .iter()
            .any(|r| r.action_name == "GetExternalIPAddress"),
        "Should have received GetExternalIPAddress request"
    );
    assert!(
        requests.iter().any(|r| r.action_name == "AddPortMapping"),
        "Should have received AddPortMapping request"
    );

    igd_server.shutdown();
}

/// Test that port mapping failure is handled gracefully.
#[tokio::test]
async fn port_mapping_failure() {
    // Start the mock IGD server with SSDP enabled
    let igd_server = MockIgdServer::builder()
        .with_ssdp()
        .start()
        .await
        .expect("Failed to start mock IGD server");

    // Register mock behaviors
    igd_server
        .mock(
            Action::GetExternalIPAddress,
            Responder::success().with_external_ip(MOCK_EXTERNAL_IP.parse().unwrap()),
        )
        .await;

    // Error 718 = ConflictInMappingEntry
    igd_server
        .mock(
            Action::add_port_mapping().with_protocol(Protocol::TCP),
            Responder::error(718, "ConflictInMappingEntry"),
        )
        .await;

    // Use test_mode to bypass IP address validations for testing.
    let mut swarm = Swarm::new_ephemeral_tokio(|_| upnp::tokio::Behaviour::new_with_test_mode());

    swarm.listen().await;

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SwarmEvent::Behaviour(upnp_event) = swarm.next_swarm_event().await {
                // We expect no UPnP event due to the port mapping error
                panic!("Unexpected UPnP event: {upnp_event:?}");
            }
        }
    })
    .await;

    // Timeout is expected since mapping fails and no success event is emitted
    assert!(
        result.is_err(),
        "Expected timeout since port mapping should fail"
    );

    // Verify that the IGD server received the AddPortMapping request
    let requests = igd_server.received_requests().await;
    assert!(
        requests.iter().any(|r| r.action_name == "AddPortMapping"),
        "Should have received AddPortMapping request"
    );

    igd_server.shutdown();
}

/// Test that the behaviour emits NonRoutableGateway when the gateway returns a private IP address.
#[tokio::test]
async fn non_routable_gateway() {
    // Start the mock IGD server with SSDP enabled
    let igd_server = MockIgdServer::builder()
        .with_ssdp()
        .start()
        .await
        .expect("Failed to start mock IGD server");

    // Register mock behaviors
    igd_server
        .mock(
            Action::GetExternalIPAddress,
            // Use a private IP address (192.168.0.1) to simulate a non-routable gateway.
            // This should trigger the NonRoutableGateway event.
            Responder::success().with_external_ip("192.168.0.1".parse().unwrap()),
        )
        .await;

    // Don't use test_mode so that the private IP address check remains active.
    let mut swarm = Swarm::new_ephemeral_tokio(|_| upnp::tokio::Behaviour::default());

    swarm.listen().await;

    // Wait for UPnP events
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SwarmEvent::Behaviour(upnp_event) = swarm.next_swarm_event().await {
                match upnp_event {
                    // The gateway returned a private IP, so it should be deemed non-routable.
                    upnp::Event::NonRoutableGateway => {
                        break;
                    }
                    event => panic!("Unexpected event: {:?}", event),
                }
            }
        }
    })
    .await
    .unwrap();

    // Verify that the IGD server received the expected requests
    let requests = igd_server.received_requests().await;
    assert!(
        requests
            .iter()
            .any(|r| r.action_name == "GetExternalIPAddress"),
        "Should have received GetExternalIPAddress request"
    );

    igd_server.shutdown();
}

/// Test that the behaviour emits GatewayNotFound when no gateway is available.
#[tokio::test]
async fn gateway_not_found() {
    // No mock server - SSDP discovery should fail
    let mut swarm = Swarm::new_ephemeral_tokio(|_| upnp::tokio::Behaviour::new_with_test_mode());

    swarm.listen().await;

    // Wait for GatewayNotFound event
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let SwarmEvent::Behaviour(upnp_event) = swarm.next_swarm_event().await {
                match upnp_event {
                    upnp::Event::GatewayNotFound => {
                        // Expected event received - test passes
                        break;
                    }
                    event => panic!("Unexpected UPnP event: {:?}", event),
                }
            }
        }
    })
    .await
    .unwrap();
}
