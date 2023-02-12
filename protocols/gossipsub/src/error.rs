// TODO: which copyright here?
//
//

#[deprecated(
    since = "0.44.0",
    note = "Use `libp2p::gossipsub::PublishError` instead, as the `error` will become crate-private in the future."
)]
pub type PublishError = crate::error_priv::PublishError;

#[deprecated(
    since = "0.44.0",
    note = "Use `libp2p::gossipsub::SubscriptionError` instead, as the `error` will become crate-private in the future."
)]
pub type SubscriptionError = crate::error_priv::SubscriptionError;

#[deprecated(
    since = "0.44.0",
    note = "Use re-exports that omit `Gossipsub` prefix, i.e. `libp2p::gossipsub::HandlerError"
)]
pub type GossipsubHandlerError = crate::error_priv::HandlerError;

#[deprecated(
    since = "0.44.0",
    note = "Use `libp2p::gossipsub::HandlerError instead, as the `error` will become crate-private in the future."
)]
pub type HandlerError = crate::error_priv::HandlerError;

#[deprecated(
    since = "0.44.0",
    note = "Use `libp2p::gossipsub::ValidationError` instead, as the `error` will become crate-private in the future."
)]
pub type ValidationError = crate::error_priv::ValidationError;
