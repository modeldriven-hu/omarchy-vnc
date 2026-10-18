//! Shared terminal connection manager. Toolkit-specific viewers live in sibling crates.
mod editor;
use anyhow::{Context, Result, bail};
use crossterm::{
    cursor::{Hide, Show},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use omarchy_vnc_core::{Model, Palette, Profile, Session, Status};
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::{
    io::{self, IsTerminal},
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

type Terminal = ratatui::Terminal<CrosstermBackend<io::Stdout>>;
/// Direct desktop launches get Omarchy's selected terminal; existing terminals are reused.
pub fn ensure_terminal() -> Result<bool> {
    if io::stdin().is_terminal() && io::stdout().is_terminal() {
        return Ok(true);
    }
    let status = Command::new("omarchy")
        .args(["launch", "tui", "--app-id=local.omarchy-vnc.manager"])
        .arg(std::env::current_exe()?)
        .args(std::env::args_os().skip(1))
        .status()
        .context(
            "Open this application in a terminal, or pass --gui for the graphical connection form",
        )?;
    if !status.success() {
        bail!("The terminal launcher failed; run this command inside a terminal or use --gui")
    }
    Ok(false)
}
struct Screen;
impl Screen {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let screen = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            Hide
        )?;
        Ok(screen)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            ratatui::crossterm::terminal::Clear(ratatui::crossterm::terminal::ClearType::All),
            DisableBracketedPaste,
            LeaveAlternateScreen,
            Show
        );
        let _ = disable_raw_mode();
    }
}
#[derive(Clone, Copy, PartialEq)]
enum Focus {
    Profiles,
    Form,
}
struct Manager<'a> {
    model: &'a mut Model,
    password: Zeroizing<String>,
    port: String,
    focus: Focus,
    field: usize,
    cursor: usize,
    list: ListState,
    password_visible: bool,
    confirm_delete: bool,
    before_edit: Profile,
    connecting: Option<Session>,
    started: Instant,
    notice: String,
    frontend: &'a str,
}
/// Returns only a live connected session with a framebuffer. Errors stay in the terminal manager.
pub fn connect(model: &mut Model, frontend: &str) -> Result<Option<Session>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("The connection manager needs a terminal")
    }
    let _screen = Screen::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut manager = Manager::new(model, frontend);
    loop {
        if let Some(session) = &manager.connecting {
            let snapshot = session.snapshot();
            match snapshot.status {
                Status::Connected if snapshot.frame.is_some() => {
                    return Ok(manager.connecting.take());
                }
                Status::Failed(error) => {
                    manager.model.error = error;
                    manager.connecting = None;
                }
                Status::Disconnected => {
                    manager.notice = "Connection closed before the viewer opened".into();
                    manager.connecting = None;
                }
                _ => {}
            }
        }
        terminal.draw(|frame| manager.draw(frame))?;
        let timeout = if manager.connecting.is_some() {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(60)
        };
        if !event::poll(timeout)? {
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if manager.key(key) {
                    return Ok(None);
                }
            }
            Event::Paste(text)
                if manager.connecting.is_none()
                    && manager.focus == Focus::Form
                    && manager.field < 5 =>
            {
                for c in text.chars().filter(|c| !c.is_control()) {
                    manager.edit(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
                }
            }
            _ => {}
        }
    }
}
impl<'a> Manager<'a> {
    fn new(model: &'a mut Model, frontend: &'a str) -> Self {
        let selected = model.selected;
        let field = if selected.is_some() {
            4
        } else if model.draft.host.is_empty() {
            0
        } else if model.draft.username.is_empty() {
            3
        } else {
            4
        };
        let mut manager = Self {
            port: model.draft.port.to_string(),
            before_edit: model.draft.clone(),
            focus: if selected.is_some() {
                Focus::Profiles
            } else {
                Focus::Form
            },
            model,
            password: Zeroizing::new(String::new()),
            field,
            cursor: 0,
            list: ListState::default().with_selected(selected),
            password_visible: false,
            confirm_delete: false,
            connecting: None,
            started: Instant::now(),
            notice: "Ready · VNC session traffic is unencrypted".into(),
            frontend,
        };
        manager.cursor = manager.field_text().len();
        manager
    }
    fn field_text(&self) -> &str {
        match self.field {
            0 => &self.model.draft.name,
            1 => &self.model.draft.host,
            2 => &self.port,
            3 => &self.model.draft.username,
            4 => &self.password,
            _ => "",
        }
    }
    fn text_mut(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.model.draft.name,
            1 => &mut self.model.draft.host,
            2 => &mut self.port,
            3 => &mut self.model.draft.username,
            4 => &mut self.password,
            _ => unreachable!(),
        }
    }
    fn edit(&mut self, key: KeyEvent) {
        let mut cursor = self.cursor;
        let field = self.field;
        let limit = match field {
            0 => 128,
            1 => 253,
            2 => 5,
            _ => 127,
        };
        if let KeyCode::Char(c) = key.code
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && self.field_text().len() + c.len_utf8() > limit
        {
            return;
        }
        editor::edit(self.text_mut(), &mut cursor, key);
        self.cursor = cursor;
    }
    fn focus_field(&mut self, field: usize) {
        self.focus = Focus::Form;
        self.field = field;
        self.cursor = self.field_text().len();
    }
    fn sync_port(&mut self) -> Result<()> {
        self.model.draft.port = self
            .port
            .parse::<u16>()
            .context("Port must be between 1 and 65535")?;
        self.model.draft.validate()
    }
    fn outcome(&mut self, result: Result<()>, message: &str) {
        match result {
            Ok(()) => {
                self.model.error.clear();
                self.notice = message.into();
                self.before_edit = self.model.draft.clone();
                self.list.select(self.model.selected);
            }
            Err(e) => self.model.error = e.to_string(),
        }
    }
    fn start(&mut self) {
        if let Err(e) = self.sync_port() {
            self.model.error = e.to_string();
            let invalid =
                if self.model.draft.name.trim().is_empty() || self.model.draft.name.len() > 128 {
                    0
                } else if self.port.parse::<u16>().is_err() || self.port == "0" {
                    2
                } else if self.model.draft.username.len() > 127
                    || self.model.draft.username.contains('\0')
                {
                    3
                } else {
                    1
                };
            self.focus_field(invalid);
            return;
        }
        if !self.model.draft.username.is_empty() && self.password.is_empty() {
            self.notice = "Enter the Mac account password, then press Enter".into();
            self.model.error.clear();
            self.focus_field(4);
            return;
        }
        self.password_visible = false;
        let password = std::mem::take(&mut *self.password);
        match Session::connect(self.model.draft.clone(), password, Arc::new(|| {})) {
            Ok(session) => {
                self.connecting = Some(session);
                self.started = Instant::now();
                self.model.error.clear();
                self.notice.clear();
            }
            Err(e) => self.model.error = e.to_string(),
        }
    }
    fn select(&mut self, index: usize) {
        self.model.select(index);
        self.port = self.model.draft.port.to_string();
        self.before_edit = self.model.draft.clone();
        self.password.zeroize();
        self.password_visible = false;
        self.list.select(Some(index));
    }
    fn key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('q' | 'c')) {
            return true;
        }
        if self.connecting.is_some() {
            if key.code == KeyCode::Esc {
                self.connecting = None;
                self.notice = "Connection cancelled".into();
            }
            return false;
        }
        if self.confirm_delete {
            match key.code {
                KeyCode::Char('y' | 'Y') => {
                    let result = self.model.delete();
                    self.outcome(result, "Profile removed");
                    self.port = self.model.draft.port.to_string();
                    self.confirm_delete = false;
                }
                KeyCode::Char('n' | 'N') | KeyCode::Esc => self.confirm_delete = false,
                _ => {}
            }
            return false;
        }
        match key.code {
            KeyCode::F(2) => self.password_visible = !self.password_visible,
            KeyCode::F(5) => self.start(),
            KeyCode::F(6) => {
                let result = self.sync_port().and_then(|_| self.model.save());
                self.outcome(result, "Profile saved");
            }
            KeyCode::F(7) => {
                let result = self.model.make_default();
                self.outcome(result, "Default profile updated");
            }
            KeyCode::F(8) => self.confirm_delete = self.model.selected.is_some(),
            KeyCode::F(9) => {
                let result = self.model.reload();
                self.outcome(result, "Profiles reloaded");
                self.port = self.model.draft.port.to_string();
                self.password.zeroize();
                self.password_visible = false;
            }
            KeyCode::Tab => {
                if self.focus == Focus::Profiles {
                    self.focus_field(0);
                } else if self.field < 7 {
                    self.focus_field(self.field + 1);
                } else {
                    self.focus = Focus::Profiles;
                }
            }
            KeyCode::BackTab => {
                if self.focus == Focus::Profiles {
                    self.focus_field(7);
                } else if self.field > 0 {
                    self.focus_field(self.field - 1);
                } else {
                    self.focus = Focus::Profiles;
                }
            }
            KeyCode::Esc => {
                self.model.draft = self.before_edit.clone();
                self.port = self.model.draft.port.to_string();
                self.password.zeroize();
                self.password_visible = false;
                self.focus = Focus::Profiles;
                self.model.error.clear();
            }
            KeyCode::Up | KeyCode::Down if self.focus == Focus::Profiles => {
                let n = self.model.profiles.profiles.len();
                if n > 0 {
                    let current = self.list.selected().unwrap_or(0);
                    let next = if key.code == KeyCode::Down {
                        (current + 1) % n
                    } else {
                        (current + n - 1) % n
                    };
                    self.select(next);
                }
            }
            KeyCode::Up if self.field > 0 => self.focus_field(self.field - 1),
            KeyCode::Down if self.field < 7 => self.focus_field(self.field + 1),
            KeyCode::Char('n') if self.focus == Focus::Profiles => {
                self.model.new_profile();
                self.port = "5900".into();
                self.password.zeroize();
                self.before_edit = self.model.draft.clone();
                self.list.select(None);
                self.focus_field(0);
            }
            KeyCode::Char('e') if self.focus == Focus::Profiles => self.focus_field(0),
            KeyCode::Delete if self.focus == Focus::Profiles => {
                self.confirm_delete = self.model.selected.is_some()
            }
            KeyCode::Enter if self.focus == Focus::Profiles => self.start(),
            KeyCode::Enter if self.field == 4 => self.start(),
            KeyCode::Enter | KeyCode::Char(' ') if self.focus == Focus::Form && self.field >= 5 => {
                match self.field {
                    5 => self.model.draft.fullscreen = !self.model.draft.fullscreen,
                    6 => {
                        self.model.draft.map_ctrl_to_command = !self.model.draft.map_ctrl_to_command
                    }
                    7 => self.model.draft.clipboard = !self.model.draft.clipboard,
                    _ => {}
                }
            }
            KeyCode::Enter if self.field < 7 => self.focus_field(self.field + 1),
            _ if self.focus == Focus::Form && self.field < 5 => self.edit(key),
            _ => {}
        }
        false
    }
    fn draw(&mut self, frame: &mut Frame) {
        let palette = Palette::load();
        let color = |c: [u8; 3]| Color::Rgb(c[0], c[1], c[2]);
        let bg = color(palette.background);
        let fg = color(palette.foreground);
        let accent = color(palette.accent);
        let base = Style::default().bg(bg).fg(fg);
        let muted = Style::default().fg(fg).add_modifier(Modifier::DIM);
        let bounds = frame.area();
        frame.render_widget(Block::default().style(base), bounds);
        if bounds.width < 60 || bounds.height < 24 {
            frame.render_widget(Paragraph::new("OMARCHY VNC\n\nResize the terminal to at least 60 × 24.\nCtrl+Q exits; a connection can still be cancelled with Esc.").block(Block::bordered().border_type(BorderType::Rounded)).wrap(Wrap{trim:false}),bounds);
            return;
        }
        let panel = Rect::new(
            bounds.x + (bounds.width - bounds.width.min(110)) / 2,
            bounds.y + (bounds.height - bounds.height.min(28)) / 2,
            bounds.width.min(110),
            bounds.height.min(28),
        );
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(if self.model.error.is_empty() { 3 } else { 5 }),
            Constraint::Length(3),
        ])
        .split(panel);
        let header = Line::from(vec![
            Span::styled(
                "  O M A R C H Y   V N C",
                Style::default().fg(accent).bold(),
            ),
            Span::styled(format!("     {} viewer", self.frontend), muted),
        ]);
        frame.render_widget(
            Paragraph::new(header).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(accent)),
            ),
            rows[0],
        );
        let columns =
            Layout::horizontal([Constraint::Length(26), Constraint::Min(0)]).split(rows[1]);
        let items: Vec<_> = self
            .model
            .profiles
            .profiles
            .iter()
            .map(|p| {
                ListItem::new(format!(
                    "{}{}",
                    if self.model.profiles.default.as_ref() == Some(&p.name) {
                        "★ "
                    } else {
                        ""
                    },
                    clean(&p.name)
                ))
            })
            .collect();
        let list = List::new(items)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .title(" Connections ")
                    .border_style(if self.focus == Focus::Profiles {
                        Style::default().fg(accent)
                    } else {
                        muted
                    }),
            )
            .highlight_style(Style::default().bg(accent).fg(bg).bold())
            .highlight_symbol("› ");
        frame.render_stateful_widget(list, columns[0], &mut self.list);
        if self.model.profiles.profiles.is_empty() {
            let area = columns[0].inner(Margin::new(2, 2));
            frame.render_widget(
                Paragraph::new(
                    "No saved connections\n\nTab to edit\nF6 to save\nn for a new profile",
                )
                .style(muted),
                area,
            );
        }
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(" Connection ")
            .border_style(if self.focus == Focus::Form {
                Style::default().fg(accent)
            } else {
                muted
            });
        let area = block.inner(columns[1]);
        frame.render_widget(block, columns[1]);
        let names = [
            "Name",
            "Host",
            "Port",
            "Username",
            "Password",
            "Fullscreen",
            "Ctrl → Command",
            "Text clipboard",
        ];
        for (index, name) in names.iter().enumerate() {
            let y = area.y + index as u16 + 1 + u16::from(index >= 5);
            if y >= area.bottom() {
                break;
            }
            let row = Rect::new(area.x + 1, y, area.width.saturating_sub(2), 1);
            let active =
                self.focus == Focus::Form && self.field == index && self.connecting.is_none();
            let label_width = 15u16.min(row.width);
            let text_width = row.width.saturating_sub(label_width) as usize;
            let full_text = match index {
                0 => self.model.draft.name.clone(),
                1 => self.model.draft.host.clone(),
                2 => self.port.clone(),
                3 => self.model.draft.username.clone(),
                4 => {
                    if self.password_visible {
                        self.password.to_string()
                    } else {
                        "•".repeat(self.password.chars().count())
                    }
                }
                5 => if self.model.draft.fullscreen {
                    "[x]"
                } else {
                    "[ ]"
                }
                .into(),
                6 => if self.model.draft.map_ctrl_to_command {
                    "[x]"
                } else {
                    "[ ]"
                }
                .into(),
                _ => if self.model.draft.clipboard {
                    "[x]"
                } else {
                    "[ ]"
                }
                .into(),
            };
            let (text, cursor) = if active && index < 5 {
                editor::visible(
                    self.field_text(),
                    self.cursor,
                    text_width,
                    index == 4 && !self.password_visible,
                )
            } else {
                (clean(&full_text), 0)
            };
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        format!("{name:<15}"),
                        if active {
                            Style::default().fg(accent).bold()
                        } else {
                            muted
                        },
                    ),
                    Span::styled(
                        text,
                        if active {
                            Style::default().bg(accent).fg(bg)
                        } else {
                            base
                        },
                    ),
                ])),
                row,
            );
            if active && index < 5 && row.width > label_width {
                frame.set_cursor_position((row.x + label_width + cursor as u16, y));
            }
        }
        if area.height > 13 {
            frame.render_widget(Paragraph::new("Enter connects a saved profile. Tab edits fields.\nSpace toggles options; F2 reveals the password.").style(muted).wrap(Wrap{trim:false}),Rect::new(area.x+1,area.y+11,area.width.saturating_sub(2),area.height.saturating_sub(11)));
        }
        let message = if self.connecting.is_some() {
            let glyphs = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
            format!(
                "{} Connecting to {}:{} · Esc cancels",
                glyphs[(self.started.elapsed().as_millis() / 100) as usize % glyphs.len()],
                self.model.draft.host,
                self.model.draft.port
            )
        } else if !self.model.error.is_empty() {
            clean(&self.model.error)
        } else {
            self.notice.clone()
        };
        frame.render_widget(
            Paragraph::new(message)
                .style(if self.model.error.is_empty() {
                    Style::default().fg(accent)
                } else {
                    Style::default().fg(Color::LightRed)
                })
                .wrap(Wrap { trim: false })
                .block(
                    Block::bordered()
                        .border_type(BorderType::Rounded)
                        .title(" Status "),
                ),
            rows[2],
        );
        let key_style = Style::default().bg(accent).fg(bg).bold();
        let shortcuts = [
            vec![
                ("Tab/↑↓", "Navigate"),
                ("n", "New"),
                ("e", "Edit"),
                ("F2", "Password"),
            ],
            vec![
                ("F5", "Connect"),
                ("F6", "Save"),
                ("F7", "Default"),
                ("F8", "Delete"),
            ],
            vec![("F9", "Reload"), ("Esc", "Cancel"), ("Ctrl+Q", "Quit")],
        ];
        let lines: Vec<_> = shortcuts
            .into_iter()
            .map(|row| {
                Line::from(
                    row.into_iter()
                        .flat_map(|(key, label)| {
                            [
                                Span::styled(format!(" {key} "), key_style),
                                Span::styled(format!(" {label} "), muted),
                            ]
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        frame.render_widget(Paragraph::new(lines), rows[3]);
        if self.confirm_delete {
            let width = 50.min(bounds.width);
            let area = Rect::new((bounds.width - width) / 2, bounds.height / 2 - 2, width, 5);
            frame.render_widget(Clear, area);
            frame.render_widget(
                Paragraph::new(
                    "Remove this profile from both frontends?\n\ny: delete     n / Esc: keep",
                )
                .style(base)
                .block(
                    Block::bordered()
                        .title(" Delete connection ")
                        .border_style(Style::default().fg(accent)),
                ),
                area,
            );
        }
    }
}
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn model() -> Model {
        Model {
            store: omarchy_vnc_core::Store {
                directory: std::path::PathBuf::from("/unused"),
            },
            profiles: Default::default(),
            selected: None,
            draft: Profile {
                host: "example.local".into(),
                ..Default::default()
            },
            error: String::new(),
        }
    }
    #[test]
    fn render_masking_and_small_terminal() {
        let mut m = model();
        let mut manager = Manager::new(&mut m, "egui");
        manager.password.push_str("never-print-me");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 28)).unwrap();
        terminal.draw(|f| manager.draw(f)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(!output.contains("never-print-me"));
        manager.password_visible = true;
        terminal.draw(|f| manager.draw(f)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(output.contains("never-print-me"));
        let mut small = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 10)).unwrap();
        small.draw(|f| manager.draw(f)).unwrap();
    }
    #[test]
    fn edit_cancel_restores_profile_and_hides_secret() {
        let mut m = model();
        let old = m.draft.clone();
        let mut manager = Manager::new(&mut m, "gtk");
        manager.focus_field(1);
        manager.edit(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        manager.password.push_str("secret");
        manager.password_visible = true;
        manager.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(manager.model.draft, old);
        assert!(manager.password.is_empty());
        assert!(!manager.password_visible);
    }
    #[test]
    fn apple_profile_requests_password_before_connecting() {
        let mut m = model();
        m.draft.username = "sample".into();
        let mut manager = Manager::new(&mut m, "egui");
        manager.start();
        assert!(manager.connecting.is_none());
        assert_eq!(manager.field, 4);
        assert!(manager.notice.contains("password"));
    }
    #[test]
    fn full_field_can_be_cleared_with_ctrl_u() {
        let mut m = model();
        let mut manager = Manager::new(&mut m, "egui");
        manager.focus_field(4);
        manager.password.push_str(&"x".repeat(127));
        manager.cursor = 127;
        manager.edit(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(manager.password.is_empty());
    }
}
