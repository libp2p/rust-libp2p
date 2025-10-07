fn main() {
    cfg_aliases::cfg_aliases! {
        // Only native targets and `wasm32-unknown-unknown` are supported by `ring` crate.
        rsa_supported: { any(not(target_arch = "wasm32"), target_os = "unknown") }
    }
}
