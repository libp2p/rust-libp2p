#[allow(unused_imports)]
use super::*;

use crate::SwarmBuilder;

pub struct ProviderPhase {}

impl SwarmBuilder<NoProviderSpecified, ProviderPhase> {
    #[cfg(all(not(target_arch = "wasm32"), feature = "async-std"))]
    pub fn with_async_std(self) -> SwarmBuilder<AsyncStd, TcpPhase> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: std::marker::PhantomData,
            phase: TcpPhase {},
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    pub fn with_tokio(self) -> SwarmBuilder<Tokio, TcpPhase> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: std::marker::PhantomData,
            phase: TcpPhase {},
        }
    }

    #[cfg(feature = "wasm-bindgen")]
    pub fn with_wasm_bindgen(self) -> SwarmBuilder<WasmBindgen, TcpPhase> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: std::marker::PhantomData,
            phase: TcpPhase {},
        }
    }
}

pub enum NoProviderSpecified {}

#[cfg(all(not(target_arch = "wasm32"), feature = "async-std"))]
pub enum AsyncStd {}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
pub enum Tokio {}

#[cfg(feature = "wasm-bindgen")]
pub enum WasmBindgen {}
