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
        Span::styled(format!("Parallel: {} Active · {} Queued · {} Completed Tasks", running_cnt, queued_cnt, done_cnt), Style::default().fg(Color::White)),
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

    let total_tasks = app.campaigns.len();
    let visible_cards = (area.height.saturating_sub(2) / 4).max(1) as usize;

    let mut scroll_offset = app.scroll_offset;
    if app.selected_index >= scroll_offset + visible_cards {
        scroll_offset = app.selected_index.saturating_sub(visible_cards - 1);
    } else if app.selected_index < scroll_offset {
        scroll_offset = app.selected_index;
    }
    if scroll_offset >= total_tasks {
        scroll_offset = total_tasks.saturating_sub(visible_cards);
    }

    let end_idx = (scroll_offset + visible_cards).min(total_tasks);
    let visible_slice = if total_tasks > 0 { &app.campaigns[scroll_offset..end_idx] } else { &[] };

    let items: Vec<ListItem> = visible_slice.iter().enumerate().map(|(slice_idx, c)| {
        let real_idx = scroll_offset + slice_idx;
        let is_selected = real_idx == app.selected_index;

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
            Span::styled(format!(" Task #{} · {}", c.id, c.age), Style::default().fg(Color::White)),
        ]);

        let line2 = Line::from(vec![
            Span::raw("  "),
            Span::styled(c.prompt.clone(), Style::default().fg(if is_selected { Color::White } else { Color::Gray }).add_modifier(if is_selected { Modifier::BOLD } else { Modifier::empty() })),
        ]);

        let mode_label = if c.mode == "continuous" { "⟳ Loop" } else { "⊡ Task" };
        let line3_text = if c.status == CampaignStatus::Running {
            format!("  {} · {} · {} #{}/{}", c.repo, c.agent, mode_label, c.iteration, c.max_iterations)
        } else {
            format!("  {} · {} · {} ({} iters)", c.repo, c.agent, mode_label, c.max_iterations)
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

    let scroll_tag = if total_tasks > visible_cards {
        format!(" Tasks ({}) [{}/{}] ▲▼ ", total_tasks, app.selected_index + 1, total_tasks)
    } else {
        format!(" Tasks ({}) ", total_tasks)
    };

    let list_widget = List::new(items)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(scroll_tag, Style::default().fg(if is_focused { Color::Cyan } else { Color::Gray }).add_modifier(Modifier::BOLD)))
            .border_style(Style::default().fg(border_color)));

    frame.render_widget(list_widget, area);
}

fn draw_workspace(frame: &mut Frame, app: &mut App, area: Rect) {
    let is_focused = app.focused_pane == FocusedPane::Terminal;
    let border_color = if is_focused { Color::Green } else { Color::DarkGray };

    let pty_has_session = app.pty.active_task_id.is_some();

    let title = if is_focused {
        " Live Workspace [FOCUSED - F6/Tab to exit] "
    } else if pty_has_session {
        " Live Workspace [Enter to focus & talk] "
    } else {
        " Workspace [Standing by] "
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

    // Otherwise show selected campaign info card
    if let Some(c) = app.selected_campaign() {
        let (status_text, status_color) = match c.status {
            CampaignStatus::Running => ("RUNNING (Parallel session active)", Color::Green),
            CampaignStatus::Claimed => ("CLAIMED (Worktree being provisioned)", Color::Yellow),
            CampaignStatus::Pending => ("QUEUED (Ready for parallel execution)", Color::Magenta),
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
            Span::styled(format!("Task #{}: ", c.id), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(c.prompt.clone(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));

        lines.push(Line::from(vec![
            Span::styled("Status:      ", Style::default().fg(Color::Gray)),
            Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Engine:      ", Style::default().fg(Color::Gray)),
            Span::styled(c.agent.to_uppercase(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("    "),
            Span::styled("Mode: ", Style::default().fg(Color::Gray)),
            Span::styled(mode_desc, Style::default().fg(Color::Yellow)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("Repository:  ", Style::default().fg(Color::Gray)),
            Span::styled(c.repo.clone(), Style::default().fg(Color::White)),
            Span::raw("    "),
            Span::styled("Branch: ", Style::default().fg(Color::Gray)),
            Span::styled(c.branch.clone(), Style::default().fg(Color::Yellow)),
        ]));
        if c.agent != "opencode" {
            lines.push(Line::from(vec![
                Span::styled("Model:       ", Style::default().fg(Color::Gray)),
                Span::styled(c.model.clone(), Style::default().fg(Color::Cyan)),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("─── Status & Details ─────────────────────────────────────────────", Style::default().fg(Color::DarkGray))));

        if c.status == CampaignStatus::Pending {
            lines.push(Line::from(Span::styled("This task is queued and ready for parallel execution.", Style::default().fg(Color::Magenta))));
            lines.push(Line::from(Span::styled("The daemon executes queued tasks concurrently in isolated worktrees.", Style::default().fg(Color::Gray))));
        } else if c.status == CampaignStatus::Running {
            lines.push(Line::from(Span::styled("Agent is actively executing in isolated worktree and live tmux session.", Style::default().fg(Color::Green))));
            lines.push(Line::from(Span::styled("Press [Enter] to attach and interact live, or [x] to cancel this task.", Style::default().fg(Color::Cyan))));
        } else if c.status == CampaignStatus::Claimed {
            lines.push(Line::from(Span::styled("Task claimed by daemon; initializing Git worktree and agent session...", Style::default().fg(Color::Yellow))));
        } else {
            lines.push(Line::from(Span::styled("Session recorded. Press [Enter] to open interactive review.", Style::default().fg(Color::Cyan))));
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
    } else {
        let empty_msg = Paragraph::new("No tasks available. Press [n] to launch a new 24/7 autonomous loop.")
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
            Span::raw("Interrupt Agent  "),
            Span::styled("[Type] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Steer Live  "),
        ])
    } else {
        Line::from(vec![
            Span::styled(" [Enter] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Talk to Agent  "),
            Span::styled("[n] ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("New Task  "),
            Span::styled("[x] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Stop Task  "),
            Span::styled("[r] ", Style::default().fg(Color::Cyan)),
            Span::raw("Refresh  "),
            Span::styled("[q] ", Style::default().fg(Color::Red)),
            Span::raw("Quit"),
        ])
    }
}

fn draw_modal(frame: &mut Frame, modal: &crate::tui::app::ModalState, screen: Rect) {
    let (width, height) = match modal.step {
        ModalStep::SelectModel => {
            (
                86.min(screen.width.saturating_sub(4)),
                24.min(screen.height.saturating_sub(4)),
            )
        }
        ModalStep::EnterPrompt => {
            (
                76.min(screen.width.saturating_sub(4)),
                12.min(screen.height.saturating_sub(4)),
            )
        }
        _ => {
            (
                74.min(screen.width.saturating_sub(4)),
                14.min(screen.height.saturating_sub(4)),
            )
        }
    };
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
                    .title(" New Task: Select Target Repository (Enter to confirm, Esc to cancel) ")
                    .border_style(Style::default().fg(Color::Cyan)));

            frame.render_widget(list, modal_area);
        }
        ModalStep::SelectEngine => {
            let items: Vec<ListItem> = modal.engines.iter().enumerate().map(|(idx, e)| {
                let is_sel = idx == modal.selected_engine_idx;
                let prefix = if is_sel { "▸ " } else { "  " };
                let mut item = ListItem::new(format!("{}{}", prefix, e.label));
                if is_sel {
                    item = item.style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).bg(Color::Rgb(30, 32, 48)));
                }
                item
            }).collect();

            let list = List::new(items)
                .block(Block::default()
                    .borders(Borders::ALL)
                    .title(" New Task: Select Agent Engine (Codex vs OpenCode) ")
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
                    .title(" New Task: Select Execution Mode (Loop vs Single Task) ")
                    .border_style(Style::default().fg(Color::Cyan)));

            frame.render_widget(list, modal_area);
        }
        ModalStep::SelectModel => {
            let engine_name = modal.selected_engine().label.split('(').next().unwrap_or(modal.selected_engine().id).trim();
            let filtered = modal.filtered_models();
            let total = filtered.len();

            let current_pos = if total > 0 { modal.selected_model_idx + 1 } else { 0 };
            let title_info = if modal.model_filter.is_empty() {
                format!(" New Task: Select Model for {} [{}/{}] ", engine_name, current_pos, total)
            } else {
                format!(" New Task: Select Model for {} [Match {}/{}] ", engine_name, current_pos, total)
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .title(title_info)
                .border_style(Style::default().fg(Color::Cyan));

            let inner = block.inner(modal_area);
            frame.render_widget(block, modal_area);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2), // Search bar & navigation hints
                    Constraint::Min(4),    // Scrollable list
                ])
                .split(inner);

            // Search filter row
            let filter_line = if modal.model_filter.is_empty() {
                Line::from(vec![
                    Span::styled("  Search: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled("(Type to filter models...)  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[↑/↓] ", Style::default().fg(Color::Green)),
                    Span::raw("Scroll  "),
                    Span::styled("[PgUp/PgDn] ", Style::default().fg(Color::Yellow)),
                    Span::raw("Page  "),
                    Span::styled("[Enter] ", Style::default().fg(Color::Cyan)),
                    Span::raw("Select"),
                ])
            } else {
                Line::from(vec![
                    Span::styled("  Search: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    Span::styled(format!("'{}'█ ", modal.model_filter), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled("(Backspace deletes, Esc/Ctrl-U clears filter)", Style::default().fg(Color::DarkGray)),
                ])
            };
            let sep_line = Line::from(Span::styled("  ──────────────────────────────────────────────────────────────────────────────", Style::default().fg(Color::Rgb(40, 42, 58))));
            let search_widget = Paragraph::new(vec![filter_line, sep_line]);
            frame.render_widget(search_widget, chunks[0]);

            // Scrollable List Area
            let list_area = chunks[1];
            let visible_rows = list_area.height as usize;
            let mut scroll_offset = 0;
            if modal.selected_model_idx >= visible_rows {
                scroll_offset = modal.selected_model_idx.saturating_sub(visible_rows.saturating_sub(1));
            }

            let end_idx = (scroll_offset + visible_rows).min(total);
            let visible_slice = if total > 0 { &filtered[scroll_offset..end_idx] } else { &[] };

            let items: Vec<ListItem> = visible_slice.iter().map(|(real_idx, m)| {
                let is_sel = *real_idx == modal.selected_model_idx;
                let prefix = if is_sel { "▸ " } else { "  " };
                let num = real_idx + 1;
                let text = format!("{:>2}. {}{}", num, prefix, m.label);
                let mut item = ListItem::new(text);
                if is_sel {
                    item = item.style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                            .bg(Color::Rgb(30, 32, 48)),
                    );
                }
                item
            }).collect();

            if items.is_empty() {
                let empty = Paragraph::new(format!("  No models match query '{}'. Press Backspace or Esc to clear.", modal.model_filter))
                    .style(Style::default().fg(Color::Red));
                frame.render_widget(empty, list_area);
            } else {
                let list = List::new(items);
                frame.render_widget(list, list_area);
            }
        }
        ModalStep::EnterPrompt => {
            let mode_tag = if modal.selected_mode().id == "continuous" { "24/7 Loop (30 iters)" } else { "Single Task" };
            let engine_tag = modal.selected_engine().id.to_uppercase();
            let title = format!(" New Task: [{}] · [{}] · {} ", modal.selected_repo(), engine_tag, mode_tag);

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

            let label = Paragraph::new(format!("Engine: {} │ Enter campaign objective:", engine_tag))
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
