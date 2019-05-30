#[derive(Debug)]
pub enum NoiseError {
	DecryptionError,
	UnsupportedMessageLengthError,
	ExhaustedNonceError,
	InvalidKeyError,
	InvalidPublicKeyError,
	EmptyKeyError,
	InvalidInputError,
	DerivePublicKeyFromEmptyKeyError,
	Hex(hex::FromHexError),
	MissingnsError,
	MissingneError,
	MissingHsMacError,
	MissingrsError,
	MissingreError
}

impl std::fmt::Display for NoiseError {
	#[inline]
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		match *self {
			NoiseError::DecryptionError => write!(f, "Unsuccesful decryption."),
			NoiseError::UnsupportedMessageLengthError => write!(f, "Unsupported Message Length."),
			NoiseError::ExhaustedNonceError => write!(f, "Reached maximum number of messages that can be sent for this session."),
			NoiseError::DerivePublicKeyFromEmptyKeyError => write!(f, "Unable to derive PublicKey."),
			NoiseError::InvalidKeyError => write!(f, "Invalid Key."),
			NoiseError::InvalidPublicKeyError => write!(f, "Invalid Public Key."),
			NoiseError::EmptyKeyError => write!(f, "Empty Key."),
			NoiseError::InvalidInputError => write!(f, "Invalid input length."),
			NoiseError::MissingnsError => write!(f, "Invalid message length."),
			NoiseError::MissingHsMacError => write!(f, "Invalid message length."),
			NoiseError::MissingneError => write!(f, "Invalid message length."),
			NoiseError::MissingrsError => write!(f, "Invalid message length."),
			NoiseError::MissingreError => write!(f, "Invalid message length."),
			NoiseError::Hex(ref e) => e.fmt(f),
		}
	}
}

impl std::error::Error for NoiseError {
	fn description(&self) -> &str {
		match *self {
			NoiseError::DecryptionError => "The sender was not authenticated.",
			NoiseError::UnsupportedMessageLengthError => "The length of a transport message must be exclusively between 0 and 0xFFFF bytes.",
			NoiseError::ExhaustedNonceError => "You must end terminate this session and start a new one.",
			NoiseError::DerivePublicKeyFromEmptyKeyError => "It is forbidden to derive a PublicKey from an Empty PrivateKey",
			NoiseError::InvalidKeyError => "The key must be exactly 32 bytes of length.",
			NoiseError::InvalidPublicKeyError => "The public key must be derived using a Curve25519 operation.",
			NoiseError::EmptyKeyError => "The key must not be empty.",
			NoiseError::InvalidInputError => "The input string exactly 32 bytes of length.",
			NoiseError::MissingnsError => "You have not allocated enough space for ns in your handshake message.",
			NoiseError::MissingneError => "You have not allocated enough space for ne in your handshake message.",
			NoiseError::MissingrsError => "You have not included rs in your handshake message.",
			NoiseError::MissingHsMacError => "You have not included the MAC place holder for either the ciphertext or rs in one of your handshake messages.",
			NoiseError::MissingreError => "You have not included re in your handshake message.",
			NoiseError::Hex(ref e) => e.description(), 
		} 
	}
}

impl From<hex::FromHexError> for NoiseError {
	fn from(err: hex::FromHexError) -> NoiseError {
		NoiseError::Hex(err)
	}
}
