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

use crate::{
    multiaddr::{Multiaddr, Protocol},
    transport::PortUse,
};

/// The endpoint roles associated with a peer-to-peer communication channel.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// The socket comes from a dialer.
    Dialer,
    /// The socket comes from a listener.
    Listener,
}

impl std::ops::Not for Endpoint {
    type Output = Endpoint;

    fn not(self) -> Self::Output {
        match self {
            Endpoint::Dialer => Endpoint::Listener,
            Endpoint::Listener => Endpoint::Dialer,
        }
    }
}

impl Endpoint {
    /// Is this endpoint a dialer?
    pub fn is_dialer(self) -> bool {
        matches!(self, Endpoint::Dialer)
    }

    /// Is this endpoint a listener?
    pub fn is_listener(self) -> bool {
        matches!(self, Endpoint::Listener)
    }
}

/// The endpoint roles associated with an established peer-to-peer connection.
#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub enum ConnectedPoint {
    /// We dialed the node.
    Dialer {
        /// Multiaddress that was successfully dialed.
        address: Multiaddr,
        /// Whether the role of the local node on the connection should be
        /// overridden. I.e. whether the local node should act as a listener on
        /// the outgoing connection.
        ///
        /// This option is needed for NAT and firewall hole punching.
        ///
        /// - [`Endpoint::Dialer`] represents the default non-overriding option.
        ///
        /// - [`Endpoint::Listener`] represents the overriding option. Realization depends on the
        ///   transport protocol. E.g. in the case of TCP, both endpoints dial each other,
        ///   resulting in a _simultaneous open_ TCP connection. On this new connection both
        ///   endpoints assume to be the dialer of the connection. This is problematic during the
        ///   connection upgrade process where an upgrade assumes one side to be the listener. With
        ///   the help of this option, both peers can negotiate the roles (dialer and listener) for
        ///   the new connection ahead of time, through some external channel, e.g. the DCUtR
        ///   protocol, and thus have one peer dial the other and upgrade the connection as a
        ///   dialer and one peer dial the other and upgrade the connection _as a listener_
        ///   overriding its role.
        role_override: Endpoint,
        /// Whether the port for the outgoing connection was reused from a listener
        /// or a new port was allocated. This is useful for address translation.
        ///
        /// The port use is implemented on a best-effort basis. It is not guaranteed
        /// that [`PortUse::Reuse`] actually reused a port. A good example is the case
        /// where there is no listener available to reuse a port from.
        port_use: PortUse,
    },
    /// We received the node.
    Listener {
        /// Local connection address.
        local_addr: Multiaddr,
        /// Address used to send back data to the remote.
        send_back_addr: Multiaddr,
    },
}

impl From<&'_ ConnectedPoint> for Endpoint {
    fn from(endpoint: &'_ ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl From<ConnectedPoint> for Endpoint {
    fn from(endpoint: ConnectedPoint) -> Endpoint {
        endpoint.to_endpoint()
    }
}

impl ConnectedPoint {
    /// Turns the `ConnectedPoint` into the corresponding `Endpoint`.
    pub fn to_endpoint(&self) -> Endpoint {
        match self {
            ConnectedPoint::Dialer { .. } => Endpoint::Dialer,
            ConnectedPoint::Listener { .. } => Endpoint::Listener,
        }
    }

    /// Returns true if we are `Dialer`.
    pub fn is_dialer(&self) -> bool {
        matches!(self, ConnectedPoint::Dialer { .. })
    }

    /// Returns true if we are `Listener`.
    pub fn is_listener(&self) -> bool {
        matches!(self, ConnectedPoint::Listener { .. })
    }

    /// Returns true if the connection is relayed.
    pub fn is_relayed(&self) -> bool {
        match self {
            ConnectedPoint::Dialer { address, .. } => address,
            ConnectedPoint::Listener { local_addr, .. } => local_addr,
        }
        .iter()
        .any(|p| p == Protocol::P2pCircuit)
    }

    /// Returns the address of the remote stored in this struct.
    ///
    /// For `Dialer`, this returns `address`. For `Listener`, this returns `send_back_addr`.
    ///
    /// Note that the remote node might not be listening on this address and hence the address might
    /// not be usable to establish new connections.
    pub fn get_remote_address(&self) -> &Multiaddr {
        match self {
            ConnectedPoint::Dialer { address, .. } => address,
            ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr,
        }
    }

    /// Modifies the address of the remote stored in this struct.
    ///
    /// For `Dialer`, this modifies `address`. For `Listener`, this modifies `send_back_addr`.
    pub fn set_remote_address(&mut self, new_address: Multiaddr) {
        match self {
            ConnectedPoint::Dialer { address, .. } => *address = new_address,
            ConnectedPoint::Listener { send_back_addr, .. } => *send_back_addr = new_address,
        }
    }
}
