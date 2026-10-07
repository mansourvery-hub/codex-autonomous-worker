use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::Clear,
    Frame,
};

pub struct PtySession {
    pub parser: Arc<Mutex<vt100::Parser>>,
    pub writer: Option<Box<dyn Write + Send>>,
    pub child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    pub master: Option<Box<dyn MasterPty + Send>>,
    pub active_task_id: Option<String>,
    pub current_rows: u16,
    pub current_cols: u16,
}

impl Default for PtySession {
    fn default() -> Self {
        Self::new(24, 80)
    }
}

impl PtySession {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 500))),
            writer: None,
            child: None,
            master: None,
            active_task_id: None,
            current_rows: rows,
            current_cols: cols,
        }
    }

    pub fn spawn(
        &mut self,
        task_id: String,
        program: &str,
        args: &[&str],
        cwd: &Path,
        rows: u16,
        cols: u16,
    ) -> Result<()> {
        self.kill();

        let target_rows = rows.max(10);
        let target_cols = cols.max(20);

        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: target_rows,
            cols: target_cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(target_rows, target_cols, 500)));
        let parser_clone = Arc::clone(&parser);

        let mut reader = pair.master.try_clone_reader()?;
        std::thread::spawn(move || {
            let mut buf = [0u8; 16384];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Ok(mut p) = parser_clone.lock() {
                    p.process(&buf[..n]);
                }
            }
        });

        let mut cmd = CommandBuilder::new(program);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.cwd(cwd);
        // Explicit terminal capabilities
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("LANG", "en_US.UTF-8");

        let child = pair.slave.spawn_command(cmd)?;
        let writer = pair.master.take_writer()?;

        self.parser = parser;
        self.writer = Some(writer);
        self.child = Some(child);
        self.master = Some(pair.master);
        self.active_task_id = Some(task_id);
        self.current_rows = target_rows;
        self.current_cols = target_cols;

        Ok(())
    }

    pub fn write_input(&mut self, bytes: &[u8]) -> Result<()> {
        if let Some(ref mut w) = self.writer {
            w.write_all(bytes)?;
            w.flush()?;
        }
        Ok(())
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if rows == 0 || cols == 0 {
            return;
        }
        // Deduplicate resize calls to avoid SIGWINCH redraw storm
        if rows == self.current_rows && cols == self.current_cols {
            return;
        }

        self.current_rows = rows;
        self.current_cols = cols;

        if let Some(ref mut m) = self.master {
            let _ = m.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
    }

    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_status)) => false,
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    pub fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
        }
        self.child = None;
        self.writer = None;
        self.master = None;
        self.active_task_id = None;
        self.current_rows = 0;
        self.current_cols = 0;
        self.parser = Arc::new(Mutex::new(vt100::Parser::new(24, 80, 500)));
    }

    pub fn render_screen(&self, frame: &mut Frame, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        // 1. Clear destination area so no characters from previous screens bleed through
        frame.render_widget(Clear, area);

        if let Ok(parser) = self.parser.lock() {
            let screen = parser.screen();
            let screen_size = screen.size();
            let max_r = (area.height as u16).min(screen_size.0);
            let max_c = (area.width as u16).min(screen_size.1);

            for r in 0..max_r {
                for c in 0..max_c {
                    let buf_cell = frame.buffer_mut().cell_mut((area.x + c, area.y + r)).unwrap();
                    if let Some(cell) = screen.cell(r, c) {
                        let contents = cell.contents();
                        // CRITICAL: Blank cells must be explicit whitespace " "
                        if contents.is_empty() {
                            buf_cell.set_symbol(" ");
                        } else {
                            buf_cell.set_symbol(&contents);
                        }

                        let mut style = Style::default();
                        if cell.bold() {
                            style = style.add_modifier(Modifier::BOLD);
                        }
                        if cell.italic() {
                            style = style.add_modifier(Modifier::ITALIC);
                        }
                        if cell.underline() {
                            style = style.add_modifier(Modifier::UNDERLINED);
                        }
                        if cell.inverse() {
                            style = style.add_modifier(Modifier::REVERSED);
                        }

                        match cell.fgcolor() {
                            vt100::Color::Default => {}
                            vt100::Color::Idx(i) => style = style.fg(Color::Indexed(i)),
                            vt100::Color::Rgb(red, green, blue) => {
                                style = style.fg(Color::Rgb(red, green, blue))
                            }
                        }

                        match cell.bgcolor() {
                            vt100::Color::Default => {}
                            vt100::Color::Idx(i) => style = style.bg(Color::Indexed(i)),
                            vt100::Color::Rgb(red, green, blue) => {
                                style = style.bg(Color::Rgb(red, green, blue))
                            }
                        }

                        buf_cell.set_style(style);
                    } else {
                        buf_cell.set_symbol(" ");
                        buf_cell.set_style(Style::default());
                    }
                }
            }

            // Synchronize terminal hardware cursor position if visible
            if !screen.hide_cursor() {
                let (cursor_r, cursor_c) = screen.cursor_position();
                if cursor_r < area.height && cursor_c < area.width {
                    frame.set_cursor_position((area.x + cursor_c, area.y + cursor_r));
                }
            }
        }
    }
}
