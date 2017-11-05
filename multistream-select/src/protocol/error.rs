use std::io::Error as IoError;

/// Error at the multistream-select layer of communication.
#[derive(Debug)]
pub enum MultistreamSelectError {
	/// I/O error.
	IoError(IoError),

	/// The remote doesn't use the same multistream-select protocol as we do.
	FailedHandshake,

	/// Received an unknown message from the remote.
	UnknownMessage,

	/// Protocol names must always start with `/`, otherwise this error is returned.
	WrongProtocolName,
}

impl From<IoError> for MultistreamSelectError {
	#[inline]
	fn from(err: IoError) -> MultistreamSelectError {
		MultistreamSelectError::IoError(err)
	}
}
