use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Bar, BarChart, BarGroup, Block, Borders, Gauge, Paragraph, Row, Sparkline, Table,
        TableState, Tabs,
    },
    Frame, Terminal,
};
use std::{
    collections::VecDeque,
    io::{self, Stdout},
    time::{Duration, Instant},
};
use sysinfo::{Disks, Networks, Pid, System};

// Constants
const TICK: Duration = Duration::from_secs(1);
const HIST_LEN: usize = 120;
const WIFI_POLL_INTERVAL: Duration = Duration::from_secs(15);
const WIFI_WARMUP_INTERVAL: Duration = Duration::from_secs(2);

// Theme colors (Tailwind-inspired)
#[derive(Clone, Copy)]
struct Theme {
    bg: Color,
    fg: Color,
    accent: Color,
    good: Color,
    warn: Color,
    muted: Color,
}

impl Theme {
    fn dark() -> Self {
        Self {
            bg: Color::Rgb(15, 23, 42),      // slate-900
            fg: Color::Rgb(226, 232, 240),   // slate-200
            accent: Color::Rgb(34, 211, 238), // cyan-400
            good: Color::Rgb(52, 211, 153),   // emerald-400
            warn: Color::Rgb(251, 191, 36),   // amber-400
            muted: Color::Rgb(148, 163, 184), // slate-400
        }
    }
}

// Wi-Fi statistics
#[derive(Debug, Clone)]
struct WifiStats {
    iface: String,
    rssi_dbm: Option<i32>,
    snr_db: Option<i32>,
}

// Network interface row
#[derive(Debug, Clone)]
struct NetworkRow {
    iface: String,
    down_rate: u64,
    up_rate: u64,
    total_in: u64,
    total_out: u64,
    errors_in: u64,
    errors_out: u64,
    drops: u64,
}

// Process row
#[derive(Debug, Clone)]
struct ProcessRow {
    pid: u32,
    name: String,
    cpu_usage: f32,
    memory: u64,
}

// Application state
struct AppState {
    theme: Theme,
    active_tab: usize,
    paused: bool,
    table_state: TableState,
    sys: System,
    disks: Disks,
    networks: Networks,
    wifi_stats: Option<WifiStats>,
    last_wifi_poll: Option<Instant>,
    
    // Aggregates
    cpu_avg: f32,
    mem_used: u64,
    mem_total: u64,
    mem_free: u64,
    mem_avail: u64,
    swap_used: u64,
    swap_total: u64,
    uptime: u64,
    load_avg_1: f64,
    load_avg_5: f64,
    load_avg_15: f64,
    hostname: String,
    os_string: String,
    process_count: usize,
    
    // Network data
    network_rows: Vec<NetworkRow>,
    prev_net_data: std::collections::HashMap<String, (u64, u64, Instant)>,
    
    // Process data
    process_rows: Vec<ProcessRow>,
    
    // History
    cpu_history: VecDeque<f32>,
    mem_history: VecDeque<f32>,
}

impl AppState {
    fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        
        let theme = Theme::dark();
        let mut table_state = TableState::default();
        table_state.select(Some(0));
        
        Self {
            theme,
            active_tab: 0,
            paused: false,
            table_state,
            sys,
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            wifi_stats: None,
            last_wifi_poll: None,
            
            cpu_avg: 0.0,
            mem_used: 0,
            mem_total: 0,
            mem_free: 0,
            mem_avail: 0,
            swap_used: 0,
            swap_total: 0,
            uptime: 0,
            load_avg_1: 0.0,
            load_avg_5: 0.0,
            load_avg_15: 0.0,
            hostname: String::new(),
            os_string: String::new(),
            process_count: 0,
            
            network_rows: Vec::new(),
            prev_net_data: std::collections::HashMap::new(),
            
            process_rows: Vec::new(),
            
            cpu_history: VecDeque::with_capacity(HIST_LEN),
            mem_history: VecDeque::with_capacity(HIST_LEN),
        }
    }
    
    fn refresh(&mut self) {
        // Refresh system info
        self.sys.refresh_all();
        
        // CPU average
        self.cpu_avg = self.sys.global_cpu_usage();
        
        // Memory
        self.mem_used = self.sys.used_memory();
        self.mem_total = self.sys.total_memory();
        self.mem_free = self.sys.free_memory();
        self.mem_avail = self.sys.available_memory();
        
        // Swap
        self.swap_used = self.sys.used_swap();
        self.swap_total = self.sys.total_swap();
        
        // Uptime
        self.uptime = System::uptime();
        
        // Load averages (0.0 on Windows)
        let load = System::load_average();
        self.load_avg_1 = load.one;
        self.load_avg_5 = load.five;
        self.load_avg_15 = load.fifteen;
        
        // Host info
        self.hostname = System::host_name().unwrap_or_else(|| "Unknown".to_string());
        self.os_string = format!(
            "{} {}",
            System::name().unwrap_or_else(|| "Unknown".to_string()),
            System::os_version().unwrap_or_else(|| "".to_string())
        );
        
        // Process count
        self.process_count = self.sys.processes().len();
        
        // Update history
        self.cpu_history.push_back(self.cpu_avg);
        if self.cpu_history.len() > HIST_LEN {
            self.cpu_history.pop_front();
        }
        
        let mem_pct = if self.mem_total > 0 {
            (self.mem_used as f32 / self.mem_total as f32) * 100.0
        } else {
            0.0
        };
        self.mem_history.push_back(mem_pct);
        if self.mem_history.len() > HIST_LEN {
            self.mem_history.pop_front();
        }
        
        // Refresh disks
        self.disks.refresh(false);
        
        // Refresh networks
        self.networks.refresh(false);
        self.update_network_rows();
        
        // Refresh Wi-Fi stats
        self.refresh_wifi();
        
        // Update processes
        self.update_processes();
    }
    
    fn refresh_wifi(&mut self) {
        let now = Instant::now();
        let should_poll = match self.last_wifi_poll {
            None => true,
            Some(last) => {
                let interval = if self.wifi_stats.is_none() {
                    WIFI_WARMUP_INTERVAL
                } else {
                    WIFI_POLL_INTERVAL
                };
                now.duration_since(last) >= interval
            }
        };
        
        if should_poll {
            self.wifi_stats = get_wifi_stats_windows();
            self.last_wifi_poll = Some(now);
        }
    }
    
    fn update_network_rows(&mut self) {
        let now = Instant::now();
        let mut rows = Vec::new();
        
        for (iface_name, data) in self.networks.iter() {
            let total_in = data.total_received();
            let total_out = data.total_transmitted();
            let errors_in = data.total_errors_on_received();
            let errors_out = data.total_errors_on_transmitted();
            
            // Calculate rates
            let (down_rate, up_rate) = if let Some((prev_in, prev_out, prev_time)) =
                self.prev_net_data.get(iface_name)
            {
                let dt = now.duration_since(*prev_time).as_secs_f64();
                if dt > 0.0 {
                    let down = ((total_in - prev_in) as f64 / dt) as u64;
                    let up = ((total_out - prev_out) as f64 / dt) as u64;
                    (down, up)
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };
            
            self.prev_net_data
                .insert(iface_name.to_string(), (total_in, total_out, now));
            
            // Try to get drops from Windows
            let drops = get_interface_drops_windows(iface_name);
            
            rows.push(NetworkRow {
                iface: iface_name.to_string(),
                down_rate,
                up_rate,
                total_in,
                total_out,
                errors_in,
                errors_out,
                drops,
            });
        }
        
        // Sort by total throughput descending
        rows.sort_by(|a, b| {
            let a_total = a.down_rate + a.up_rate;
            let b_total = b.down_rate + b.up_rate;
            b_total.cmp(&a_total)
        });
        
        self.network_rows = rows;
    }
    
    fn update_processes(&mut self) {
        let mut processes: Vec<ProcessRow> = self
            .sys
            .processes()
            .iter()
            .map(|(pid, process)| ProcessRow {
                pid: pid.as_u32(),
                name: process.name().to_string_lossy().to_string(),
                cpu_usage: process.cpu_usage(),
                memory: process.memory(),
            })
            .collect();
        
        // Sort by CPU usage descending
        processes.sort_by(|a, b| b.cpu_usage.partial_cmp(&a.cpu_usage).unwrap());
        
        // Keep top 25
        processes.truncate(25);
        
        self.process_rows = processes;
    }
    
    fn kill_selected_process(&mut self, force: bool) {
        if let Some(selected) = self.table_state.selected() {
            if selected < self.process_rows.len() {
                let pid = self.process_rows[selected].pid;
                if let Some(process) = self.sys.process(Pid::from_u32(pid)) {
                    if force {
                        process.kill();
                    } else {
                        let _ = process.kill_with(sysinfo::Signal::Term);
                    }
                }
            }
        }
    }
    
    fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % 6;
    }
    
    fn prev_tab(&mut self) {
        self.active_tab = if self.active_tab == 0 {
            5
        } else {
            self.active_tab - 1
        };
    }
    
    fn select_next_process(&mut self) {
        if self.process_rows.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.process_rows.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }
    
    fn select_prev_process(&mut self) {
        if self.process_rows.is_empty() {
            return;
        }
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.process_rows.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
    }
}

// Windows-specific: Get Wi-Fi stats using netsh
fn get_wifi_stats_windows() -> Option<WifiStats> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("netsh")
            .args(&["wlan", "show", "interfaces"])
            .output()
            .ok()?;
        
        if !output.status.success() {
            return None;
        }
        
        let text = String::from_utf8_lossy(&output.stdout);
        let mut iface = None;
        let mut signal_pct = None;
        
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with("Name") {
                if let Some(val) = line.split(':').nth(1) {
                    iface = Some(val.trim().to_string());
                }
            } else if line.starts_with("Signal") {
                if let Some(val) = line.split(':').nth(1) {
                    let val = val.trim().replace('%', "");
                    if let Ok(pct) = val.parse::<i32>() {
                        signal_pct = Some(pct);
                    }
                }
            }
        }
        
        if let (Some(iface), Some(pct)) = (iface, signal_pct) {
            // Convert signal percentage to approximate RSSI dBm
            // Rough mapping: 100% ≈ -30 dBm, 0% ≈ -90 dBm
            let rssi_dbm = -90 + (pct * 60 / 100);
            
            return Some(WifiStats {
                iface,
                rssi_dbm: Some(rssi_dbm),
                snr_db: None, // Not available via netsh
            });
        }
    }
    
    None
}

// Windows-specific: Get interface drops
fn get_interface_drops_windows(_iface: &str) -> u64 {
    // Placeholder: would use netsh or PowerShell
    // netsh interface ipv4 show interfaces
    // or Get-NetAdapterStatistics
    0
}

// Format bytes
fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;
    
    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }
    
    if unit_idx == 0 {
        format!("{} {}", size as u64, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

// Format bytes per second
fn format_bytes_per_sec(bytes: u64) -> String {
    format!("{}/s", format_bytes(bytes))
}

// Format duration
fn format_duration(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    
    if days > 0 {
        format!("{}d {}h {}m {}s", days, hours, mins, secs)
    } else if hours > 0 {
        format!("{}h {}m {}s", hours, mins, secs)
    } else if mins > 0 {
        format!("{}m {}s", mins, secs)
    } else {
        format!("{}s", secs)
    }
}

// UI rendering
fn render_ui(frame: &mut Frame, app: &mut AppState) {
    let theme = app.theme;
    
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Title bar
            Constraint::Min(0),     // Content
            Constraint::Length(3), // Status bar
        ])
        .split(frame.area());
    
    // Title bar
    render_title_bar(frame, main_layout[0], app, theme);
    
    // Status bar
    render_status_bar(frame, main_layout[2], theme);
    
    // Tab content
    let tabs_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(main_layout[1]);
    
    // Tab selector
    render_tabs(frame, tabs_layout[0], app, theme);
    
    // Tab content
    match app.active_tab {
        0 => render_overview_tab(frame, tabs_layout[1], app, theme),
        1 => render_simple_tab(frame, tabs_layout[1], app, theme),
        2 => render_system_tab(frame, tabs_layout[1], app, theme),
        3 => render_disks_tab(frame, tabs_layout[1], app, theme),
        4 => render_network_tab(frame, tabs_layout[1], app, theme),
        5 => render_processes_tab(frame, tabs_layout[1], app, theme),
        _ => {}
    }
}

fn render_title_bar(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let status = if app.paused { " [PAUSED] " } else { "" };
    let title = format!(
        "{} {} | {} | Uptime: {}",
        status,
        app.hostname,
        app.os_string,
        format_duration(app.uptime)
    );
    
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.accent))
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    
    let paragraph = Paragraph::new(title)
        .block(block)
        .alignment(Alignment::Center);
    
    frame.render_widget(paragraph, area);
}

fn render_status_bar(frame: &mut Frame, area: Rect, theme: Theme) {
    let text = " q/Esc: Quit | Space: Pause | r: Refresh | ←/→: Tab | ↑/↓: Select | k: Term | K: Kill ";
    
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.muted))
        .style(Style::default().bg(theme.bg).fg(theme.muted));
    
    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center);
    
    frame.render_widget(paragraph, area);
}

fn render_tabs(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let tab_titles = vec!["Overview", "Simple", "System", "Disks", "Network", "Processes"];
    let tabs = Tabs::new(tab_titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.bg)),
        )
        .select(app.active_tab)
        .style(Style::default().fg(theme.fg))
        .highlight_style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD));
    
    frame.render_widget(tabs, area);
}

fn render_overview_tab(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);
    
    let top_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(33),
            Constraint::Percentage(33),
            Constraint::Percentage(34),
        ])
        .split(main_layout[0]);
    
    // CPU gauge + sparkline
    render_cpu_panel(frame, top_layout[0], app, theme);
    
    // Memory gauge + sparkline
    render_memory_panel(frame, top_layout[1], app, theme);
    
    // Per-core bar chart
    render_per_core_panel(frame, top_layout[2], app, theme);
    
    let bottom_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_layout[1]);
    
    // Disks table
    render_disks_panel(frame, bottom_layout[0], app, theme);
    
    // Network table (top 10)
    render_network_panel(frame, bottom_layout[1], app, theme, 10);
}

fn render_cpu_panel(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    
    // Gauge
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title("CPU"),
        )
        .gauge_style(Style::default().fg(theme.good))
        .percent(app.cpu_avg as u16)
        .label(format!("{:.1}%", app.cpu_avg));
    
    frame.render_widget(gauge, layout[0]);
    
    // Sparkline
    let data: Vec<u64> = app.cpu_history.iter().map(|&v| v as u64).collect();
    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.muted))
                .title("History"),
        )
        .data(&data)
        .style(Style::default().fg(theme.accent));
    
    frame.render_widget(sparkline, layout[1]);
}

fn render_memory_panel(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    
    let mem_pct = if app.mem_total > 0 {
        ((app.mem_used as f64 / app.mem_total as f64) * 100.0) as u16
    } else {
        0
    };
    
    // Gauge
    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title("Memory"),
        )
        .gauge_style(Style::default().fg(theme.good))
        .percent(mem_pct)
        .label(format!("{:.1}%", mem_pct));
    
    frame.render_widget(gauge, layout[0]);
    
    // Sparkline
    let data: Vec<u64> = app.mem_history.iter().map(|&v| v as u64).collect();
    let sparkline = Sparkline::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.muted))
                .title("History"),
        )
        .data(&data)
        .style(Style::default().fg(theme.accent));
    
    frame.render_widget(sparkline, layout[1]);
}

fn render_per_core_panel(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let cpus = app.sys.cpus();
    let core_count = cpus.len().min(16);
    
    let mut bars = Vec::new();
    for (i, cpu) in cpus.iter().enumerate().take(core_count) {
        let label = format!("{}", i);
        let value = cpu.cpu_usage() as u64;
        bars.push(Bar::default().label(label.into()).value(value));
    }
    
    let group = BarGroup::default().bars(&bars);
    let chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title("Per-Core CPU"),
        )
        .data(group)
        .bar_width(3)
        .bar_gap(1)
        .bar_style(Style::default().fg(theme.good))
        .value_style(Style::default().fg(theme.fg));
    
    frame.render_widget(chart, area);
}

fn render_disks_panel(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let rows: Vec<Row> = app
        .disks
        .iter()
        .map(|disk| {
            let mount = disk.mount_point().to_string_lossy().to_string();
            let used = disk.total_space() - disk.available_space();
            let used_pct = if disk.total_space() > 0 {
                (used as f64 / disk.total_space() as f64 * 100.0) as u64
            } else {
                0
            };
            let free = format_bytes(disk.available_space());
            
            Row::new(vec![
                mount,
                format!("{}%", used_pct),
                free,
            ])
        })
        .collect();
    
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(50),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ],
    )
    .header(
        Row::new(vec!["Mount", "Used %", "Free"])
            .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title("Disks"),
    )
    .style(Style::default().fg(theme.fg));
    
    frame.render_widget(table, area);
}

fn render_network_panel(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    theme: Theme,
    limit: usize,
) {
    let rows: Vec<Row> = app
        .network_rows
        .iter()
        .take(limit)
        .map(|net| {
            Row::new(vec![
                net.iface.clone(),
                format_bytes_per_sec(net.down_rate),
                format_bytes_per_sec(net.up_rate),
            ])
        })
        .collect();
    
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(30),
            Constraint::Percentage(30),
        ],
    )
    .header(
        Row::new(vec!["Interface", "Down/s", "Up/s"])
            .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title("Network"),
    )
    .style(Style::default().fg(theme.fg));
    
    frame.render_widget(table, area);
}

fn render_simple_tab(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    
    // CPU gauge
    let cpu_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title("CPU Busy %"),
        )
        .gauge_style(Style::default().fg(theme.good))
        .percent(app.cpu_avg as u16)
        .label(format!("{:.1}%", app.cpu_avg));
    
    frame.render_widget(cpu_gauge, layout[0]);
    
    // Memory gauge
    let mem_pct = if app.mem_total > 0 {
        ((app.mem_used as f64 / app.mem_total as f64) * 100.0) as u16
    } else {
        0
    };
    
    let mem_gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title("Memory % Used"),
        )
        .gauge_style(Style::default().fg(theme.good))
        .percent(mem_pct)
        .label(format!("{:.1}%", mem_pct));
    
    frame.render_widget(mem_gauge, layout[1]);
    
    // Summary text
    let status_tier = if app.cpu_avg > 80.0 || mem_pct > 80 {
        "Busy"
    } else if app.cpu_avg > 50.0 || mem_pct > 60 {
        "Moderate"
    } else {
        "Light"
    };
    
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(theme.muted)),
            Span::styled(status_tier, Style::default().fg(theme.accent)),
        ]),
        Line::from(vec![
            Span::styled("CPU Usage: ", Style::default().fg(theme.muted)),
            Span::styled(format!("{:.1}%", app.cpu_avg), Style::default().fg(theme.fg)),
        ]),
        Line::from(vec![
            Span::styled("Memory Usage: ", Style::default().fg(theme.muted)),
            Span::styled(
                format!("{} / {}", format_bytes(app.mem_used), format_bytes(app.mem_total)),
                Style::default().fg(theme.fg),
            ),
        ]),
    ];
    
    // Storage (first disk)
    if let Some(disk) = app.disks.iter().next() {
        let used = disk.total_space() - disk.available_space();
        lines.push(Line::from(vec![
            Span::styled("Storage: ", Style::default().fg(theme.muted)),
            Span::styled(
                format!("{} / {}", format_bytes(used), format_bytes(disk.total_space())),
                Style::default().fg(theme.fg),
            ),
        ]));
    }
    
    // Network aggregate
    let total_down: u64 = app.network_rows.iter().map(|n| n.down_rate).sum();
    let total_up: u64 = app.network_rows.iter().map(|n| n.up_rate).sum();
    lines.push(Line::from(vec![
        Span::styled("Network: ", Style::default().fg(theme.muted)),
        Span::styled(
            format!("↓ {} ↑ {}", format_bytes_per_sec(total_down), format_bytes_per_sec(total_up)),
            Style::default().fg(theme.fg),
        ),
    ]));
    
    // Wi-Fi stats
    if let Some(ref wifi) = app.wifi_stats {
        let rssi_str = wifi
            .rssi_dbm
            .map(|r| format!("{} dBm", r))
            .unwrap_or_else(|| "N/A".to_string());
        lines.push(Line::from(vec![
            Span::styled("Wi-Fi: ", Style::default().fg(theme.muted)),
            Span::styled(format!("{} ({})", wifi.iface, rssi_str), Style::default().fg(theme.fg)),
        ]));
    }
    
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title("Summary"),
        )
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    
    frame.render_widget(paragraph, layout[2]);
}

fn render_system_tab(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let cpu_brand = app
        .sys
        .cpus()
        .first()
        .map(|c| c.brand())
        .unwrap_or("Unknown");
    let cpu_freq = app
        .sys
        .cpus()
        .first()
        .map(|c| format!("{} MHz", c.frequency()))
        .unwrap_or_else(|| "N/A".to_string());
    
    let uptime_str = format_duration(app.uptime);
    let load_avg_str = format!("{:.2}/{:.2}/{:.2}", app.load_avg_1, app.load_avg_5, app.load_avg_15);
    let logical_cores_str = app.sys.cpus().len().to_string();
    let cpu_avg_str = format!("{:.1}%", app.cpu_avg);
    let mem_used_str = format!("{} / {}", format_bytes(app.mem_used), format_bytes(app.mem_total));
    let mem_free_str = format_bytes(app.mem_free);
    let mem_avail_str = format_bytes(app.mem_avail);
    let swap_str = format!("{} / {}", format_bytes(app.swap_used), format_bytes(app.swap_total));
    let process_count_str = app.process_count.to_string();
    
    let rows = vec![
        Row::new(vec!["Host", &app.hostname]),
        Row::new(vec!["OS", &app.os_string]),
        Row::new(vec!["Uptime", &uptime_str]),
        Row::new(vec![
            "Load Avg 1/5/15",
            &load_avg_str,
        ]),
        Row::new(vec!["CPU", cpu_brand]),
        Row::new(vec!["CPU Freq", &cpu_freq]),
        Row::new(vec!["Logical Cores", &logical_cores_str]),
        Row::new(vec!["CPU Avg", &cpu_avg_str]),
        Row::new(vec![
            "Memory Used",
            &mem_used_str,
        ]),
        Row::new(vec!["Memory Free", &mem_free_str]),
        Row::new(vec!["Memory Avail", &mem_avail_str]),
        Row::new(vec![
            "Swap",
            &swap_str,
        ]),
        Row::new(vec!["Process Count", &process_count_str]),
    ];
    
    let table = Table::new(rows, [Constraint::Percentage(30), Constraint::Percentage(70)])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title("System Information"),
        )
        .style(Style::default().fg(theme.fg))
        .column_spacing(2);
    
    frame.render_widget(table, area);
}

fn render_disks_tab(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let rows: Vec<Row> = app
        .disks
        .iter()
        .map(|disk| {
            let name = disk.name().to_string_lossy().to_string();
            let mount = disk.mount_point().to_string_lossy().to_string();
            let fs = disk.file_system().to_string_lossy().to_string();
            let kind = format!("{:?}", disk.kind());
            let total = format_bytes(disk.total_space());
            let used = disk.total_space() - disk.available_space();
            let used_str = format_bytes(used);
            let free = format_bytes(disk.available_space());
            
            // Read/write stats would need to be tracked separately
            let r_s = "N/A".to_string();
            let w_s = "N/A".to_string();
            
            Row::new(vec![name, mount, fs, kind, total, used_str, free, r_s, w_s])
        })
        .collect();
    
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(10),
            Constraint::Percentage(15),
            Constraint::Percentage(8),
            Constraint::Percentage(10),
            Constraint::Percentage(12),
            Constraint::Percentage(12),
            Constraint::Percentage(12),
            Constraint::Percentage(10),
            Constraint::Percentage(11),
        ],
    )
    .header(
        Row::new(vec!["Name", "Mount", "FS", "Type", "Total", "Used", "Free", "R/s", "W/s"])
            .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title("Disks"),
    )
    .style(Style::default().fg(theme.fg));
    
    frame.render_widget(table, area);
}

fn render_network_tab(frame: &mut Frame, area: Rect, app: &AppState, theme: Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);
    
    // Wi-Fi summary
    let wifi_text = if let Some(ref wifi) = app.wifi_stats {
        let rssi_str = wifi
            .rssi_dbm
            .map(|r| format!("{} dBm", r))
            .unwrap_or_else(|| "N/A".to_string());
        let snr_str = wifi
            .snr_db
            .map(|s| format!("{} dB", s))
            .unwrap_or_else(|| "N/A".to_string());
        format!(
            "Interface: {} | RSSI: {} | SNR: {} | Drops: 0",
            wifi.iface, rssi_str, snr_str
        )
    } else {
        "Wi-Fi stats unavailable".to_string()
    };
    
    let wifi_para = Paragraph::new(wifi_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.accent))
                .title("Wi-Fi"),
        )
        .style(Style::default().bg(theme.bg).fg(theme.fg));
    
    frame.render_widget(wifi_para, layout[0]);
    
    // Network table
    let rows: Vec<Row> = app
        .network_rows
        .iter()
        .map(|net| {
            let rssi = app
                .wifi_stats
                .as_ref()
                .and_then(|w| {
                    if w.iface == net.iface {
                        w.rssi_dbm.map(|r| format!("{}", r))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "-".to_string());
            
            let snr = app
                .wifi_stats
                .as_ref()
                .and_then(|w| {
                    if w.iface == net.iface {
                        w.snr_db.map(|s| format!("{}", s))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "-".to_string());
            
            Row::new(vec![
                net.iface.clone(),
                format_bytes_per_sec(net.down_rate),
                format_bytes_per_sec(net.up_rate),
                format_bytes(net.total_in),
                format_bytes(net.total_out),
                net.errors_in.to_string(),
                net.errors_out.to_string(),
                rssi,
                snr,
                net.drops.to_string(),
            ])
        })
        .collect();
    
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(15),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(10),
            Constraint::Percentage(9),
            Constraint::Percentage(9),
            Constraint::Percentage(9),
            Constraint::Percentage(9),
            Constraint::Percentage(9),
        ],
    )
    .header(
        Row::new(vec![
            "Iface", "Down/s", "Up/s", "Tot In", "Tot Out", "Err In", "Err Out", "RSSI", "SNR",
            "Drops",
        ])
        .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title("Network Interfaces"),
    )
    .style(Style::default().fg(theme.fg));
    
    frame.render_widget(table, layout[1]);
}

fn render_processes_tab(frame: &mut Frame, area: Rect, app: &mut AppState, theme: Theme) {
    let rows: Vec<Row> = app
        .process_rows
        .iter()
        .map(|proc| {
            Row::new(vec![
                proc.pid.to_string(),
                proc.name.clone(),
                format!("{:.1}%", proc.cpu_usage),
                format_bytes(proc.memory),
            ])
        })
        .collect();
    
    let table = Table::new(
        rows,
        [
            Constraint::Percentage(10),
            Constraint::Percentage(50),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ],
    )
    .header(
        Row::new(vec!["PID", "Process", "CPU%", "Memory"])
            .style(Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title("Top 25 Processes (by CPU)"),
    )
    .row_highlight_style(Style::default().fg(theme.bg).bg(theme.accent))
    .style(Style::default().fg(theme.fg));
    
    frame.render_stateful_widget(table, area, &mut app.table_state);
}

// Main function
fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // Create app state
    let mut app = AppState::new();
    app.refresh();
    
    // Run event loop
    let result = run_app(&mut terminal, &mut app);
    
    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    
    result
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut AppState) -> Result<()> {
    let mut last_tick = Instant::now();
    
    loop {
        terminal.draw(|f| render_ui(f, app))?;
        
        let timeout = TICK.saturating_sub(last_tick.elapsed());
        
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        KeyCode::Char(' ') => app.paused = !app.paused,
                        KeyCode::Char('r') => {
                            if !app.paused {
                                app.refresh();
                            }
                        }
                        KeyCode::Char('k') => app.kill_selected_process(false),
                        KeyCode::Char('K') => app.kill_selected_process(true),
                        KeyCode::Left => app.prev_tab(),
                        KeyCode::Right => app.next_tab(),
                        KeyCode::Up => app.select_prev_process(),
                        KeyCode::Down => app.select_next_process(),
                        _ => {}
                    }
                }
            }
        }
        
        if last_tick.elapsed() >= TICK {
            if !app.paused {
                app.refresh();
            }
            last_tick = Instant::now();
        }
    }
}
