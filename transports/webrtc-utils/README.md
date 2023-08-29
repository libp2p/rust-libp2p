# WebRTC Common Utilities

Tools that are common to more than one WebRTC transport:

```rust
// Protobuf Message framing and Flags
use libp2p_webrtc_utils::proto::{Flag, Message};

// Length constants
use libp2p_webrtc_utils::stream::{MAX_DATA_LEN, MAX_MSG_LEN, VARINT_LEN};

// Stream state machine
use libp2p_webrtc_utils::stream::state::{Closing, State};

// Noise Identity Fingerprinting utilities and SHA256 string constant
use libp2p_webrtc_utils::fingerprint::{Fingerprint, SHA256};

// Utility Error types
use libp2p_webrtc_utils::Error;

// Session Description Protocol ufrag generation and rendering
use libp2p_webrtc_utils::sdp::{random_ufrag, render_description};

// WebRTC Dial Address parsing
use libp2p_webrtc_utils::parse_webrtc_dial_addr;
```
