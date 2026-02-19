# Burrow

**A decentralized peer-to-peer chat application for the terminal.**

Burrow is a privacy-focused, serverless messaging platform that runs entirely in your terminal. Built on libp2p, it enables direct peer-to-peer communication without relying on central servers. Messages and channels synchronize automatically using conflict-free replicated data types (CRDTs), ensuring eventual consistency across all peers.

## Features

- **Decentralized Architecture**: No servers, no accounts - just peer-to-peer communication
- **Automatic Peer Discovery**: Find peers on your local network automatically via mDNS
- **Persistent Identity**: Ed25519 cryptographic keypair gives you a stable identity across sessions
- **Channel Synchronization**: Create channels that automatically sync with connected peers
- **Conflict-Free Updates**: CRDT-based state ensures channels converge without conflicts
- **Terminal Interface**: Clean, keyboard-driven TUI built with ratatui
- **Local Storage**: All messages and channels stored locally in SQLite
- **Message Ordering**: Vector clocks and Lamport timestamps maintain causal ordering

## Installation

### Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs))

### Build from Source

```bash
git clone https://github.com/yourusername/burrow.git
cd burrow
cargo build --release
```

The binary will be available at `target/release/burrow`.

### Run

```bash
cargo run --release
```

Or install to your system:

```bash
cargo install --path .
burrow
```

## Usage

### Getting Started

On first launch, Burrow will:
1. Generate your cryptographic identity (Ed25519 keypair)
2. Create a default "me" channel for personal notes
3. Start listening for peer connections on port 9000
4. Begin discovering peers on your local network

### Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+H` | Show help menu |
| `Ctrl+N` | Create new channel |
| `Ctrl+P` | Connect to peer manually |
| `↑` / `↓` | Navigate between channels |
| `Enter` | Send message / Confirm dialog |
| `Esc` | Cancel dialog |
| `Ctrl+Q` / `Ctrl+C` | Quit application |

### Creating Channels

1. Press `Ctrl+N` to open the new channel dialog
2. Type your channel name
3. Press `Enter` to create

The channel will be automatically announced to all connected peers.

### Connecting to Peers

**Automatic (mDNS):**
Burrow automatically discovers and connects to peers on your local network.

**Manual Connection:**
1. Press `Ctrl+P` to open the connect dialog
2. Enter the peer's multiaddress (e.g., `/ip4/192.168.1.100/tcp/9000`)
3. Press `Enter` to connect

You can find your own listen addresses in the status bar at the top of the screen.

### Sending Messages

1. Use `↑` / `↓` to select a channel
2. Type your message in the input box at the bottom
3. Press `Enter` to send

Messages are broadcast to all connected peers and stored locally.

## Configuration

### Data Storage Locations

Burrow stores all data locally on your machine:

| Platform | Location |
|----------|----------|
| **Linux** | `~/.local/share/burrow/` |
| **macOS** | `~/Library/Application Support/burrow/` |
| **Windows** | `%LOCALAPPDATA%\burrow\` |

Files in this directory:
- `identity.key` - Your Ed25519 keypair (keep this secure!)
- `burrow.db` - SQLite database containing messages and channels
- `burrow.log` - Application logs

### Port Configuration

By default, Burrow listens on port 9000. To use a different port:

```bash
BURROW_PORT=9001 burrow
```

### Logging

To enable debug logging:

```bash
RUST_LOG=burrow=debug burrow
```

View logs in real-time:

```bash
# Linux/macOS
tail -f ~/.local/share/burrow/burrow.log

# Windows
Get-Content "$env:LOCALAPPDATA\burrow\burrow.log" -Wait
```

## How It Works

### Architecture

```
┌─────────────────────────────────────────────────┐
│           Terminal UI (ratatui)                 │
│  Channels │ Messages │ Input │ Status           │
├─────────────────────────────────────────────────┤
│              Storage (SQLite)                   │
│  Messages, Channels, CRDT State                 │
├─────────────────────────────────────────────────┤
│            Network (libp2p)                     │
│  TCP + Noise + Yamux + Gossipsub + mDNS         │
└─────────────────────────────────────────────────┘
```

### CRDT-Based Synchronization

Burrow uses Conflict-free Replicated Data Types (CRDTs) to ensure all peers converge to the same state:

- **LWW-Register**: Channel names automatically resolve to the most recent update
- **OR-Set**: Channel membership merges additions and removals without conflicts
- **Hybrid Logical Clocks**: Track causality while maintaining physical time awareness

When peers reconnect, channel states automatically merge and converge without manual conflict resolution.

### Message Ordering

Messages maintain causal ordering using:
- **Vector Clocks**: Capture happens-before relationships between messages
- **Lamport Timestamps**: Provide total ordering when concurrent
- **UUID v7**: Time-based identifiers for consistent display order

## Roadmap

### Current Status: Phase 5 (Storage) Complete ✓

- [x] **Phase 1**: Foundation - Storage, types, basic TUI
- [x] **Phase 2**: Networking - P2P connections, message broadcast
- [x] **Phase 3**: CRDTs - Channel synchronization, conflict-free state
- [x] **Phase 4**: Message DAG - Causal message ordering and gossip protocol
- [~] **Phase 5**: Encryption - Signal Protocol storage layer complete, message encryption pending
- [ ] **Phase 6**: Voice - Encrypted voice chat with relay support
- [ ] **Phase 7**: Polish - Testing, optimization, documentation

See [ARCHITECTURE.md](ARCHITECTURE.md) for detailed design documentation.

## Development

### Running Tests

```bash
cargo test
```

### Project Structure

```
src/
├── main.rs         # Application entry point
├── types/          # Core type definitions
├── storage/        # SQLite persistence layer
├── identity/       # Cryptographic identity management
├── network/        # libp2p networking
├── protocol/       # Network message protocol
├── crdt/           # CRDT implementations
├── dag/            # Message DAG and gossip protocol
├── encryption/     # Signal Protocol storage and session management
└── tui/            # Terminal user interface
```

### Key Dependencies

- **tokio** - Async runtime
- **libp2p** - P2P networking stack
- **sqlx** - Async SQLite database
- **ratatui** - Terminal UI framework
- **libsignal-protocol** - Signal Protocol implementation for E2E encryption
- **serde** / **bincode** - Serialization
- **uuid** - Time-ordered identifiers

## Design Philosophy

**Decentralized** - No servers, no accounts. Pure peer-to-peer architecture.

**Eventually Consistent** - CRDTs ensure all peers converge to the same state without coordination.

**Privacy-First** - All data stored locally. No telemetry, no tracking, no cloud.

**Offline-Capable** - Works without internet. Syncs automatically when peers reconnect.

**Terminal-Native** - Lightweight, keyboard-driven interface for power users.

## Security Considerations

**Current Status:**
- Network transport encrypted with Noise protocol (libp2p default)
- Persistent Ed25519 identity for peer authentication
- Signal Protocol storage layer implemented (identity keys, prekeys, sessions, sender keys)
- Message content currently transmitted in plaintext

**In Progress (Phase 5):**
- End-to-end encryption with Signal Protocol
- X3DH key exchange for initial peer connections
- Perfect forward secrecy via Double Ratchet
- Group messaging with Sender Keys

## Contributing

Burrow is in active development. Contributions, bug reports, and feature requests are welcome!

### Getting Involved

1. Check existing [issues](https://github.com/yourusername/burrow/issues)
2. Fork the repository
3. Create a feature branch
4. Submit a pull request

## License

This project is licensed under the GNU Affero General Public License v3.0 or later - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

Built with:
- [libp2p](https://libp2p.io/) - Modular peer-to-peer networking stack
- [ratatui](https://ratatui.rs/) - Terminal UI framework
- [SQLx](https://github.com/launchbadge/sqlx) - Async SQL toolkit
