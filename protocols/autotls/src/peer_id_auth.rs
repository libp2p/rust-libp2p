use std::collections::HashMap;

use base64::{
    Engine, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};
use libp2p_identity::{DecodingError, Keypair, PublicKey, SigningError};
use rand::RngCore;

const SCHEME: &str = "libp2p-PeerID";

const BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// Errors produced while authenticating with the `libp2p-PeerID` scheme.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Signing the challenge failed.
    #[error("failed to sign challenge")]
    Signing(#[from] SigningError),
    /// A header value was not valid base64url.
    #[error("invalid base64 value")]
    Base64(#[from] base64::DecodeError),
    /// A `public-key` value was not a valid protobuf-encoded public key.
    #[error("invalid public key")]
    PublicKey(#[from] DecodingError),
}

/// Client for the libp2p HTTP `libp2p-PeerID` authentication scheme.
#[derive(Debug, Clone)]
pub struct PeerIdAuthClient {
    keypair: Keypair,
}

impl PeerIdAuthClient {
    /// Create a client that authenticates with the given identity keypair.
    pub fn new(keypair: Keypair) -> Self {
        Self { keypair }
    }

    /// The base64url-encoded protobuf public key, for the `public-key` header parameter.
    pub fn public_key_param(&self) -> String {
        BASE64.encode(self.keypair.public().encode_protobuf())
    }

    /// Generate a fresh random `challenge-server` value (base64url-encoded 32 random bytes).
    pub fn generate_challenge() -> String {
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        BASE64.encode(bytes)
    }

    /// Produce the client's signature over `{challenge-client, server-public-key, hostname}`,
    /// base64url-encoded for the `sig` header parameter.
    pub fn sign_challenge(
        &self,
        challenge_client: &str,
        server_public_key: &PublicKey,
        hostname: &str,
    ) -> Result<String, Error> {
        let server_key = server_public_key.encode_protobuf();
        let data = gen_data_to_sign(&[
            ("challenge-client", challenge_client.as_bytes()),
            ("server-public-key", server_key.as_slice()),
            ("hostname", hostname.as_bytes()),
        ]);
        Ok(BASE64.encode(self.keypair.sign(&data)?))
    }

    /// Verify the server's base64url-encoded signature over
    /// `{challenge-server, client-public-key, hostname}`, where `challenge_server` is the challenge
    /// this client sent and the client public key is this client's own.
    pub fn verify_server(
        &self,
        challenge_server: &str,
        server_public_key: &PublicKey,
        hostname: &str,
        sig: &str,
    ) -> Result<bool, Error> {
        let sig = BASE64.decode(sig)?;
        let client_key = self.keypair.public().encode_protobuf();
        let data = gen_data_to_sign(&[
            ("challenge-server", challenge_server.as_bytes()),
            ("client-public-key", client_key.as_slice()),
            ("hostname", hostname.as_bytes()),
        ]);
        Ok(server_public_key.verify(&data, &sig))
    }
}

/// Decode a base64url-encoded protobuf `public-key` parameter value.
pub fn decode_public_key(value: &str) -> Result<PublicKey, Error> {
    let bytes = BASE64.decode(value)?;
    Ok(PublicKey::try_decode_protobuf(&bytes)?)
}

/// Build a `libp2p-PeerID` auth header value from the given ordered parameters.
pub fn build_auth_header(params: &[(&str, &str)]) -> String {
    let mut header = String::from(SCHEME);
    for (i, (name, value)) in params.iter().enumerate() {
        header.push_str(if i == 0 { " " } else { ", " });
        header.push_str(name);
        header.push_str("=\"");
        header.push_str(value);
        header.push('"');
    }
    header
}

/// Parse the parameters of a `libp2p-PeerID` auth header value, tolerating both comma- and
/// space-separated parameters. Returns `None` if the scheme prefix or quoting is malformed.
pub fn parse_auth_params(header: &str) -> Option<HashMap<String, String>> {
    let mut rest = header.strip_prefix(SCHEME)?.trim_start();
    let mut params = HashMap::new();
    while !rest.is_empty() {
        rest = rest.trim_start_matches([',', ' ', '\t']);
        if rest.is_empty() {
            break;
        }
        let eq = rest.find('=')?;
        let name = rest[..eq].trim();
        let value = rest[eq + 1..].strip_prefix('"')?;
        let end = value.find('"')?;
        params.insert(name.to_owned(), value[..end].to_owned());
        rest = &value[end + 1..];
    }
    Some(params)
}

fn gen_data_to_sign(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut built: Vec<Vec<u8>> = parts
        .iter()
        .map(|(name, value)| {
            let mut part = Vec::with_capacity(name.len() + 1 + value.len());
            part.extend_from_slice(name.as_bytes());
            part.push(b'=');
            part.extend_from_slice(value);
            part
        })
        .collect();
    built.sort();

    let mut data = SCHEME.as_bytes().to_vec();
    let mut buffer = unsigned_varint::encode::usize_buffer();
    for part in &built {
        data.extend_from_slice(unsigned_varint::encode::usize(part.len(), &mut buffer));
        data.extend_from_slice(part);
    }
    data
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use super::*;

    const SERVER_PRIVKEY_PB: [u8; 68] = hex!(
        "0801124001010101010101010101010101010101010101010101010101010101010101018a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c"
    );
    const CLIENT_PRIVKEY_PB: [u8; 68] = hex!(
        "0801124002020202020202020202020202020202020202020202020202020202020202028139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394"
    );
    const CLIENT_PUBKEY_PB: [u8; 36] =
        hex!("080112208139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b394");
    const HOSTNAME: &str = "example.com";
    const CHALLENGE: &str = "ERERERERERERERERERERERERERERERERERERERERERE=";
    const SERVER_SIG: &str =
        "UA88qZbLUzmAxrD9KECbDCgSKAUBAvBHrOCF2X0uPLR1uUCF7qGfLPc7dw3Olo-LaFCDpk5sXN7TkLWPVvuXAA==";
    const CLIENT_SIG: &str =
        "OrwJPO4buHKJdKXP2av8PFwv3XF_-m5MqndskeVV5UzufYzBCTm7RBaFnBS1sEhuQHZSZPh9RJgN5NmLzrUrBQ==";

    fn server_keypair() -> Keypair {
        Keypair::from_protobuf_encoding(&SERVER_PRIVKEY_PB).unwrap()
    }

    fn client() -> PeerIdAuthClient {
        PeerIdAuthClient::new(Keypair::from_protobuf_encoding(&CLIENT_PRIVKEY_PB).unwrap())
    }

    #[test]
    fn data_to_sign_matches_spec() {
        let data = gen_data_to_sign(&[
            ("challenge-server", CHALLENGE.as_bytes()),
            ("client-public-key", CLIENT_PUBKEY_PB.as_slice()),
            ("hostname", HOSTNAME.as_bytes()),
        ]);
        let expected = hex!(
            "6c69627032702d5065657249443d6368616c6c656e67652d7365727665723d455245524552455245524552455245524552455245524552455245524552455245524552455245524552453d36636c69656e742d7075626c69632d6b65793d080112208139770ea87d175f56a35466c34c7ecccb8d8a91b4ee37a25df60f5b8fc9b39414686f73746e616d653d6578616d706c652e636f6d"
        );
        assert_eq!(data, expected);
    }

    #[test]
    fn server_signature_matches_spec() {
        let data = gen_data_to_sign(&[
            ("challenge-server", CHALLENGE.as_bytes()),
            ("client-public-key", CLIENT_PUBKEY_PB.as_slice()),
            ("hostname", HOSTNAME.as_bytes()),
        ]);
        let sig = server_keypair().sign(&data).unwrap();
        assert_eq!(BASE64.encode(sig), SERVER_SIG);
    }

    #[test]
    fn client_sign_challenge_matches_spec() {
        let sig = client()
            .sign_challenge(CHALLENGE, &server_keypair().public(), HOSTNAME)
            .unwrap();
        assert_eq!(sig, CLIENT_SIG);
    }

    #[test]
    fn verify_server_signature_from_spec() {
        assert!(
            client()
                .verify_server(CHALLENGE, &server_keypair().public(), HOSTNAME, SERVER_SIG)
                .unwrap()
        );
    }

    #[test]
    fn decode_public_key_round_trips() {
        let encoded = BASE64.encode(server_keypair().public().encode_protobuf());
        assert_eq!(
            decode_public_key(&encoded).unwrap(),
            server_keypair().public()
        );
    }

    #[test]
    fn parse_auth_params_handles_padding_and_build_round_trips() {
        let header = format!(
            r#"libp2p-PeerID challenge-client="{CHALLENGE}", public-key="CAESIIqI4910CfGV_VLbLTy6XXLKZwm_HZQSG_N0iAG0D29c", sig="{SERVER_SIG}""#
        );
        let params = parse_auth_params(&header).unwrap();
        assert_eq!(params["challenge-client"], CHALLENGE);
        assert_eq!(params["sig"], SERVER_SIG);
        assert_eq!(
            params["public-key"],
            "CAESIIqI4910CfGV_VLbLTy6XXLKZwm_HZQSG_N0iAG0D29c"
        );

        assert_eq!(
            build_auth_header(&[("public-key", "CAESII"), ("sig", "abc")]),
            r#"libp2p-PeerID public-key="CAESII", sig="abc""#
        );
    }

    #[test]
    fn generate_challenge_is_32_bytes() {
        assert_eq!(
            BASE64
                .decode(PeerIdAuthClient::generate_challenge())
                .unwrap()
                .len(),
            32
        );
    }
}
