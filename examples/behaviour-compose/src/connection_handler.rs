use std::task::Poll;

use libp2p::{
    identify, mdns, ping,
    swarm::{handler::ConnectionEvent, ConnectionHandler, NetworkBehaviour},
};

// The actual types are not accessible from the crates but visible through trait.
// There will be TONS of types like these with `as` casts.
type IdentifyHandler = <identify::Behaviour as NetworkBehaviour>::ConnectionHandler;
type PingHandler = <ping::Behaviour as NetworkBehaviour>::ConnectionHandler;
type MdnsHandler = <mdns::tokio::Behaviour as NetworkBehaviour>::ConnectionHandler;

/// The composed `ConnectionHandler`
pub struct Handler {
    pub ping: PingHandler,
    pub identify: IdentifyHandler,
    pub mdns: MdnsHandler,
}

/// Aggregated `FromBehaviour` type that needs to be delegated to the respective
/// `ConnectionHandler`.
#[derive(Debug)]
pub enum FromBehaviour {
    Ping(<PingHandler as ConnectionHandler>::FromBehaviour),
    Identify(<IdentifyHandler as ConnectionHandler>::FromBehaviour),
    Mdns(<MdnsHandler as ConnectionHandler>::FromBehaviour),
}

/// Aggregated `ToBehaviour` type that needs to be aggregated to the upper
/// `NetworkBehaviour`.
#[derive(Debug)]
pub enum ToBehaviour {
    Ping(<PingHandler as ConnectionHandler>::ToBehaviour),
    Identify(<IdentifyHandler as ConnectionHandler>::ToBehaviour),
    Mdns(<MdnsHandler as ConnectionHandler>::ToBehaviour),
}

/// Module for code concerning inbound substream upgrade.
pub mod inbound_upgrade {
    use libp2p::{
        core::UpgradeInfo, swarm::handler::FullyNegotiatedInbound, InboundUpgrade, Stream,
    };

    use super::*;

    // Shorten them somewhat with type ailas.
    pub type PingProtocol = <<ping::Behaviour as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::InboundProtocol;
    pub type IdentifyProtocol = <<identify::Behaviour as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::InboundProtocol;
    pub type MdnsProtocol = <<mdns::tokio::Behaviour as NetworkBehaviour>::ConnectionHandler as ConnectionHandler>::InboundProtocol;

    /// Aggregated inbound `UpgradeInfo`
    #[derive(Clone)]
    pub enum UpgradeInfoSelect {
        Ping(<PingProtocol as UpgradeInfo>::Info),
        Identify(<IdentifyProtocol as UpgradeInfo>::Info),
        Mdns(<MdnsProtocol as UpgradeInfo>::Info),
    }

    // "Serialize" the type to compare against the protocol of incoming substream request,
    // as well as type erasure.
    impl AsRef<str> for UpgradeInfoSelect {
        fn as_ref(&self) -> &str {
            use UpgradeInfoSelect::*;
            match self {
                Ping(info) => AsRef::<str>::as_ref(info),
                Identify(info) => AsRef::<str>::as_ref(info),
                Mdns(info) => AsRef::<str>::as_ref(info),
            }
        }
    }

    /// Aggregated `UpgradeInfo` iterator.  
    /// ### Difference from outbound upgrade
    /// The peer accepting an inbound stream needs to know all the
    /// protocol it supports, but the other peer requesting an outbound only needs
    /// to know what kind of substream it wants. So for inbound upgrade this is a
    /// struct containing all `UpgradeInfo` from all behaviours that get composed.  
    type UpgradeInfoIter = IntoIter<UpgradeInfoSelect>;

    #[expect(deprecated)]
    pub struct OpenInfo {
        pub ping: <PingHandler as ConnectionHandler>::InboundOpenInfo,
        pub mdns: <MdnsHandler as ConnectionHandler>::InboundOpenInfo,
        pub identify: <IdentifyHandler as ConnectionHandler>::InboundOpenInfo,
    }

    pub struct Protocols {
        pub ping: PingProtocol,
        pub identify: IdentifyProtocol,
        pub mdns: MdnsProtocol,
    }

    impl UpgradeInfo for Protocols {
        type Info = UpgradeInfoSelect;
        type InfoIter = UpgradeInfoIter;

        fn protocol_info(&self) -> Self::InfoIter {
            self.ping
                .protocol_info()
                .into_iter()
                .map(UpgradeInfoSelect::Ping)
                .chain(
                    self.identify
                        .protocol_info()
                        .into_iter()
                        .map(UpgradeInfoSelect::Identify),
                )
                .chain(
                    self.mdns
                        .protocol_info()
                        .into_iter()
                        .map(UpgradeInfoSelect::Mdns),
                )
                .collect::<Vec<_>>()
                .into_iter()
        }
    }

    /// Aggregated type for upgrade errors.
    pub enum UpgradeErrorSelect {
        Ping(<PingProtocol as InboundUpgrade<::libp2p::Stream>>::Error),
        Identify(<IdentifyProtocol as InboundUpgrade<::libp2p::Stream>>::Error),
        Mdns(<MdnsProtocol as InboundUpgrade<::libp2p::Stream>>::Error),
    }

    use std::{pin::Pin, vec::IntoIter};

    type IdentifyFuture =
        <IdentifyProtocol as ::libp2p::core::upgrade::OutboundUpgrade<Stream>>::Future;
    type PingFuture = <PingProtocol as ::libp2p::core::upgrade::OutboundUpgrade<Stream>>::Future;
    type MdnsFuture = <MdnsProtocol as ::libp2p::core::upgrade::OutboundUpgrade<Stream>>::Future;

    pub enum SubstreamFuture {
        Ping(PingFuture),
        Identify(IdentifyFuture),
        Mdns(MdnsFuture),
    }

    pub enum Pinned<'a> {
        Ping(Pin<&'a mut PingFuture>),
        Mdns(Pin<&'a mut MdnsFuture>),
        Identify(Pin<&'a mut IdentifyFuture>),
    }

    /// Aggregated upgrade output, aka substream, that needs to be delegated to the respective
    /// `ConnectionHandler`.
    pub enum SubstreamSelect {
        Ping(<PingProtocol as InboundUpgrade<::libp2p::Stream>>::Output),
        Identify(<IdentifyProtocol as InboundUpgrade<::libp2p::Stream>>::Output),
        Mdns(<MdnsProtocol as InboundUpgrade<::libp2p::Stream>>::Output),
    }

    impl SubstreamFuture {
        pub fn as_pin_mut(self: ::std::pin::Pin<&mut Self>) -> Pinned<'_> {
            unsafe {
                match *::std::pin::Pin::get_unchecked_mut(self) {
                    SubstreamFuture::Ping(ref mut inner) => {
                        Pinned::Ping(::std::pin::Pin::new_unchecked(inner))
                    }
                    SubstreamFuture::Mdns(ref mut inner) => {
                        Pinned::Mdns(::std::pin::Pin::new_unchecked(inner))
                    }
                    SubstreamFuture::Identify(ref mut inner) => {
                        Pinned::Identify(::std::pin::Pin::new_unchecked(inner))
                    }
                }
            }
        }
    }

    impl ::futures::Future for SubstreamFuture {
        type Output = Result<SubstreamSelect, super::inbound_upgrade::UpgradeErrorSelect>;

        fn poll(
            self: ::std::pin::Pin<&mut Self>,
            cx: &mut ::std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            use ::std::task::Poll;
            match self.as_pin_mut() {
                Pinned::Ping(inner) => match inner.poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(inner) => Poll::Ready(match inner {
                        Ok(stream) => Ok(SubstreamSelect::Ping(stream)),
                        Err(upg_err) => Err(UpgradeErrorSelect::Ping(upg_err)),
                    }),
                },
                Pinned::Mdns(inner) => match inner.poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(inner) => Poll::Ready(match inner {
                        Ok(stream) => Ok(SubstreamSelect::Mdns(stream)),
                        Err(upg_err) => Err(UpgradeErrorSelect::Mdns(upg_err)),
                    }),
                },
                Pinned::Identify(inner) => match inner.poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(inner) => Poll::Ready(match inner {
                        Ok(stream) => Ok(SubstreamSelect::Identify(stream)),
                        Err(upg_err) => Err(UpgradeErrorSelect::Identify(upg_err)),
                    }),
                },
            }
        }
    }

    // Make the composed `Protocols` be able to actually accept inbound upgrades.
    impl ::libp2p::core::upgrade::InboundUpgrade<::libp2p::swarm::Stream> for Protocols {
        type Output = SubstreamSelect;
        type Future = SubstreamFuture;
        type Error = UpgradeErrorSelect;

        fn upgrade_inbound(self, sock: ::libp2p::Stream, info: UpgradeInfoSelect) -> Self::Future {
            match info {
                UpgradeInfoSelect::Ping(info) => {
                    Self::Future::Ping(self.ping.upgrade_inbound(sock, info))
                }
                UpgradeInfoSelect::Mdns(info) => {
                    Self::Future::Mdns(self.mdns.upgrade_inbound(sock, info))
                }
                UpgradeInfoSelect::Identify(info) => {
                    Self::Future::Identify(self.identify.upgrade_inbound(sock, info))
                }
            }
        }
    }

    #[expect(deprecated)]
    pub enum NegotiatedSelect {
        Ping(
            FullyNegotiatedInbound<
                PingProtocol,
                <PingHandler as ConnectionHandler>::OutboundOpenInfo,
            >,
        ),
        Mdns(
            FullyNegotiatedInbound<
                MdnsProtocol,
                <MdnsHandler as ConnectionHandler>::OutboundOpenInfo,
            >,
        ),
        Identify(
            FullyNegotiatedInbound<
                IdentifyProtocol,
                <IdentifyHandler as ConnectionHandler>::OutboundOpenInfo,
            >,
        ),
    }

    pub fn transpose_negotiated_inbound(
        inbound: FullyNegotiatedInbound<Protocols, OpenInfo>,
    ) -> NegotiatedSelect {
        match inbound {
            FullyNegotiatedInbound {
                protocol: SubstreamSelect::Ping(stream),
                info,
            } => NegotiatedSelect::Ping(FullyNegotiatedInbound {
                protocol: stream,
                info: info.ping,
            }),
            FullyNegotiatedInbound {
                protocol: SubstreamSelect::Mdns(stream),
                info,
            } => NegotiatedSelect::Mdns(FullyNegotiatedInbound {
                protocol: stream,
                info: info.mdns,
            }),
            FullyNegotiatedInbound {
                protocol: SubstreamSelect::Identify(stream),
                info,
            } => NegotiatedSelect::Identify(FullyNegotiatedInbound {
                protocol: stream,
                info: info.identify,
            }),
            #[allow(unreachable_patterns)]
            _ => panic!("protocol mismatch!"),
        }
    }
}

mod outbound_upgrade {
    use libp2p::{core::UpgradeInfo, OutboundUpgrade, Stream};

    use super::*;

    type PingProtocol = <PingHandler as ConnectionHandler>::OutboundProtocol;
    type IdentifyProtocol = <IdentifyHandler as ConnectionHandler>::OutboundProtocol;
    type MdnsProtocol = <MdnsHandler as ConnectionHandler>::OutboundProtocol;

    /// Aggreated type for outbound `UpgradeInfo`.
    /// ### Difference from inbound upgrade
    /// Does not require an accompanying iterator that outputs all supported protocols because you
    /// can only open one kind of substream at a time.
    #[derive(Clone)]
    pub enum UpgradeInfoSelect {
        Ping(<PingProtocol as UpgradeInfo>::Info),
        Identify(<IdentifyProtocol as UpgradeInfo>::Info),
        Mdns(<MdnsProtocol as UpgradeInfo>::Info),
    }

    // "Serialize" the protocol to send over the wire, as well as type erasure.
    impl AsRef<str> for UpgradeInfoSelect {
        fn as_ref(&self) -> &str {
            use UpgradeInfoSelect::*;
            match self {
                Ping(info) => AsRef::<str>::as_ref(info),
                Identify(info) => AsRef::<str>::as_ref(info),
                Mdns(info) => AsRef::<str>::as_ref(info),
            }
        }
    }

    /// A marker for the to-be-opened substream. You can put in information about
    /// what the substream is negotiated for.
    #[expect(deprecated)]
    pub enum OpenInfoSelect {
        Ping(<PingHandler as ConnectionHandler>::OutboundOpenInfo),
        Mdns(<MdnsHandler as ConnectionHandler>::OutboundOpenInfo),
        Identify(<IdentifyHandler as ConnectionHandler>::OutboundOpenInfo),
    }

    /// Aggregated type for outbound protocols.
    pub enum ProtocolSelect {
        Ping(PingProtocol),
        Identify(IdentifyProtocol),
        Mdns(MdnsProtocol),
    }

    impl UpgradeInfo for ProtocolSelect {
        type Info = UpgradeInfoSelect;
        // Only produce one `UpgradeInfo` because `ProtocolSelect` is also an enum.
        type InfoIter = core::iter::Once<UpgradeInfoSelect>;

        fn protocol_info(&self) -> Self::InfoIter {
            match self {
                Self::Ping(inner) => core::iter::once(UpgradeInfoSelect::Ping(
                    inner.protocol_info().into_iter().next().unwrap(),
                )),
                Self::Mdns(inner) => core::iter::once(UpgradeInfoSelect::Mdns(
                    inner.protocol_info().into_iter().next().unwrap(),
                    // Yes, this will panic when we're trying to negotiate a substream for `mdns`
                    // protocol. But in practice it never does, because the
                    // protocol never negotiate a substream for itself.
                )),
                Self::Identify(inner) => core::iter::once(UpgradeInfoSelect::Identify(
                    inner.protocol_info().into_iter().next().unwrap(),
                )),
            }
        }
    }

    pub enum UpgradeErrorSelect {
        Ping(<PingProtocol as OutboundUpgrade<::libp2p::Stream>>::Error),
        Identify(<IdentifyProtocol as OutboundUpgrade<::libp2p::Stream>>::Error),
        Mdns(<MdnsProtocol as OutboundUpgrade<::libp2p::Stream>>::Error),
    }

    use std::pin::Pin;

    type IdentifyFuture =
        <IdentifyProtocol as ::libp2p::core::upgrade::OutboundUpgrade<Stream>>::Future;
    type PingFuture = <PingProtocol as ::libp2p::core::upgrade::OutboundUpgrade<Stream>>::Future;
    type MdnsFuture = <MdnsProtocol as ::libp2p::core::upgrade::OutboundUpgrade<Stream>>::Future;

    pub enum SubstreamFuture {
        Ping(PingFuture),
        Identify(IdentifyFuture),
        Mdns(MdnsFuture),
    }

    pub enum Pinned<'a> {
        Ping(Pin<&'a mut PingFuture>),
        Mdns(Pin<&'a mut MdnsFuture>),
        Identify(Pin<&'a mut IdentifyFuture>),
    }

    pub enum SubstreamSelect {
        Ping(<PingProtocol as OutboundUpgrade<::libp2p::Stream>>::Output),
        Identify(<IdentifyProtocol as OutboundUpgrade<::libp2p::Stream>>::Output),
        Mdns(<MdnsProtocol as OutboundUpgrade<::libp2p::Stream>>::Output),
    }

    impl SubstreamFuture {
        pub fn as_pin_mut(self: ::std::pin::Pin<&mut Self>) -> Pinned<'_> {
            unsafe {
                match *::std::pin::Pin::get_unchecked_mut(self) {
                    SubstreamFuture::Ping(ref mut inner) => {
                        Pinned::Ping(::std::pin::Pin::new_unchecked(inner))
                    }
                    SubstreamFuture::Mdns(ref mut inner) => {
                        Pinned::Mdns(::std::pin::Pin::new_unchecked(inner))
                    }
                    SubstreamFuture::Identify(ref mut inner) => {
                        Pinned::Identify(::std::pin::Pin::new_unchecked(inner))
                    }
                }
            }
        }
    }

    impl ::futures::Future for SubstreamFuture {
        type Output = Result<SubstreamSelect, UpgradeErrorSelect>;

        fn poll(
            self: ::std::pin::Pin<&mut Self>,
            cx: &mut ::std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            use ::std::task::Poll;
            match self.as_pin_mut() {
                Pinned::Ping(inner) => match inner.poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(inner) => Poll::Ready(match inner {
                        Ok(stream) => Ok(SubstreamSelect::Ping(stream)),
                        Err(upg_err) => Err(UpgradeErrorSelect::Ping(upg_err)),
                    }),
                },
                Pinned::Mdns(inner) => match inner.poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(inner) => Poll::Ready(match inner {
                        Ok(stream) => Ok(SubstreamSelect::Mdns(stream)),
                        Err(upg_err) => Err(UpgradeErrorSelect::Mdns(upg_err)),
                    }),
                },
                Pinned::Identify(inner) => match inner.poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(inner) => Poll::Ready(match inner {
                        Ok(stream) => Ok(SubstreamSelect::Identify(stream)),
                        Err(upg_err) => Err(UpgradeErrorSelect::Identify(upg_err)),
                    }),
                },
            }
        }
    }

    impl ::libp2p::core::upgrade::OutboundUpgrade<::libp2p::swarm::Stream> for ProtocolSelect {
        type Output = SubstreamSelect;
        type Future = SubstreamFuture;
        type Error = UpgradeErrorSelect;

        fn upgrade_outbound(self, sock: ::libp2p::Stream, info: UpgradeInfoSelect) -> Self::Future {
            match info {
                UpgradeInfoSelect::Ping(info) => Self::Future::Ping(match self {
                    Self::Ping(inner) => inner.upgrade_outbound(sock, info),
                    _ => panic!("upgrade info and upgrade mismatch!"),
                }),
                UpgradeInfoSelect::Mdns(info) => Self::Future::Mdns(match self {
                    Self::Mdns(inner) => inner.upgrade_outbound(sock, info),
                    _ => panic!("upgrade info and upgrade mismatch!"),
                }),
                UpgradeInfoSelect::Identify(info) => Self::Future::Identify(match self {
                    Self::Identify(inner) => inner.upgrade_outbound(sock, info),
                    _ => panic!("upgrade info and upgrade mismatch!"),
                }),
            }
        }
    }

    use libp2p::swarm::handler::FullyNegotiatedOutbound;

    #[expect(deprecated)]
    pub(crate) enum NegotiatedSelect {
        Ping(
            FullyNegotiatedOutbound<
                PingProtocol,
                <PingHandler as ConnectionHandler>::OutboundOpenInfo,
            >,
        ),
        Mdns(
            FullyNegotiatedOutbound<
                MdnsProtocol,
                <MdnsHandler as ConnectionHandler>::OutboundOpenInfo,
            >,
        ),
        Identify(
            FullyNegotiatedOutbound<
                IdentifyProtocol,
                <IdentifyHandler as ConnectionHandler>::OutboundOpenInfo,
            >,
        ),
    }

    /// Delegate the negotiated outbound substream.
    pub(crate) fn transpose_full_outbound(
        outbound: FullyNegotiatedOutbound<ProtocolSelect, OpenInfoSelect>,
    ) -> NegotiatedSelect {
        match outbound {
            FullyNegotiatedOutbound {
                protocol: SubstreamSelect::Ping(stream),
                info: OpenInfoSelect::Ping(info),
            } => NegotiatedSelect::Ping(FullyNegotiatedOutbound {
                protocol: stream,
                info,
            }),
            FullyNegotiatedOutbound {
                protocol: SubstreamSelect::Mdns(stream),
                info: OpenInfoSelect::Mdns(info),
            } => NegotiatedSelect::Mdns(FullyNegotiatedOutbound {
                protocol: stream,
                info,
            }),
            FullyNegotiatedOutbound {
                protocol: SubstreamSelect::Identify(stream),
                info: OpenInfoSelect::Identify(info),
            } => NegotiatedSelect::Identify(FullyNegotiatedOutbound {
                protocol: stream,
                info,
            }),
            #[allow(unreachable_patterns)]
            _ => panic!("protocol mismatch!"),
        }
    }
}

impl Handler {
    #[expect(deprecated)]
    fn on_listen_upgrade_error(
        &mut self,
        ::libp2p::swarm::handler::ListenUpgradeError {
        info,
        error,
    }: ::libp2p::swarm::handler::ListenUpgradeError<
        <Self as ::libp2p::swarm::ConnectionHandler>::InboundOpenInfo,
        <Self as ::libp2p::swarm::ConnectionHandler>::InboundProtocol,
    >,
    ) {
        use inbound_upgrade::UpgradeErrorSelect::*;
        use libp2p::swarm::handler::ConnectionEvent::ListenUpgradeError;
        // delegate the error to their respective `ConnectionHandler`
        match error {
            Identify(error) => self.identify.on_connection_event(ListenUpgradeError(
                libp2p::swarm::handler::ListenUpgradeError {
                    info: info.identify,
                    error,
                },
            )),
            inbound_upgrade::UpgradeErrorSelect::Mdns(error) => self.mdns.on_connection_event(
                ListenUpgradeError(libp2p::swarm::handler::ListenUpgradeError {
                    info: info.identify,
                    error,
                }),
            ),
            inbound_upgrade::UpgradeErrorSelect::Ping(error) => self.ping.on_connection_event(
                ListenUpgradeError(libp2p::swarm::handler::ListenUpgradeError {
                    info: info.identify,
                    error,
                }),
            ),
        }
    }
}

#[expect(deprecated)]
impl ConnectionHandler for Handler {
    type FromBehaviour = FromBehaviour;

    type ToBehaviour = ToBehaviour;

    type InboundProtocol = inbound_upgrade::Protocols;

    type OutboundProtocol = outbound_upgrade::ProtocolSelect;

    type InboundOpenInfo = inbound_upgrade::OpenInfo;

    type OutboundOpenInfo = outbound_upgrade::OpenInfoSelect;

    fn listen_protocol(
        &self,
    ) -> libp2p::swarm::SubstreamProtocol<Self::InboundProtocol, Self::InboundOpenInfo> {
        let mdns = self.mdns.listen_protocol().into_upgrade();
        let ping = self.ping.listen_protocol().into_upgrade();
        let identify = self.identify.listen_protocol().into_upgrade();
        generate_substream_protocol(mdns, ping, identify)
    }

    fn connection_keep_alive(&self) -> bool {
        [
            self.mdns.connection_keep_alive(),
            self.ping.connection_keep_alive(),
            self.identify.connection_keep_alive(),
        ]
        .into_iter()
        .max()
        .unwrap_or(false)
    }

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<
        libp2p::swarm::ConnectionHandlerEvent<
            Self::OutboundProtocol,
            Self::OutboundOpenInfo,
            Self::ToBehaviour,
        >,
    > {
        // Polling all the `ConnectionHandler`. The order shouldn't matter because of the compiler.
        use libp2p::swarm::ConnectionHandlerEvent::*;
        match self.ping.poll(cx) {
            Poll::Ready(NotifyBehaviour(event)) => {
                return Poll::Ready(NotifyBehaviour(ToBehaviour::Ping(event)));
            }
            Poll::Ready(OutboundSubstreamRequest { protocol }) => {
                return Poll::Ready(OutboundSubstreamRequest {
                    protocol: protocol
                        .map_upgrade(|u| outbound_upgrade::ProtocolSelect::Ping(u))
                        .map_info(|i| outbound_upgrade::OpenInfoSelect::Ping(i)),
                });
            }
            Poll::Ready(ReportRemoteProtocols(report)) => {
                return Poll::Ready(ReportRemoteProtocols(report));
            }
            Poll::Pending => (),
            _ => (),
        };
        match self.identify.poll(cx) {
            Poll::Ready(NotifyBehaviour(event)) => {
                return Poll::Ready(NotifyBehaviour(ToBehaviour::Identify(event)));
            }
            Poll::Ready(OutboundSubstreamRequest { protocol }) => {
                return Poll::Ready(OutboundSubstreamRequest {
                    protocol: protocol
                        .map_upgrade(|u| outbound_upgrade::ProtocolSelect::Identify(u))
                        .map_info(|i| outbound_upgrade::OpenInfoSelect::Identify(i)),
                });
            }
            Poll::Ready(ReportRemoteProtocols(report)) => {
                return Poll::Ready(ReportRemoteProtocols(report));
            }
            Poll::Pending => (),
            _ => (),
        };
        match self.mdns.poll(cx) {
            Poll::Ready(NotifyBehaviour(event)) => {
                return Poll::Ready(NotifyBehaviour(ToBehaviour::Mdns(event)));
            }
            Poll::Ready(OutboundSubstreamRequest { protocol }) => {
                return Poll::Ready(OutboundSubstreamRequest {
                    protocol: protocol
                        .map_upgrade(|u| outbound_upgrade::ProtocolSelect::Mdns(u))
                        .map_info(|i| outbound_upgrade::OpenInfoSelect::Mdns(i)),
                });
            }
            Poll::Ready(ReportRemoteProtocols(report)) => {
                return Poll::Ready(ReportRemoteProtocols(report));
            }
            Poll::Pending => (),
            _ => (),
        };
        Poll::Pending
    }

    fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
        use FromBehaviour::*;
        match event {
            Ping(ev) => self.ping.on_behaviour_event(ev),
            Mdns(ev) => self.mdns.on_behaviour_event(ev),
            Identify(ev) => self.identify.on_behaviour_event(ev),
        }
    }

    fn on_connection_event(
        &mut self,
        event: libp2p::swarm::handler::ConnectionEvent<
            Self::InboundProtocol,
            Self::OutboundProtocol,
            Self::InboundOpenInfo,
            Self::OutboundOpenInfo,
        >,
    ) {
        match event {
            // Hand the negotiated substream over to the corresponding `ConnectionHandler`.
            // Substreams are negotiated and owned by their `ConnectionHandler`, not the composed
            // handler.
            ConnectionEvent::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                use outbound_upgrade::NegotiatedSelect::*;
                use ConnectionEvent::FullyNegotiatedOutbound;
                match outbound_upgrade::transpose_full_outbound(fully_negotiated_outbound) {
                    Mdns(inner) => self
                        .mdns
                        .on_connection_event(FullyNegotiatedOutbound(inner)),
                    Ping(inner) => self
                        .ping
                        .on_connection_event(FullyNegotiatedOutbound(inner)),
                    Identify(inner) => self
                        .identify
                        .on_connection_event(FullyNegotiatedOutbound(inner)),
                }
            }
            ConnectionEvent::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                use inbound_upgrade::NegotiatedSelect::*;
                use ConnectionEvent::FullyNegotiatedInbound;
                match inbound_upgrade::transpose_negotiated_inbound(fully_negotiated_inbound) {
                    Mdns(inner) => self.mdns.on_connection_event(FullyNegotiatedInbound(inner)),
                    Ping(inner) => self.ping.on_connection_event(FullyNegotiatedInbound(inner)),
                    Identify(inner) => self
                        .identify
                        .on_connection_event(FullyNegotiatedInbound(inner)),
                }
            }
            ConnectionEvent::DialUpgradeError(dial_upgrade_error) => {
                match transpose_upgr_error(dial_upgrade_error) {
                    DialUpgradeErrorSelect::Ping(inner) => self
                        .ping
                        .on_connection_event(ConnectionEvent::DialUpgradeError(inner)),
                    DialUpgradeErrorSelect::Mdns(inner) => self
                        .mdns
                        .on_connection_event(ConnectionEvent::DialUpgradeError(inner)),
                    DialUpgradeErrorSelect::Identify(inner) => self
                        .identify
                        .on_connection_event(ConnectionEvent::DialUpgradeError(inner)),
                }
            }
            // Below are events that need to be "broadcast" to all `ConnectionHandler`s.
            // We need to re-package the event to do "type-erasure" because even though
            // no generics are involved, they are still a part of the type.
            ConnectionEvent::AddressChange(address) => {
                self.mdns
                    .on_connection_event(ConnectionEvent::AddressChange(
                        libp2p::swarm::handler::AddressChange {
                            new_address: address.new_address,
                        },
                    ));
                self.ping
                    .on_connection_event(ConnectionEvent::AddressChange(
                        libp2p::swarm::handler::AddressChange {
                            new_address: address.new_address,
                        },
                    ));
                self.identify
                    .on_connection_event(ConnectionEvent::AddressChange(
                        libp2p::swarm::handler::AddressChange {
                            new_address: address.new_address,
                        },
                    ));
            }
            ConnectionEvent::ListenUpgradeError(listen_upgrade_error) => {
                self.on_listen_upgrade_error(listen_upgrade_error)
            }
            ConnectionEvent::LocalProtocolsChange(supported_protocols) => {
                self.mdns
                    .on_connection_event(ConnectionEvent::LocalProtocolsChange(
                        supported_protocols.clone(),
                    ));
                self.ping
                    .on_connection_event(ConnectionEvent::LocalProtocolsChange(
                        supported_protocols.clone(),
                    ));
                self.identify
                    .on_connection_event(ConnectionEvent::LocalProtocolsChange(
                        supported_protocols.clone(),
                    ));
            }
            ConnectionEvent::RemoteProtocolsChange(supported_protocols) => {
                self.mdns
                    .on_connection_event(ConnectionEvent::RemoteProtocolsChange(
                        supported_protocols.clone(),
                    ));
                self.ping
                    .on_connection_event(ConnectionEvent::RemoteProtocolsChange(
                        supported_protocols.clone(),
                    ));
                self.identify
                    .on_connection_event(ConnectionEvent::RemoteProtocolsChange(
                        supported_protocols.clone(),
                    ));
            }
            _ => unimplemented!("New branch not covered"),
        }
    }
}

/// Aggregates all supported protocols.
#[expect(deprecated)]
fn generate_substream_protocol(
    mdns: (
        <MdnsHandler as ::libp2p::swarm::ConnectionHandler>::InboundProtocol,
        <MdnsHandler as ::libp2p::swarm::ConnectionHandler>::InboundOpenInfo,
    ),
    ping: (
        <PingHandler as ::libp2p::swarm::ConnectionHandler>::InboundProtocol,
        <PingHandler as ::libp2p::swarm::ConnectionHandler>::InboundOpenInfo,
    ),
    identify: (
        <IdentifyHandler as ::libp2p::swarm::ConnectionHandler>::InboundProtocol,
        <IdentifyHandler as ::libp2p::swarm::ConnectionHandler>::InboundOpenInfo,
    ),
) -> ::libp2p::swarm::SubstreamProtocol<inbound_upgrade::Protocols, inbound_upgrade::OpenInfo> {
    ::libp2p::swarm::SubstreamProtocol::new(
        inbound_upgrade::Protocols {
            mdns: mdns.0,
            identify: identify.0,
            ping: ping.0,
        },
        inbound_upgrade::OpenInfo {
            mdns: mdns.1,
            identify: identify.1,
            ping: ping.1,
        },
    )
}

use libp2p::swarm::handler::{DialUpgradeError, StreamUpgradeError};
#[expect(deprecated)]
enum DialUpgradeErrorSelect {
    Ping(
        DialUpgradeError<
            <PingHandler as ConnectionHandler>::OutboundOpenInfo,
            <PingHandler as ConnectionHandler>::OutboundProtocol,
        >,
    ),
    Mdns(
        DialUpgradeError<
            <MdnsHandler as ConnectionHandler>::OutboundOpenInfo,
            <MdnsHandler as ConnectionHandler>::OutboundProtocol,
        >,
    ),
    Identify(
        DialUpgradeError<
            <IdentifyHandler as ConnectionHandler>::OutboundOpenInfo,
            <IdentifyHandler as ConnectionHandler>::OutboundProtocol,
        >,
    ),
}

fn transpose_upgr_error(
    error: DialUpgradeError<outbound_upgrade::OpenInfoSelect, outbound_upgrade::ProtocolSelect>,
) -> DialUpgradeErrorSelect {
    match error {
        DialUpgradeError {
            error: StreamUpgradeError::Apply(outbound_upgrade::UpgradeErrorSelect::Ping(error)),
            info: outbound_upgrade::OpenInfoSelect::Ping(info),
        } => DialUpgradeErrorSelect::Ping(DialUpgradeError {
            info,
            error: StreamUpgradeError::Apply(error),
        }),
        DialUpgradeError {
            error: StreamUpgradeError::Apply(outbound_upgrade::UpgradeErrorSelect::Mdns(error)),
            info: outbound_upgrade::OpenInfoSelect::Mdns(info),
        } => DialUpgradeErrorSelect::Mdns(DialUpgradeError {
            info,
            error: StreamUpgradeError::Apply(error),
        }),
        DialUpgradeError {
            error: StreamUpgradeError::Apply(outbound_upgrade::UpgradeErrorSelect::Identify(error)),
            info: outbound_upgrade::OpenInfoSelect::Identify(info),
        } => DialUpgradeErrorSelect::Identify(DialUpgradeError {
            info,
            error: StreamUpgradeError::Apply(error),
        }),

        DialUpgradeError {
            error: e,
            info: outbound_upgrade::OpenInfoSelect::Ping(info),
        } => DialUpgradeErrorSelect::Ping(DialUpgradeError {
            info,
            error: e.map_upgrade_err(|_| panic!("already handled above")),
        }),
        DialUpgradeError {
            error: e,
            info: outbound_upgrade::OpenInfoSelect::Mdns(info),
        } => DialUpgradeErrorSelect::Mdns(DialUpgradeError {
            info,
            error: e.map_upgrade_err(|_| panic!("already handled above")),
        }),
        DialUpgradeError {
            error: e,
            info: outbound_upgrade::OpenInfoSelect::Identify(info),
        } => DialUpgradeErrorSelect::Identify(DialUpgradeError {
            info,
            error: e.map_upgrade_err(|_| panic!("already handled above")),
        }),
    }
}
