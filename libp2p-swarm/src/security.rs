pub trait SecurityProvider {
    fn public_key(&self) -> &[u8];
}
