pub(crate) mod state;

/// Maximum length of a message.
///
/// "As long as message interleaving is not supported, the sender SHOULD limit the maximum message
/// size to 16 KB to avoid monopolization."
/// Source: <https://www.rfc-editor.org/rfc/rfc8831#name-transferring-user-data-on-a>
pub(crate) const MAX_MSG_LEN: usize = 16384; // 16kiB
/// Length of varint, in bytes.
pub(crate) const VARINT_LEN: usize = 2;
/// Overhead of the protobuf encoding, in bytes.
pub(crate) const PROTO_OVERHEAD: usize = 5;
/// Maximum length of data, in bytes.
pub(crate) const MAX_DATA_LEN: usize = MAX_MSG_LEN - VARINT_LEN - PROTO_OVERHEAD;
