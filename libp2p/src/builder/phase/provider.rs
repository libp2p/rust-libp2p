use std::marker::PhantomData;

#[allow(unused_imports)]
use super::*;
use crate::SwarmBuilder;
/// Represents the phase where a provider is not yet specified.
/// This is a marker type used in the type-state pattern to ensure compile-time checks of the
/// builder's state.
pub enum NoProviderSpecified {}

// Define enums for each of the possible runtime environments. These are used as markers in the
// type-state pattern, allowing compile-time checks for the appropriate environment configuration.

#[cfg(all(not(target_arch = "wasm32"), feature = "async-std"))]
/// Represents the AsyncStd runtime environment.
pub enum AsyncStd {}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
/// Represents the Tokio runtime environment.
pub enum Tokio {}

#[cfg(feature = "wasm-bindgen")]
/// Represents the WasmBindgen environment for WebAssembly.
pub enum WasmBindgen {}

/// Represents a phase in the SwarmBuilder where a provider has been chosen but not yet specified.
pub struct ProviderPhase {}

impl SwarmBuilder<NoProviderSpecified, ProviderPhase> {
    /// Configures the SwarmBuilder to use the AsyncStd runtime.
    /// This method is only available when compiling for non-Wasm
    /// targets with the `async-std` feature enabled.
    #[cfg(all(not(target_arch = "wasm32"), feature = "async-std"))]
    pub fn with_async_std(self) -> SwarmBuilder<AsyncStd, TcpPhase> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: TcpPhase {},
        }
    }

    /// Configures the SwarmBuilder to use the Tokio runtime.
    /// This method is only available when compiling for non-Wasm
    /// targets with the `tokio` feature enabled
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    pub fn with_tokio(self) -> SwarmBuilder<Tokio, TcpPhase> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: TcpPhase {},
        }
    }

    /// Configures the SwarmBuilder for WebAssembly using WasmBindgen.
    /// This method is available when the `wasm-bindgen` feature is enabled.
    #[cfg(feature = "wasm-bindgen")]
    pub fn with_wasm_bindgen(self) -> SwarmBuilder<WasmBindgen, TcpPhase> {
        SwarmBuilder {
            keypair: self.keypair,
            phantom: PhantomData,
            phase: TcpPhase {},
        }
    }
}
