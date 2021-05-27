use crate::identity::error::SigningError;
use crate::identity::Keypair;
use crate::PublicKey;
use prost::bytes::BufMut;
use unsigned_varint::encode::usize_buffer;

// TODO: docs
#[derive(Debug, Clone)]
pub struct SignedEnvelope {
    key: PublicKey,
    payload_type: Vec<u8>,
    payload: Vec<u8>,
    signature: Vec<u8>,
}

impl SignedEnvelope {
    // TODO: docs
    pub fn new(
        key: Keypair,
        domain_separation: String,
        payload_type: Vec<u8>,
        payload: Vec<u8>,
    ) -> Result<Self, SigningError> {
        // TODO: fix duplication

        let mut domain_sep_length_buffer = usize_buffer();
        let domain_sep_length =
            unsigned_varint::encode::usize(domain_separation.len(), &mut domain_sep_length_buffer);

        let mut payload_type_length_buffer = usize_buffer();
        let payload_type_length =
            unsigned_varint::encode::usize(payload_type.len(), &mut payload_type_length_buffer);

        let mut payload_length_buffer = usize_buffer();
        let payload_length =
            unsigned_varint::encode::usize(payload.len(), &mut payload_length_buffer);

        let mut buffer = Vec::with_capacity(
            domain_sep_length.len()
                + domain_separation.len()
                + payload_type_length.len()
                + payload_type.len()
                + payload_length.len()
                + payload.len(),
        );

        buffer.put(domain_sep_length);
        buffer.put(domain_separation.as_bytes());
        buffer.put(payload_type_length);
        buffer.put(payload_type.as_slice());
        buffer.put(payload_length);
        buffer.put(payload.as_slice());

        let signature = key.sign(&buffer)?;

        Ok(Self {
            key: key.public(),
            payload_type,
            payload,
            signature,
        })
    }

    #[must_use]
    pub fn verify(&self, domain_separation: String) -> bool {
        let mut domain_sep_length_buffer = usize_buffer();
        let domain_sep_length =
            unsigned_varint::encode::usize(domain_separation.len(), &mut domain_sep_length_buffer);

        let mut payload_type_length_buffer = usize_buffer();
        let payload_type_length = unsigned_varint::encode::usize(
            self.payload_type.len(),
            &mut payload_type_length_buffer,
        );

        let mut payload_length_buffer = usize_buffer();
        let payload_length =
            unsigned_varint::encode::usize(self.payload.len(), &mut payload_length_buffer);

        let mut buffer = Vec::with_capacity(
            domain_sep_length.len()
                + domain_separation.len()
                + payload_type_length.len()
                + self.payload_type.len()
                + payload_length.len()
                + self.payload.len(),
        );

        buffer.put(domain_sep_length);
        buffer.put(domain_separation.as_bytes());
        buffer.put(payload_type_length);
        buffer.put(self.payload_type.as_slice());
        buffer.put(payload_length);
        buffer.put(self.payload.as_slice());

        self.key.verify(&buffer, &self.signature)
    }

    // TODO: docs
    pub fn payload(
        &self,
        domain_separation: String,
        expected_payload_type: &[u8],
    ) -> Result<&[u8], ()> {
        if &self.payload_type != expected_payload_type {
            panic!("bad payload type") // TODO: error handling
        }

        if !self.verify(domain_separation) {
            panic!("bad signature") // TODO: error handling
        }

        Ok(&self.payload)
    }

    // TODO: Do we need this?
    // pub fn payload_unchecked(&self) -> Vec<u8> {
    //
    // }
}
