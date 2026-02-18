# Burrow

A decentralized, encrypted peer-to-peer chat application with a terminal user interface (TUI).

## Current Status: Phase 2 Complete ✓

**Phase 1: Foundation & Storage** ✅
- ✅ Core type definitions (PeerId, MessageId, ChannelId, Message, Channel)
- ✅ Vector Clock implementation for causal ordering
- ✅ SQLite storage layer with CRUD operations
- ✅ Basic TUI with channel management and messaging
- ✅ Local message persistence

**Phase 2: P2P Networking** ✅
- ✅ libp2p integration with TCP, Noise encryption, and Yamux multiplexing
- ✅ Gossipsub for message broadcasting between peers
- ✅ mDNS for automatic local peer discovery
- ✅ Manual peer connection via multiaddr (Ctrl+P)
- ✅ Message broadcast and reception
- ✅ Peer lifecycle management (connect/disconnect events)
- ✅ Network events integrated into TUI

## Quick Start

### Build and Run

```bash
cargo build --release
cargo run --release
```

### Using the Application

**First Run:**
- The app automatically creates a "me" channel (your personal space)
- This channel is selected by default

**Help Menu:**
- Press `Ctrl+H` to show the help menu with all keyboard shortcuts

**Creating Channels:**
1. Press `Ctrl+N` to open the "New Channel" dialog
2. Type the channel name
3. Press `Enter` to create (or `Esc` to cancel)
4. The new channel is automatically selected

**Selecting Channels:**
- Use `↑` and `↓` arrow keys to navigate between channels

**Sending Messages:**
1. Select a channel (or use the default "me" channel)
2. Type your message in the input box at the bottom
3. Press `Enter` to send

**Quitting:**
- Press `Ctrl+Q` or `Ctrl+C` to quit

**All Keyboard Shortcuts:**
- `Ctrl+H` - Show help menu
- `Ctrl+N` - Open new channel dialog
- `Ctrl+P` - Connect to peer (opens dialog with multiaddr input)
- `↑/↓` - Navigate channels
- `Enter` - Send message (or confirm in dialogs)
- `Esc` - Cancel dialog
- `Backspace` - Delete character
- `Ctrl+Q` or `Ctrl+C` - Quit

### Data Storage

**Messages** are stored in a SQLite database at:
- **Linux**: `~/.local/share/burrow/burrow.db`
- **macOS**: `~/Library/Application Support/burrow/burrow.db`
- **Windows**: `%LOCALAPPDATA%\burrow\burrow.db`

**Identity** (cryptographic keypair) is stored at:
- **Linux**: `~/.local/share/burrow/identity.key`
- **macOS**: `~/Library/Application Support/burrow/identity.key`
- **Windows**: `%LOCALAPPDATA%\burrow\identity.key`

Your identity file contains your Ed25519 keypair and is created on first run. This gives you a persistent peer ID across restarts. **Keep this file secure** - it's your cryptographic identity on the network.

### Logs

Application logs are written to a file (to avoid interfering with the TUI):
- **Linux**: `~/.local/share/burrow/burrow.log`
- **macOS**: `~/Library/Application Support/burrow/burrow.log`
- **Windows**: `%LOCALAPPDATA%\burrow\burrow.log`

You can tail the log file in another terminal to see what's happening:
```bash
tail -f ~/.local/share/burrow/burrow.log
```

### What's Working (Phases 1-2)

**Local Features:**
- ✅ Auto-created "me" channel on first run
- ✅ Modal dialog for creating new channels (Ctrl+N)
- ✅ Keyboard-driven channel navigation (↑/↓)
- ✅ Send messages to any channel
- ✅ Messages persist across restarts
- ✅ Vector clock increments with each message
- ✅ Messages ordered chronologically
- ✅ Help menu (Ctrl+H) with all shortcuts
- ✅ Clean, responsive TUI interface

**Networking Features:**
- ✅ P2P networking with libp2p (TCP + Noise + Yamux)
- ✅ Persistent cryptographic identity (Ed25519 keypair)
- ✅ Automatic peer discovery via mDNS on local network
- ✅ Manual peer connection via multiaddr (Ctrl+P)
- ✅ Real-time message broadcasting to connected peers
- ✅ Receive and store messages from other peers
- ✅ Status bar showing peer ID, listen addresses, and connected peers
- ✅ Gossipsub for efficient message distribution
- ✅ Logs written to file (not stdout) to avoid TUI interference

## Architecture

### Core Components

```
┌────────────────────────────────────────────────────────────┐
│                    TUI Layer (ratatui)                     │
│  - Channel list  - Message view  - Voice status  - Input  │
└─────────────────────────┬──────────────────────────────────┘
                          ↓
┌────────────────────────────────────────────────────────────┐
│                   Storage Layer (SQLite)                   │
│  - Messages  - Channels  - Keys  - Peers                  │
└────────────────────────────────────────────────────────────┘
```

### Core Types

- **PeerId**: Unique identifier for each peer (UUID v7)
- **ChannelId**: Unique identifier for channels
- **MessageId**: Time-ordered message identifier (UUID v7)
- **VectorClock**: Causal ordering metadata for messages
- **Message**: Message with content, author, timestamps, and ordering metadata
- **Channel**: Channel with name and metadata

### Message Ordering

Messages use a hybrid ordering system:
1. **Vector Clocks**: Track causal relationships between messages
2. **Lamport Timestamps**: Provide total ordering fallback
3. **UUID v7**: Time-ordered identifiers for display

## What's Next: Phase 2-7

### Phase 2: P2P Networking (Week 2-3)
- libp2p integration for peer-to-peer connections
- Manual peer connection via IP:port
- Message broadcast between peers
- Peer lifecycle management

### Phase 3: CRDT & State Sync (Week 3-4)
- OR-Set CRDT for channel membership
- LWW-Register for metadata
- Hybrid Logical Clocks
- Conflict resolution

### Phase 4: Message Ordering & Gossip (Week 4-5)
- Message DAG for handling concurrent messages
- Gossip protocol (rumor mongering + anti-entropy)
- Message deduplication
- Gap detection and recovery

### Phase 5: Encryption (Week 5-6)
- Signal Protocol Double Ratchet for 1-to-1 messaging
- Sender Keys for group messaging
- Epoch-based key rotation
- Trust verification

### Phase 6: Voice Chat (Week 6-8)
- Audio capture/playback with Opus codec
- SFU relay for voice forwarding
- SRTP encryption
- Relay selection and failover

### Phase 7: Polish & Production (Week 8-10)
- Comprehensive error handling
- Performance optimization
- Testing (unit, integration, chaos)
- Documentation

## Development

### Running Tests

```bash
cargo test
```

### Running with Debug Logging

To enable debug logging, set the `RUST_LOG` environment variable:

```bash
RUST_LOG=burrow=debug cargo run
```

Then in another terminal, tail the log file to see debug output:

```bash
tail -f ~/.local/share/burrow/burrow.log
```

### Project Structure

```
src/
├── main.rs           # Application entry point
├── types/
│   └── mod.rs       # Core type definitions
├── storage/
│   ├── mod.rs       # Storage implementation
│   └── schema.sql   # Database schema
├── identity/
│   └── mod.rs       # Persistent cryptographic identity (Phase 2)
├── network/         # P2P networking (Phase 2)
│   ├── mod.rs       # Network implementation
│   └── peer.rs      # Peer management
├── protocol/
│   └── mod.rs       # Network protocol messages (Phase 2)
└── tui/
    └── mod.rs       # Terminal UI

Future modules (Phases 3-7):
├── crdt/           # CRDT implementations (Phase 3)
├── gossip/         # Gossip protocol (Phase 4)
├── crypto/         # Encryption (Phase 5)
└── voice/          # Voice chat (Phase 6)
```

## Dependencies

### Core
- **tokio**: Async runtime
- **sqlx**: Async SQLite database
- **serde**: Serialization
- **uuid**: Time-ordered identifiers
- **anyhow**: Error handling
- **tracing**: Structured logging

### TUI
- **ratatui**: Terminal UI framework
- **crossterm**: Terminal manipulation

### Future (Phases 2-7)
- **libp2p**: P2P networking
- **x25519-dalek**: Key agreement
- **ed25519-dalek**: Digital signatures
- **chacha20poly1305**: Encryption
- **cpal**: Audio I/O
- **opus**: Audio codec

## Design Principles

1. **Decentralized**: No central servers, pure P2P
2. **Encrypted**: End-to-end encryption for all communication
3. **Eventually Consistent**: CRDT-based state with automatic conflict resolution
4. **Resilient**: Works offline, syncs when peers reconnect
5. **Privacy-First**: Local storage, minimal metadata leakage

## License

TBD

## Contributing

This project is currently in early development (Phase 1 complete). Contributions welcome once the architecture stabilizes in Phase 3-4.
