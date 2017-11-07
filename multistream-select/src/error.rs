use protocol::MultistreamSelectError;
use std::io::Error as IoError;

/// Error that can happen when negociating a protocol with the remote.
#[derive(Debug)]
pub enum ProtocolChoiceError {
	/// Error in the protocol.
	MultistreamSelectError(MultistreamSelectError),

	/// Received a message from the remote that makes no sense in the current context.
	UnexpectedMessage,

	/// We don't support any protocol in common with the remote.
	NoProtocolFound,
}

impl From<MultistreamSelectError> for ProtocolChoiceError {
	#[inline]
	fn from(err: MultistreamSelectError) -> ProtocolChoiceError {
		ProtocolChoiceError::MultistreamSelectError(err)
	}
}

impl From<IoError> for ProtocolChoiceError {
	#[inline]
	fn from(err: IoError) -> ProtocolChoiceError {
		MultistreamSelectError::from(err).into()
	}
}
