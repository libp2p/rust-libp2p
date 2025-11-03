# libp2p Bluetooth Transport

A Bluetooth Low Energy (BLE) transport implementation for libp2p with dual-role support.

## Features

- **Central (Client) Role**: Full support for scanning and connecting to BLE peripherals via SimpleBLE
- **Peripheral (Server) Role**: Full support for advertising and accepting connections (macOS via CoreBluetooth, Linux/Windows in development)
- **Dual-Role Support**: Can run both central and peripheral simultaneously for peer-to-peer connectivity
- **Cross-platform Central**: Works on macOS, Linux, and Windows via SimpleBLE
- **Frame-based Communication**: Automatic message framing for reliable data transfer
- **Integrated with libp2p**: Full support for Noise encryption, Yamux multiplexing, and all libp2p protocols

## Current Status

✅ **Central Mode**: Fully working on all platforms
✅ **Peripheral Mode**: Fully working on macOS (95% complete)
🔧 **Peripheral Mode**: Linux/Windows support in development
✅ **Dual-Role**: Fully working on macOS

## Testing with Two Machines

The transport supports dual-role operation - each peer can simultaneously act as both central and peripheral. This allows for true peer-to-peer connectivity over Bluetooth.

**On macOS**, both machines can run the same application and discover each other automatically.

### Service UUIDs

The transport uses these UUIDs:

- **Service UUID**: `00001234-0000-1000-8000-00805f9b34fb`
- **RX Characteristic**: `00001235-0000-1000-8000-00805f9b34fb` (receives data from central)
- **TX Characteristic**: `00001236-0000-1000-8000-00805f9b34fb` (sends data to central)

### Setup Instructions (macOS Dual-Role)

#### Machine A

```bash
cd /path/to/rust-libp2p-bluetooth-test
RUST_LOG=info,libp2p_bluetooth=debug cargo run --release
```

The application will:
- Start advertising as a peripheral
- Start scanning as a central
- Display its peer address to share with peers

#### Machine B

```bash
cd /path/to/rust-libp2p-bluetooth-test
# Use the peer address from Machine A if you want to dial explicitly
RUST_LOG=info,libp2p_bluetooth=debug cargo run --release -- \
  --peer /bluetooth/<machine-a-addr>/p2p/<machine-a-peer-id>
```

The application will:
- Start advertising as a peripheral
- Start scanning as a central
- Discover and connect to Machine A

### Testing Flow

1. Start both applications
2. Watch the logs for discovery and connection:
   ```
   INFO  Local peer id: 12D3KooW...
   INFO  Listening on /bluetooth/... (share this with peers: /bluetooth/.../p2p/12D3KooW...)
   INFO  Started advertising successfully
   INFO  Peripheral manager powered on, starting setup
   INFO  Service added successfully: 00001234-0000-1000-8000-00805f9b34fb
   INFO  Found peripheral: <peer-id>
   INFO  Connection established with 12D3KooW...
   INFO  Peer 12D3KooW... subscribed to bluetooth-chat
   ```

3. Type messages and press Enter to send them via Gossipsub
4. See messages from the peer displayed in the terminal

### Troubleshooting

**"No BLE adapters found"**
- Ensure your machine has Bluetooth capability
- On Linux, check `bluetoothctl` is working
- On macOS, check Bluetooth is enabled in System Settings
- Grant Bluetooth permissions to your terminal application

**"No peripherals found"**
- Ensure the other peer is running with Bluetooth enabled
- Check both peers are using the same service UUID
- Verify Bluetooth permissions are granted on both machines
- Try restarting both peers
- Increase the scan window if needed by setting `LIBP2P_BLUETOOTH_SCAN_TIMEOUT_SECS` (seconds) or `LIBP2P_BLUETOOTH_SCAN_COLLECTION_MS` (milliseconds)

**"Connection failed"**
- Check Bluetooth signal strength (peers need to be within range)
- Verify the service UUID matches on both peers
- Ensure characteristics have correct properties (read, write, notify)
- Check system Bluetooth is not busy with other connections

## Architecture

### Dual-Role Design

```
┌─────────────────────────────────────┐
│   BluetoothTransport (Dual-Role)   │
├─────────────────────────────────────┤
│                                     │
│  ┌────────────┐   ┌──────────────┐ │
│  │  Central   │   │  Peripheral  │ │
│  │ (SimpleBLE)│   │(CoreBluetooth)│ │
│  └──────┬─────┘   └──────┬───────┘ │
│         │                │         │
│    Scan/Connect    Advertise/Accept│
│         │                │         │
└─────────┼────────────────┼─────────┘
          │                │
          ▼                ▼
     libp2p Swarm + Protocols
```

Each peer runs both roles simultaneously:
- **Central role** (SimpleBLE): Scans for and connects to other BLE peripherals
- **Peripheral role** (CoreBluetooth on macOS): Advertises and accepts incoming connections
- **libp2p layer**: Handles peer identity, security (Noise), multiplexing (Yamux), and application protocols

### Framing Layer

BLE has limited MTU sizes (typically 23-512 bytes). This transport uses a simple length-prefix framing protocol:

- 4-byte big-endian length prefix
- Maximum frame size: 1MB
- Automatic frame assembly from BLE notification chunks
- Transparent fragmentation and reassembly

### Connection Flow (Dual-Role)

```
     Peer A                         Peer B
(Central+Peripheral)          (Central+Peripheral)
       |                               |
       |-- Advertise -------->         |
       |         <-------- Advertise --|
       |                               |
       |-- Scan discovers B -->        |
       |         <-- Scan discovers A -|
       |                               |
       |-- Connect as Central -------->|
       |         (B acts as Peripheral)|
       |<- Connection Established -----|
       |                               |
       |-- GATT Service Discovery ---->|
       |<- Characteristics Info -------|
       |                               |
       |-- Subscribe to notifications->|
       |                               |
       |<==== Bidirectional Data =====>|
       |   (libp2p protocols over BLE) |
```

## Implementation Details

- **Transport Type**: Implements `libp2p_core::Transport`
- **Channel Type**: `RwStreamSink<Chan<Vec<u8>>>`
- **Central Library**: SimpleBLE (simplersble v0.10) - cross-platform
- **Peripheral Library**: CoreBluetooth (objc2-core-bluetooth v0.2) - macOS only
- **Async Runtime**: Tokio
- **Security**: Noise protocol (XX handshake)
- **Multiplexing**: Yamux

## Current Limitations

1. **Peripheral mode**: macOS only (CoreBluetooth implementation)
2. **Threading**: Peripheral operations must run on main thread (CoreBluetooth requirement)
3. **MTU**: Fixed at 512 bytes, no automatic negotiation
4. **Reconnection**: Manual restart required on disconnect
5. **Multiplexing**: Single connection per peripheral instance

## Future Enhancements

- [ ] Automatic reconnection logic
- [ ] MTU negotiation for better throughput
- [ ] Linux peripheral support (BlueZ D-Bus API)
- [ ] Windows peripheral support (Windows.Devices.Bluetooth)
- [ ] Multi-connection support for peripherals
- [ ] Proper dispatch queue integration for peripheral operations
- [ ] Background operation support

## License

MIT

## Contributing

Contributions welcome! The transport is functional but needs testing and improvements:

1. Test on different platforms and hardware
2. Help implement peripheral mode for Linux (BlueZ) and Windows
3. Improve connection stability and reconnection logic
4. Add MTU negotiation
5. Optimize throughput and latency
6. Report issues with detailed logs
