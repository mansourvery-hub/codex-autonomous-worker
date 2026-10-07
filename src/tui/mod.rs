pub mod app;
pub mod pty;
pub mod ui;

use std::io::{stdout, Stdout};
use std::time::Duration;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, MouseButton, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use crate::config::AppConfig;
use self::app::{App, FocusedPane};

pub fn run_tui(config: AppConfig) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config);
    let tick_rate = Duration::from_millis(100);

    let res = run_loop(&mut terminal, &mut app, tick_rate);

    // Clean terminal restoration
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    // Clean up PTY child if running
    app.pty.kill();

    res
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    tick_rate: Duration,
) -> Result<()> {
    let mut last_refresh = std::time::Instant::now();

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, app))?;

        let (term_cols, term_rows) = crossterm::terminal::size()?;
        // Right workspace rows/cols
        let workspace_w = term_cols.saturating_sub(34);
        let workspace_h = term_rows.saturating_sub(8);

        if event::poll(tick_rate)? {
            match event::read()? {
                Event::Key(key) => {
                    if app.modal.is_some() {
                        app.handle_modal_key(key);
                    } else if app.focused_pane == FocusedPane::Terminal {
                        match key.code {
                            KeyCode::F(6) | KeyCode::Tab => {
                                // Toggle focus back to sidebar
                                app.focused_pane = FocusedPane::Sidebar;
                            }
                            _ => {
                                forward_key_to_pty(&mut app.pty, key)?;
                            }
                        }
                    } else {
                        // Focused on Sidebar
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => {
                                app.should_quit = true;
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.should_quit = true;
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                app.next();
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                app.previous();
                            }
                            KeyCode::Char('r') => {
                                app.refresh();
                                app.set_message("Refreshed campaign queue.");
                            }
                            KeyCode::Char('n') => {
                                app.open_new_campaign_modal();
                            }
                            KeyCode::F(6) | KeyCode::Tab => {
                                if app.pty.is_running() {
                                    app.focused_pane = FocusedPane::Terminal;
                                }
                            }
                            KeyCode::Enter => {
                                app.launch_or_attach_pty(workspace_h, workspace_w);
                            }
                            _ => {}
                        }
                    }
                }
                Event::Mouse(mouse_event) => {
                    if app.modal.is_none() {
                        if let MouseEventKind::Down(MouseButton::Left) = mouse_event.kind {
                            let click_x = mouse_event.column;
                            let click_y = mouse_event.row;

                            // Sidebar is left 32 cols
                            if click_x <= 32 {
                                app.focused_pane = FocusedPane::Sidebar;
                                let start_y = 4;
                                let card_height = 4;
                                if click_y >= start_y {
                                    let clicked_idx = ((click_y - start_y) / card_height) as usize;
                                    if clicked_idx < app.campaigns.len() {
                                        app.selected_index = clicked_idx;
                                    }
                                }
                            } else {
                                // Clicked on right workspace -> focus terminal
                                if app.pty.is_running() {
                                    app.focused_pane = FocusedPane::Terminal;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Auto refresh state every 2 seconds
        if last_refresh.elapsed() >= Duration::from_secs(2) {
            app.refresh();
            last_refresh = std::time::Instant::now();
        }
    }

    Ok(())
}

fn forward_key_to_pty(pty: &mut crate::tui::pty::PtySession, key: crossterm::event::KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Char(c) => {
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                let byte = match c {
                    'c' => 0x03,
                    'd' => 0x04,
                    'z' => 0x1A,
                    'l' => 0x0C,
                    _ => (c as u8) & 0x1F,
                };
                pty.write_input(&[byte])?;
            } else {
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                pty.write_input(s.as_bytes())?;
            }
        }
        KeyCode::Enter => pty.write_input(&[13])?,
        KeyCode::Backspace => pty.write_input(&[0x7F])?,
        KeyCode::Up => pty.write_input(&[0x1B, b'[', b'A'])?,
        KeyCode::Down => pty.write_input(&[0x1B, b'[', b'B'])?,
        KeyCode::Right => pty.write_input(&[0x1B, b'[', b'C'])?,
        KeyCode::Left => pty.write_input(&[0x1B, b'[', b'D'])?,
        KeyCode::Home => pty.write_input(&[0x1B, b'[', b'H'])?,
        KeyCode::End => pty.write_input(&[0x1B, b'[', b'F'])?,
        KeyCode::PageUp => pty.write_input(&[0x1B, b'[', b'5', b'~'])?,
        KeyCode::PageDown => pty.write_input(&[0x1B, b'[', b'6', b'~'])?,
        KeyCode::Delete => pty.write_input(&[0x1B, b'[', b'3', b'~'])?,
        KeyCode::Esc => pty.write_input(&[0x1B])?,
        _ => {}
    }
    Ok(())
}
