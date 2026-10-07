use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};
use crate::campaign::CampaignStatus;
use crate::tui::app::{App, FocusedPane, ModalStep};

pub fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
        ])
        .split(size);

    draw_top_banner(frame, app, chunks[0]);
    draw_main_split(frame, app, chunks[1]);
    draw_footer(frame, app, chunks[2]);

    if let Some(ref modal) = app.modal {
        draw_modal(frame, modal, size);
    }
}

fn draw_top_banner(frame: &mut Frame, app: &App, area: Rect) {
    let running_cnt = app.campaigns.iter().filter(|c| c.status == CampaignStatus::Running).count();
    let queued_cnt = app.campaigns.iter().filter(|c| c.status == CampaignStatus::Pending || c.status == CampaignStatus::Claimed).count();
    let done_cnt = app.campaigns.iter().filter(|c| c.status == CampaignStatus::Done).count();

    let (status_text, status_color) = if running_cnt > 0 {
        ("[● 24/7 DAEMON: ACTIVE]", Color::Green)
    } else {
        ("[○ DAEMON: STANDBY]", Color::Yellow)
    };

    let title_line = Line::from(vec![
        Span::styled(" AUTOPILOT CONTROL DECK ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" │ "),
        Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        Span::raw(" │ "),
        Span::styled(format!("Loop: {} Active · {} Queued · {} Completed", running_cnt, queued_cnt, done_cnt), Style::default().fg(Color::White)),
    ]);

    let banner = Paragraph::new(title_line)
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(Color::DarkGray)));

    frame.render_widget(banner, area);
}

fn draw_main_split(frame: &mut Frame, app: &mut App, area: Rect) {
    let sidebar_w = 32.min(area.width.saturating_sub(40));
    let split_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(sidebar_w),
            Constraint::Min(40),
        ])
        .split(area);

    draw_sidebar(frame, app, split_chunks[0]);
    draw_workspace(frame, app, split_chunks[1]);
}

fn draw_sidebar(frame: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focused_pane == FocusedPane::Sidebar;
    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };

    let items: Vec<ListItem> = app.campaigns.iter().enumerate().map(|(idx, c)| {
        let is_selected = idx == app.selected_index;

        let (pill_text, pill_color) = match c.status {
            CampaignStatus::Running => ("[● WRK]", Color::Green),
            CampaignStatus::Claimed => ("[▶ CLM]", Color::Yellow),
            CampaignStatus::Pending => ("[○ QUD]", Color::Magenta),
            CampaignStatus::Done => ("[✔ DON]", Color::Green),
            CampaignStatus::Failed => ("[✖ ERR]", Color::Red),
        };

        let prefix = if is_selected { "▸ " } else { "  " };

        let line1 = Line::from(vec![
            Span::styled(prefix, Style::default().fg(if is_selected { Color::Cyan } else { Color::DarkGray })),
            Span::styled(pill_text, Style::default().fg(pill_color).add_modifier(Modifier::BOLD)),
            Span::styled(format!(" #{} · {}", c.id, c.age), Style::default().fg(Color::White)),
        ]);

        let line2 = Line::from(vec![
            Span::raw("  "),
            Span::styled(c.prompt.clone(), Style::default().fg(if is_selected { Color::White } else { Color::Gray }).add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() })),
        ]);

        let mode_label = if c.mode == "continuous" { "⟳ Loop" } else { "⊡ Task" };
        let line3_text = if c.status == CampaignStatus::Running {
            format!("  {} · {} #{}/{}", c.repo, mode_label, c.iteration, c.max_iterations)
        } else {
            format!("  {} · {} ({} iters)", c.repo, mode_label, c.max_iterations)
        };
        let line3 = Line::from(vec![
            Span::styled(line3_text, Style::default().fg(Color::DarkGray)),
        ]);

        let line4 = Line::from(Span::styled("  ──────────────────────────────", Style::default().fg(Color::Rgb(40, 40, 50))));

        let mut item = ListItem::new(vec![line1, line2, line3, line4]);
        if is_selected {
            item = item.style(Style::default().bg(Color::Rgb(30, 32, 48)));
        }
        item
    }).collect();

    let title = format!(" Campaigns ({}) ", app.campaigns.len());
    let list_widget = List::new(items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(title, Style::default().fg(if is_focused { Color::Cyan } else { Color::Gray }).add_modifier(Modifier::BOLD)))
            .border_style(Style::default().fg(border_color)));

    frame.render_widget(list_widget, area);
}

fn draw_workspace(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focused_pane == FocusedPane::Terminal;
    let border_color = if is_focused { Color::Green } else { Color::DarkGray };

    let pty_has_session = app.pty.active_task_id.is_some();

    let title = if is_focused {
        " Codex Live Workspace [FOCUSED - F6/Tab to exit] "
    } else if pty_has_session {
        " Codex Live Workspace [Enter to focus & talk] "
    } else {
        " Codex Workspace [Standing by] "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, Style::default().fg(if is_focused { Color::Green } else { Color::Cyan }).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // If PTY has active session, render the real virtual screen
    if pty_has_session {
        app.pty.render_screen(frame, inner);
        return;
    }

    // Otherwise show selected campaign info card (e.g. for pending tasks)
    if let Some(c) = app.selected_campaign() {
        let (status_text, status_color) = match c.status {
            CampaignStatus::Running => ("RUNNING (Live session ready in codex-live)", Color::Green),
            CampaignStatus::Claimed => ("CLAIMED (Worktree being provisioned)", Color::Yellow),
            CampaignStatus::Pending => ("QUEUED (Waiting for active campaign to finish or be stopped)", Color::Magenta),
            CampaignStatus::Done => ("COMPLETED", Color::Green),
            CampaignStatus::Failed => ("FAILED", Color::Red),
        };

        let mode_desc = if c.mode == "continuous" {
            format!("⟳ 24/7 Continuous Loop (budget: {} iterations)", c.max_iterations)
        } else {
            "⊡ Single-Turn Task (1 turn quick fix)".to_string()
        };

        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled(format!("Campaign #{}: ", c.id), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(c.prompt.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled("Status:      ", Style::default().fg(Color::Gray)),
            Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Mode:        ", Style::default().fg(Color::Gray)),
            Span::styled(mode_desc, Style::default().fg(Color::Yellow)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Repository:  ", Style::default().fg(Color::Gray)),
            Span::styled(c.repo.clone(), Style::default().fg(Color::White)),
            Span::raw("    "),
            Span::styled("Branch: ", Style::default().fg(Color::Gray)),
            Span::styled(c.branch.clone(), Style::default().fg(Color::Yellow)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Model:       ", Style::default().fg(Color::Gray)),
            Span::styled(c.model.clone(), Style::default().fg(Color::Cyan)),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("─── Queue Details ────────────────────────────────────────────────", Style::default().fg(Color::DarkGray))));

        if c.status == CampaignStatus::Pending {
            lines.push(Line::from(Span::styled("This campaign is queued waiting for the current active campaign to finish.", Style::default().fg(Color::Magenta))));
            lines.push(Line::from(Span::styled("To stop the running campaign and start this one now, select the running task and press [x].", Style::default().fg(Color::Gray))));
        } else {
            lines.push(Line::from(Span::styled("Session recorded. Press [Enter] to open.", Style::default().fg(Color::Cyan))));
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    } else {
        let empty_msg = Paragraph::new("No campaigns available. Press [n] to launch a new 24/7 autonomous loop.")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(Alignment::Center);
        frame.render_widget(empty_msg, inner);
    }
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let footer_text = if let Some((ref msg, instant)) = app.message {
        if instant.elapsed().as_secs() < 3 {
            Line::from(Span::styled(format!(" ★ {}", msg), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
        } else {
            default_footer_line(app)
        }
    } else {
        default_footer_line(app)
    };

    let footer = Paragraph::new(footer_text)
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray)));

    frame.render_widget(footer, area);
}

fn default_footer_line(app: &App) -> Line<'static> {
    if app.focused_pane == FocusedPane::Terminal {
        Line::from(vec![
            Span::styled(" [F6 / Tab / Alt-←] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Focus Sidebar  "),
            Span::styled("[Esc] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Interrupt Codex  "),
            Span::styled("[Type] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Steer Live  "),
        ])
    } else {
        Line::from(vec![
            Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Talk to Codex  "),
            Span::styled("[n] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("New (Loop/Task)  "),
            Span::styled("[x] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Stop Active  "),
            Span::styled("[r] ", Style::default().fg(Color::Cyan)),
            Span::raw("Refresh  "),
            Span::styled("[q] ", Style::default().fg(Color::Red)),
            Span::raw("Quit"),
        ])
    }
}

fn draw_modal(frame: &mut Frame, modal: &crate::tui::app::ModalState, screen: Rect) {
    let width = 74.min(screen.width.saturating_sub(6));
    let height = 12.min(screen.height.saturating_sub(4));
    let x = (screen.width.saturating_sub(width)) / 2;
    let y = (screen.height.saturating_sub(height)) / 2;
    let modal_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, modal_area);

    match modal.step {
        ModalStep::SelectRepo => {
            let items: Vec<ListItem> = modal.repos.iter().enumerate().map(|(idx, r)| {
                let is_sel = idx == modal.selected_repo_idx;
                let prefix = if is_sel { "▸ " } else { "  " };
                let mut item = ListItem::new(format!("{}{}", prefix, r));
                if is_sel {
                    item = item.style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Rgb(30, 32, 48)));
                }
                item
            }).collect();

            let list = List::new(items)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title(" New Campaign [1/4]: Select Target Repository (Enter to confirm, Esc to cancel) ")
                    .border_style(Style::default().fg(Color::Cyan)));

            frame.render_widget(list, modal_area);
        }
        ModalStep::SelectMode => {
            let items: Vec<ListItem> = modal.modes.iter().enumerate().map(|(idx, m)| {
                let is_sel = idx == modal.selected_mode_idx;
                let prefix = if is_sel { "▸ " } else { "  " };
                let mut item = ListItem::new(format!("{}{}", prefix, m.label));
                if is_sel {
                    item = item.style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Rgb(30, 32, 48)));
                }
                item
            }).collect();

            let list = List::new(items)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title(" New Campaign [2/4]: Select Execution Mode (Loop vs Single Task) ")
                    .border_style(Style::default().fg(Color::Cyan)));

            frame.render_widget(list, modal_area);
        }
        ModalStep::SelectModel => {
            let items: Vec<ListItem> = modal.models.iter().enumerate().map(|(idx, m)| {
                let is_sel = idx == modal.selected_model_idx;
                let prefix = if is_sel { "▸ " } else { "  " };
                let mut item = ListItem::new(format!("{}{}", prefix, m.label));
                if is_sel {
                    item = item.style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Rgb(30, 32, 48)));
                }
                item
            }).collect();

            let list = List::new(items)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title(" New Campaign [3/4]: Select Model ")
                    .border_style(Style::default().fg(Color::Cyan)));

            frame.render_widget(list, modal_area);
        }
        ModalStep::EnterPrompt => {
            let mode_tag = if modal.selected_mode().id == "continuous" { "24/7 Loop (30 iters)" } else { "Single Task" };
            let title = format!(" New Campaign [4/4]: [{}] · {} ", modal.selected_repo(), mode_tag);

            let block = Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan));

            let inner = block.inner(modal_area);
            frame.render_widget(block, modal_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(2), Constraint::Length(1)])
                .split(inner);

            let label = Paragraph::new(format!("Model: {} │ Type objective:", modal.selected_model().id))
                .style(Style::default().fg(Color::Gray));
            frame.render_widget(label, chunks[0]);

            let input = Paragraph::new(modal.prompt_buffer.as_str())
                .style(Style::default().fg(Color::White).bg(Color::Rgb(30, 32, 48)))
                .wrap(Wrap { trim: false });
            frame.render_widget(input, chunks[1]);

            let hint = Paragraph::new("[Enter] Launch Campaign    [Esc] Cancel")
                .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
            frame.render_widget(hint, chunks[2]);
        }
    }
}
