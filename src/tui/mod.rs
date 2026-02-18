use crate::network::{NetworkCommand, NetworkEvent};
use crate::network::peer::PeerManager;
use crate::storage::Storage;
use crate::types::{Channel, Message, MessageContent, PeerId, VectorClock};
use anyhow::Result;
use tokio::sync::mpsc;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use std::time::Duration;

enum AppMode {
    Normal,
    Help,
    NewChannel,
    ConnectPeer,
}

pub struct App {
    storage: Storage,
    peer_id: PeerId,
    libp2p_peer_id: libp2p::PeerId,
    channels: Vec<Channel>,
    selected_channel: Option<usize>,
    messages: Vec<Message>,
    input: String,
    lamport_clock: u64,
    vector_clock: VectorClock,
    channel_list_state: ListState,
    mode: AppMode,
    new_channel_input: String,
    connect_peer_input: String,
    network_event_rx: mpsc::UnboundedReceiver<NetworkEvent>,
    network_command_tx: mpsc::UnboundedSender<NetworkCommand>,
    peer_manager: PeerManager,
    listen_addrs: Vec<String>,
}

impl App {
    pub async fn new(
        storage: Storage,
        peer_id: PeerId,
        libp2p_peer_id: libp2p::PeerId,
        network_event_rx: mpsc::UnboundedReceiver<NetworkEvent>,
        network_command_tx: mpsc::UnboundedSender<NetworkCommand>,
    ) -> Result<Self> {
        let mut vector_clock = VectorClock::new();
        vector_clock.increment(peer_id);

        let mut channels = storage.get_all_channels().await?;

        // Create default "self" channel if no channels exist
        if channels.is_empty() {
            let self_channel = Channel::new("me".to_string());
            storage.store_channel(&self_channel).await?;
            channels = storage.get_all_channels().await?;
        }

        // Select the first channel by default
        let selected_channel = if !channels.is_empty() { Some(0) } else { None };
        let mut channel_list_state = ListState::default();
        if !channels.is_empty() {
            channel_list_state.select(Some(0));
        }

        // Load messages for the selected channel
        let messages = if let Some(idx) = selected_channel {
            if let Some(channel) = channels.get(idx) {
                storage.get_channel_messages(channel.id).await?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        Ok(Self {
            storage,
            peer_id,
            libp2p_peer_id,
            channels,
            selected_channel,
            messages,
            input: String::new(),
            lamport_clock: 0,
            vector_clock,
            channel_list_state,
            mode: AppMode::Normal,
            new_channel_input: String::new(),
            connect_peer_input: String::new(),
            network_event_rx,
            network_command_tx,
            peer_manager: PeerManager::new(),
            listen_addrs: Vec::new(),
        })
    }

    /// Run the TUI application
    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Run the app loop
        let result = self.run_loop(&mut terminal).await;

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    async fn run_loop<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()>
    where
        <B as ratatui::backend::Backend>::Error: Send + Sync + std::error::Error + 'static,
    {
        loop {
            terminal.draw(|f| self.ui(f))?;

            tokio::select! {
                // Handle keyboard input
                _ = tokio::time::sleep(Duration::from_millis(100)) => {
                    if event::poll(Duration::from_millis(0))? {
                        if let Event::Key(key) = event::read()? {
                            if key.kind == KeyEventKind::Press {
                                if self.handle_key_event(key).await? {
                                    break;
                                }
                            }
                        }
                    }
                }

                // Handle network events
                Some(network_event) = self.network_event_rx.recv() => {
                    self.handle_network_event(network_event).await?;
                }
            }
        }

        Ok(())
    }

    async fn handle_key_event(&mut self, key: KeyEvent) -> Result<bool> {
        match self.mode {
            AppMode::Help => {
                // Any key closes help
                self.mode = AppMode::Normal;
                return Ok(false);
            }
            AppMode::NewChannel => {
                return self.handle_new_channel_input(key).await;
            }
            AppMode::ConnectPeer => {
                return self.handle_connect_peer_input(key).await;
            }
            AppMode::Normal => {
                return self.handle_normal_input(key).await;
            }
        }
    }

    async fn handle_network_event(&mut self, event: NetworkEvent) -> Result<()> {
        match event {
            NetworkEvent::PeerConnected(peer_id) => {
                tracing::info!("Peer connected: {}", peer_id);
                self.peer_manager.add_peer(peer_id, None);
            }
            NetworkEvent::PeerDisconnected(peer_id) => {
                tracing::info!("Peer disconnected: {}", peer_id);
                self.peer_manager.remove_peer(&peer_id);
            }
            NetworkEvent::MessageReceived(message) => {
                tracing::info!("Message received: {:?}", message.id);
                // Store the message
                self.storage.store_message(&message).await?;

                // Update vector clock
                self.vector_clock.merge(&message.vector_clock);

                // Update lamport clock
                if message.lamport_timestamp >= self.lamport_clock {
                    self.lamport_clock = message.lamport_timestamp + 1;
                }

                // If it's for the currently selected channel, add it to the view
                if let Some(idx) = self.selected_channel {
                    if let Some(channel) = self.channels.get(idx) {
                        if message.channel_id == channel.id {
                            self.messages.push(message);
                        }
                    }
                }
            }
            NetworkEvent::ListeningOn(addr) => {
                tracing::info!("Listening on: {}", addr);
                self.listen_addrs.push(addr.to_string());
            }
        }

        Ok(())
    }

    async fn handle_normal_input(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char('q') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                return Ok(true); // Exit
            }
            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                return Ok(true); // Exit
            }
            KeyCode::Char('h') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                self.mode = AppMode::Help;
            }
            KeyCode::Char('n') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                // Open new channel modal
                self.mode = AppMode::NewChannel;
                self.new_channel_input.clear();
            }
            KeyCode::Char('p') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                // Open connect peer modal
                self.mode = AppMode::ConnectPeer;
                self.connect_peer_input.clear();
            }
            KeyCode::Up => {
                self.select_previous_channel().await?;
            }
            KeyCode::Down => {
                self.select_next_channel().await?;
            }
            KeyCode::Enter => {
                self.send_message().await?;
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
            }
            _ => {}
        }

        Ok(false)
    }

    async fn handle_new_channel_input(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                // Cancel channel creation
                self.mode = AppMode::Normal;
                self.new_channel_input.clear();
            }
            KeyCode::Enter => {
                // Create the channel
                if !self.new_channel_input.is_empty() {
                    self.create_channel_from_modal().await?;
                }
                self.mode = AppMode::Normal;
                self.new_channel_input.clear();
            }
            KeyCode::Backspace => {
                self.new_channel_input.pop();
            }
            KeyCode::Char(c) => {
                self.new_channel_input.push(c);
            }
            _ => {}
        }

        Ok(false)
    }

    async fn handle_connect_peer_input(&mut self, key: KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Esc => {
                // Cancel peer connection
                self.mode = AppMode::Normal;
                self.connect_peer_input.clear();
            }
            KeyCode::Enter => {
                // Connect to the peer
                if !self.connect_peer_input.is_empty() {
                    self.connect_to_peer().await?;
                }
                self.mode = AppMode::Normal;
                self.connect_peer_input.clear();
            }
            KeyCode::Backspace => {
                self.connect_peer_input.pop();
            }
            KeyCode::Char(c) => {
                self.connect_peer_input.push(c);
            }
            _ => {}
        }

        Ok(false)
    }

    async fn connect_to_peer(&mut self) -> Result<()> {
        // Parse the multiaddr and send connect command
        if let Ok(addr) = self.connect_peer_input.parse() {
            self.network_command_tx.send(NetworkCommand::ConnectToPeer(addr))?;
            tracing::info!("Connecting to peer at {}", self.connect_peer_input);
        } else {
            tracing::warn!("Invalid multiaddr: {}", self.connect_peer_input);
        }

        Ok(())
    }

    async fn create_channel_from_modal(&mut self) -> Result<()> {
        let channel = Channel::new(self.new_channel_input.clone());
        let channel_id = channel.id;
        self.storage.store_channel(&channel).await?;
        self.channels = self.storage.get_all_channels().await?;

        // Find and select the newly created channel
        if let Some(index) = self.channels.iter().position(|c| c.id == channel_id) {
            self.selected_channel = Some(index);
            self.channel_list_state.select(Some(index));
            self.load_messages().await?;
        }

        Ok(())
    }

    async fn select_next_channel(&mut self) -> Result<()> {
        if self.channels.is_empty() {
            return Ok(());
        }

        let next = match self.selected_channel {
            Some(i) => {
                if i >= self.channels.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };

        self.selected_channel = Some(next);
        self.channel_list_state.select(Some(next));
        self.load_messages().await?;

        Ok(())
    }

    async fn select_previous_channel(&mut self) -> Result<()> {
        if self.channels.is_empty() {
            return Ok(());
        }

        let prev = match self.selected_channel {
            Some(i) => {
                if i == 0 {
                    self.channels.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.channels.len() - 1,
        };

        self.selected_channel = Some(prev);
        self.channel_list_state.select(Some(prev));
        self.load_messages().await?;

        Ok(())
    }

    async fn load_messages(&mut self) -> Result<()> {
        if let Some(idx) = self.selected_channel {
            if let Some(channel) = self.channels.get(idx) {
                self.messages = self.storage.get_channel_messages(channel.id).await?;
            }
        }

        Ok(())
    }

    async fn send_message(&mut self) -> Result<()> {
        if self.input.is_empty() {
            return Ok(());
        }

        if let Some(idx) = self.selected_channel {
            if let Some(channel) = self.channels.get(idx) {
                // Increment clocks
                self.lamport_clock += 1;
                self.vector_clock.increment(self.peer_id);

                let message = Message::new(
                    channel.id,
                    self.peer_id,
                    MessageContent {
                        text: self.input.clone(),
                    },
                    self.vector_clock.clone(),
                    self.lamport_clock,
                );

                self.storage.store_message(&message).await?;
                self.messages.push(message.clone());

                // Broadcast to network
                self.network_command_tx.send(NetworkCommand::BroadcastMessage(message))?;

                self.input.clear();
            }
        }

        Ok(())
    }

    fn ui(&mut self, f: &mut Frame) {
        // Main layout: content area + status bar at bottom
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(f.area());

        // Content area with horizontal split
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(main_chunks[0]);

        // Left panel: channel list
        self.render_channel_list(f, chunks[0]);

        // Right panel: messages and input
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(chunks[1]);

        self.render_messages(f, right_chunks[0]);
        self.render_input(f, right_chunks[1]);

        // Status bar at bottom
        self.render_status_bar(f, main_chunks[1]);

        // Render modals on top
        match self.mode {
            AppMode::Help => {
                self.render_help(f, f.area());
            }
            AppMode::NewChannel => {
                self.render_new_channel_modal(f, f.area());
            }
            AppMode::ConnectPeer => {
                self.render_connect_peer_modal(f, f.area());
            }
            AppMode::Normal => {}
        }
    }

    fn render_channel_list(&mut self, f: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .channels
            .iter()
            .map(|channel| {
                let content = Line::from(vec![Span::styled(
                    format!("# {}", channel.name),
                    Style::default().fg(Color::White),
                )]);
                ListItem::new(content)
            })
            .collect();

        let peer_count = self.peer_manager.peer_count();
        let title = if peer_count > 0 {
            format!(" Channels ({} peers connected) ", peer_count)
        } else {
            " Channels (no peers) ".to_string()
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("► ");

        f.render_stateful_widget(list, area, &mut self.channel_list_state);
    }

    fn render_messages(&self, f: &mut Frame, area: Rect) {
        let channel_name = self
            .selected_channel
            .and_then(|idx| self.channels.get(idx))
            .map(|c| c.name.as_str())
            .unwrap_or("No channel selected");

        let messages: Vec<Line> = self
            .messages
            .iter()
            .map(|msg| {
                let is_own = msg.author == self.peer_id;
                let author_color = if is_own { Color::Green } else { Color::Blue };

                Line::from(vec![
                    Span::styled(
                        format!("[{}] ", msg.author.0.simple()),
                        Style::default().fg(author_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(&msg.content.text, Style::default().fg(Color::White)),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(messages)
            .block(
                Block::default()
                    .title(format!(" {} ", channel_name))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        f.render_widget(paragraph, area);
    }

    fn render_input(&self, f: &mut Frame, area: Rect) {
        let input_text = format!("> {}", self.input);

        let paragraph = Paragraph::new(input_text)
            .block(
                Block::default()
                    .title(" Input (Enter: send, Ctrl+H: help, Ctrl+Q: quit) ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .style(Style::default().fg(Color::White));

        f.render_widget(paragraph, area);
    }

    fn render_status_bar(&self, f: &mut Frame, area: Rect) {
        // Shorten peer ID for display (first 8 chars)
        let peer_id_str = self.libp2p_peer_id.to_string();
        let peer_id_short = if peer_id_str.len() > 12 {
            format!("{}...{}", &peer_id_str[..6], &peer_id_str[peer_id_str.len()-6..])
        } else {
            peer_id_str
        };

        // Get first listen address or show count
        let listen_info = if self.listen_addrs.is_empty() {
            "Starting...".to_string()
        } else if self.listen_addrs.len() == 1 {
            self.listen_addrs[0].clone()
        } else {
            format!("{} addresses", self.listen_addrs.len())
        };

        // Connected peers count
        let peer_count = self.peer_manager.peer_count();
        let peers_text = if peer_count == 1 {
            "1 peer".to_string()
        } else {
            format!("{} peers", peer_count)
        };

        let status_text = format!(
            " ID: {} | Listening: {} | Connected: {} ",
            peer_id_short, listen_info, peers_text
        );

        let status = Paragraph::new(status_text)
            .style(Style::default().bg(Color::DarkGray).fg(Color::White));

        f.render_widget(status, area);
    }

    fn render_new_channel_modal(&self, f: &mut Frame, area: Rect) {
        // Clear the entire screen to remove underlying UI
        f.render_widget(Clear, area);

        let text = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "Create New Channel",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from("Enter channel name:"),
            Line::from(""),
            Line::from(vec![
                Span::raw("> "),
                Span::styled(
                    &self.new_channel_input,
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled("_", Style::default().fg(Color::Gray)),
            ]),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(" to create  "),
                Span::styled("Esc", Style::default().fg(Color::Red)),
                Span::raw(" to cancel"),
            ]),
        ];

        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .title(" New Channel ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        // Center the modal
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Length(12),
                Constraint::Percentage(30),
            ])
            .split(area);

        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(60),
                Constraint::Percentage(20),
            ])
            .split(vertical_chunks[1]);

        f.render_widget(paragraph, horizontal_chunks[1]);
    }

    fn render_connect_peer_modal(&self, f: &mut Frame, area: Rect) {
        // Clear the entire screen to remove underlying UI
        f.render_widget(Clear, area);

        // Show listening addresses
        let listen_addrs_text = if !self.listen_addrs.is_empty() {
            format!("Listening on: {}", self.listen_addrs.join(", "))
        } else {
            "Starting network...".to_string()
        };

        let text = vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "Connect to Peer",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![Span::styled(
                &listen_addrs_text,
                Style::default().fg(Color::Gray),
            )]),
            Line::from(""),
            Line::from("Enter peer multiaddr (e.g., /ip4/192.168.1.5/tcp/9000):"),
            Line::from(""),
            Line::from(vec![
                Span::raw("> "),
                Span::styled(
                    &self.connect_peer_input,
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled("_", Style::default().fg(Color::Gray)),
            ]),
            Line::from(""),
            Line::from(""),
            Line::from(vec![
                Span::styled("Enter", Style::default().fg(Color::Green)),
                Span::raw(" to connect  "),
                Span::styled("Esc", Style::default().fg(Color::Red)),
                Span::raw(" to cancel"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                format!("Connected peers: {}", self.peer_manager.peer_count()),
                Style::default().fg(Color::Gray),
            )]),
        ];

        let paragraph = Paragraph::new(text)
            .block(
                Block::default()
                    .title(" Connect to Peer ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        // Center the modal
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Length(18),
                Constraint::Percentage(20),
            ])
            .split(area);

        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15),
                Constraint::Percentage(70),
                Constraint::Percentage(15),
            ])
            .split(vertical_chunks[1]);

        f.render_widget(paragraph, horizontal_chunks[1]);
    }

    fn render_help(&self, f: &mut Frame, area: Rect) {
        // Clear the entire screen to remove underlying UI
        f.render_widget(Clear, area);

        let help_text = vec![
            Line::from(vec![Span::styled(
                "Burrow - Keyboard Shortcuts",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Ctrl+H", Style::default().fg(Color::Yellow)),
                Span::raw("       Show this help menu"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Channel Management:",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("Ctrl+N", Style::default().fg(Color::Yellow)),
                Span::raw("       Create new channel (opens dialog)"),
            ]),
            Line::from(vec![
                Span::styled("↑/↓   ", Style::default().fg(Color::Yellow)),
                Span::raw("       Navigate between channels"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Networking:",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("Ctrl+P", Style::default().fg(Color::Yellow)),
                Span::raw("       Connect to peer (opens dialog)"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Messaging:",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("Enter ", Style::default().fg(Color::Yellow)),
                Span::raw("       Send message (when channel selected)"),
            ]),
            Line::from(vec![
                Span::styled("Type  ", Style::default().fg(Color::Yellow)),
                Span::raw("       Start typing to compose message"),
            ]),
            Line::from(vec![
                Span::styled("Bksp  ", Style::default().fg(Color::Yellow)),
                Span::raw("       Delete character"),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Application:",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("Ctrl+Q", Style::default().fg(Color::Yellow)),
                Span::raw("       Quit application"),
            ]),
            Line::from(vec![
                Span::styled("Ctrl+C", Style::default().fg(Color::Yellow)),
                Span::raw("       Quit application"),
            ]),
            Line::from(""),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Press any key to close this help menu",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::ITALIC),
            )]),
        ];

        let paragraph = Paragraph::new(help_text)
            .block(
                Block::default()
                    .title(" Help ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false });

        // Center the help in the middle of the screen
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(10),
                Constraint::Percentage(80),
                Constraint::Percentage(10),
            ])
            .split(area);

        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(15),
                Constraint::Percentage(70),
                Constraint::Percentage(15),
            ])
            .split(vertical_chunks[1]);

        f.render_widget(paragraph, horizontal_chunks[1]);
    }
}
