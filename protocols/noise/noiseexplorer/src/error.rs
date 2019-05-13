#[derive(Debug)]
pub enum NoiseError {
	DecryptionError,
	UnsupportedMessageLengthError,
	ExhaustedNonceError,
	InvalidKeyError,
	InvalidInputError,
	DerivePublicKeyFromEmptyKeyError,
	Hex(hex::FromHexError,),
}

impl std::fmt::Display for NoiseError {
	#[inline]
	fn fmt(&self, f: &mut std::fmt::Formatter,) -> std::fmt::Result {
		match *self {
			NoiseError::DecryptionError => write!(f, "Unsuccesful decryption."),
			NoiseError::UnsupportedMessageLengthError => write!(f, "Unsupported Message Length."),
			NoiseError::ExhaustedNonceError =>
				write!(f, "Reached maximum number of messages that can be sent for this session."),
			NoiseError::DerivePublicKeyFromEmptyKeyError => write!(f, "Unable to derive PublicKey"),
			NoiseError::InvalidKeyError => write!(f, "Invalid Key."),
			NoiseError::InvalidInputError => write!(f, "Invalid input length."),
			NoiseError::Hex(ref e,) => e.fmt(f,),
		}
	}
}

impl std::error::Error for NoiseError {
	fn description(&self,) -> &str {
		match *self {
			NoiseError::DecryptionError => "The sender was not authenticated.",
			NoiseError::UnsupportedMessageLengthError =>
				"The length of a transport message must be exclusively between 0 and 0xFFFF bytes.",
			NoiseError::ExhaustedNonceError =>
				"You must end terminate this session and start a new one.",
			NoiseError::DerivePublicKeyFromEmptyKeyError =>
				"It is forbidden to derive a PublicKey from an Empty PrivateKey",
			NoiseError::InvalidKeyError => "The key must be exactly 32 bytes of length.",
			NoiseError::InvalidInputError => "The input string exactly 32 bytes of length.",
			NoiseError::Hex(ref e,) => e.description(),
		}
	}
}

impl From<hex::FromHexError,> for NoiseError {
	fn from(err: hex::FromHexError,) -> NoiseError {
		NoiseError::Hex(err,)
	}
}
