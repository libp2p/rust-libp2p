use super::*;
use crate::SwarmBuilder;
use std::marker::PhantomData;

pub struct IdentityPhase {}

impl SwarmBuilder<NoProviderSpecified, IdentityPhase> {
    pub fn with_new_identity() -> SwarmBuilder<NoProviderSpecified, ProviderPhase> {
        SwarmBuilder::with_existing_identity(libp2p_identity::Keypair::generate_ed25519())
    }

    pub fn with_existing_identity(
        keypair: libp2p_identity::Keypair,
    ) -> SwarmBuilder<NoProviderSpecified, ProviderPhase> {
        SwarmBuilder {
            keypair,
            phantom: PhantomData,
            phase: ProviderPhase {},
        }
    }
}
