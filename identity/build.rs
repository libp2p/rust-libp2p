fn main() {
    cfg_aliases::cfg_aliases! {
        rsa_supported: { any(not(target_arch = "wasm32"), target_os = "unknown") }
    }
}
