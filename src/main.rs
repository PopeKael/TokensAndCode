#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod scanner;
use chrono::NaiveDate;
use eframe::egui::{self, Color32, RichText, ViewportCommand};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use scanner::{Snapshot, Stats};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    IsWindow, IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow,
};

const APP_NAME: &str = "Tokens and Code";
const AA: [(&str, &str, i32, f64); 3] = [
    (
        "GPT-6 Astra",
        "https://artificialanalysis.ai/models/gpt-6-astra/",
        53,
        3.26,
    ),
    (
        "GPT-6.1 Sol",
        "https://artificialanalysis.ai/models/gpt-6-1-sol/",
        52,
        0.72,
    ),
    (
        "GPT-6 Luna",
        "https://artificialanalysis.ai/models/gpt-6-luna/",
        38,
        0.07,
    ),
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct Config {
    source_root: String,
    read_only_confirmed: bool,
}
impl Default for Config {
    fn default() -> Self {
        let home = std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        Self {
            source_root: home.join(".codex").join("sessions").display().to_string(),
            read_only_confirmed: false,
        }
    }
}
fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("com", "TokensAndCode", "TokensAndCode")
        .map(|x| x.data_local_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
}
fn config_path() -> PathBuf {
    data_dir().join("config.json")
}
fn snapshot_path() -> PathBuf {
    data_dir().join("recent.json")
}
fn read_config() -> Config {
    fs::read(config_path())
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or_default()
}
fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).map_err(|e| e.to_string())?
    }
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}
fn read_snapshot() -> Option<Snapshot> {
    fs::read(snapshot_path())
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
}

enum WorkerMsg {
    Progress(usize, usize, String),
    Done(Result<Snapshot, String>),
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Period {
    Daily,
    Weekly,
    Monthly,
    Custom,
    Session,
    All,
}
impl Period {
    fn label(self) -> &'static str {
        match self {
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
            Self::Custom => "Custom",
            Self::Session => "Current session",
            Self::All => "All time",
        }
    }
}
struct UsageApp {
    config: Config,
    snapshot: Option<Snapshot>,
    period: Period,
    from: String,
    to: String,
    chart_metric: String,
    chart_source: String,
    custom_stats: Option<Stats>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
    busy: bool,
    progress: (usize, usize),
    current_file: String,
    status: String,
    setup_confirm: bool,
    _tray: Option<TrayIcon>,
    tray_open: Arc<AtomicBool>,
    tray_refresh: Arc<AtomicBool>,
    tray_quit: Arc<AtomicBool>,
    hwnd: isize,
    show_setup: bool,
    quitting: bool,
}
impl UsageApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel();
        let config = read_config();
        let snapshot = read_snapshot();
        let hwnd = cc
            .window_handle()
            .ok()
            .and_then(|h| {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    Some(w.hwnd.get())
                } else {
                    None
                }
            })
            .unwrap_or(0);
        let tray_open = Arc::new(AtomicBool::new(false));
        let tray_refresh = Arc::new(AtomicBool::new(false));
        let tray_quit = Arc::new(AtomicBool::new(false));
        let tray = build_tray(
            cc,
            hwnd,
            tray_open.clone(),
            tray_refresh.clone(),
            tray_quit.clone(),
        )
        .ok();
        let mut app = Self {
            config,
            snapshot,
            period: Period::Daily,
            from: String::new(),
            to: String::new(),
            chart_metric: "Total tokens".into(),
            chart_source: "All sources".into(),
            custom_stats: None,
            tx,
            rx,
            busy: false,
            progress: (0, 0),
            current_file: String::new(),
            status: String::new(),
            setup_confirm: false,
            _tray: tray,
            tray_open,
            tray_refresh,
            tray_quit,
            hwnd,
            show_setup: false,
            quitting: false,
        };
        app.show_setup = !app.config.read_only_confirmed || app.snapshot.is_none();
        if let Some(s) = &app.snapshot {
            let d = s.generated_at.clone().unwrap_or_default();
            app.status = format!("Recent copy loaded · scanned {d}");
            app.from = s
                .daily_history
                .first()
                .map(|x| x.date.clone())
                .unwrap_or_default();
            app.to = s
                .daily_history
                .last()
                .map(|x| x.date.clone())
                .unwrap_or_default();
        }
        if app.config.read_only_confirmed && app.snapshot.is_none() {
            app.status = "Choose the Codex sessions folder and run the first scan.".into();
        }
        app
    }
    fn start_scan(&mut self) {
        let root = PathBuf::from(self.config.source_root.trim());
        if !self.config.read_only_confirmed {
            self.status = "Confirm read-only local access in Setup first.".into();
            return;
        }
        if !root.is_dir() {
            self.status = format!("Folder not found: {}", root.display());
            return;
        }
        if self.busy {
            return;
        }
        self.busy = true;
        self.progress = (0, 0);
        self.current_file.clear();
        self.status = "Scanning Codex logs…".into();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let out = scanner::scan(&root, |i, n, p| {
                let _ = tx.send(WorkerMsg::Progress(
                    i,
                    n,
                    p.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                ));
            });
            let _ = tx.send(WorkerMsg::Done(Ok(out)));
        });
    }
    fn process_events(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                WorkerMsg::Progress(i, n, p) => {
                    self.progress = (i, n);
                    self.current_file = p;
                }
                WorkerMsg::Done(Ok(s)) => {
                    self.busy = false;
                    let partial = s.read_errors.len();
                    let stamp = s.generated_at.clone().unwrap_or_default();
                    let _ = save_json(&snapshot_path(), &s);
                    self.status = if partial == 0 {
                        format!("Refresh complete · {stamp}")
                    } else {
                        format!("Refresh complete with {partial} unreadable log(s) · {stamp}")
                    };
                    self.from = s
                        .daily_history
                        .first()
                        .map(|x| x.date.clone())
                        .unwrap_or_default();
                    self.to = s
                        .daily_history
                        .last()
                        .map(|x| x.date.clone())
                        .unwrap_or_default();
                    self.snapshot = Some(s);
                }
                WorkerMsg::Done(Err(e)) => {
                    self.busy = false;
                    self.status = format!("Refresh failed: {e}");
                }
            }
        }
        if self.tray_open.swap(false, Ordering::Relaxed) {
            self.show_window(ctx)
        }
        if self.tray_refresh.swap(false, Ordering::Relaxed) {
            self.start_scan()
        }
        if self.tray_quit.swap(false, Ordering::Relaxed) {
            self.quitting = true;
            ctx.send_viewport_cmd(ViewportCommand::Close)
        }
    }
    fn show_window(&self, ctx: &egui::Context) {
        unsafe {
            let hwnd = self.hwnd as windows_sys::Win32::Foundation::HWND;
            if hwnd.is_null() || IsWindow(hwnd) == 0 {
                return;
            }
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
            let _ = IsWindowVisible(hwnd);
        }
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
    }
    fn render_setup(&mut self, ui: &mut egui::Ui) {
        ui.heading("Set up Codex log access");
        ui.label("Tokens and Code reads Codex rollout logs on this PC. It does not modify them or send them anywhere.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Sessions folder");
            ui.text_edit_singleline(&mut self.config.source_root);
            if ui.button("Choose…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .set_title("Choose the Codex sessions folder")
                    .set_directory(&self.config.source_root)
                    .pick_folder()
                {
                    self.config.source_root = p.display().to_string();
                }
            }
        });
        ui.checkbox(
            &mut self.setup_confirm,
            "I approve read-only access to the selected local folder.",
        );
        ui.label(RichText::new("A recent, aggregate-only copy will be stored under your local application data folder. The original logs remain untouched.").color(Color32::GRAY));
        if ui.button("Save setup and refresh").clicked() {
            if !self.setup_confirm {
                self.status = "Please confirm read-only access before scanning.".into()
            } else {
                self.config.read_only_confirmed = true;
                let _ = save_json(&config_path(), &self.config);
                self.show_setup = false;
                self.start_scan();
            }
        }
        if !self.status.is_empty() {
            ui.label(&self.status);
        }
    }
    fn selected_stats(&self) -> Stats {
        let Some(s) = &self.snapshot else {
            return Stats::default();
        };
        match self.period {
            Period::Daily => s.today.clone(),
            Period::Weekly => s.this_week.clone(),
            Period::Monthly => s.this_month.clone(),
            Period::Session => s.current_session.clone(),
            Period::All => s.all_time.clone(),
            Period::Custom => self.custom_stats.clone().unwrap_or_default(),
        }
    }
    fn custom_range(&mut self) {
        let Some(s) = &self.snapshot else { return };
        let (Ok(a), Ok(b)) = (
            NaiveDate::parse_from_str(&self.from, "%Y-%m-%d"),
            NaiveDate::parse_from_str(&self.to, "%Y-%m-%d"),
        ) else {
            self.status = "Enter dates as YYYY-MM-DD.".into();
            return;
        };
        if a > b {
            self.status = "Start date must be on or before end date.".into();
            return;
        };
        let mut sum = Stats::default();
        for row in &s.daily_history {
            if let Ok(d) = NaiveDate::parse_from_str(&row.date, "%Y-%m-%d") {
                if d >= a && d <= b {
                    sum.usage.add(&row.stats.usage);
                    sum.primary_tokens += row.stats.primary_tokens;
                    sum.subagent_tokens += row.stats.subagent_tokens;
                }
            }
        }
        self.custom_stats = Some(sum);
        self.period = Period::Custom;
        self.status = format!(
            "Custom range applied · {} through {} Bangkok",
            self.from, self.to
        );
    }
    fn period_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for p in [
                Period::Daily,
                Period::Weekly,
                Period::Monthly,
                Period::Custom,
                Period::Session,
                Period::All,
            ] {
                if ui.selectable_label(self.period == p, p.label()).clicked() {
                    self.period = p;
                }
            }
        });
        if self.period == Period::Custom {
            ui.horizontal(|ui| {
                ui.label("From");
                ui.text_edit_singleline(&mut self.from);
                ui.label("To");
                ui.text_edit_singleline(&mut self.to);
                if ui.button("Apply dates").clicked() {
                    self.custom_range();
                }
            });
        }
    }
    fn show_period(&mut self, ui: &mut egui::Ui) {
        self.period_controls(ui);
        let s = self.selected_stats();
        let range = match self.period {
            Period::Daily => "Today · Bangkok",
            Period::Weekly => "Monday to today · Bangkok",
            Period::Monthly => "Month start to today · Bangkok",
            Period::Custom => "Inclusive custom date range · Bangkok",
            Period::Session => "Current primary session with linked child work",
            Period::All => "All logged history",
        };
        ui.label(RichText::new(range).color(Color32::GRAY));
        ui.add_space(8.0);
        egui::Grid::new("token-metrics")
            .num_columns(3)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                metric(ui, "Input", s.usage.input_tokens);
                metric(ui, "Cached input", s.usage.cached_input_tokens);
                metric(ui, "Fresh input", s.usage.fresh_input_tokens);
                ui.end_row();
                metric(ui, "Output", s.usage.output_tokens);
                metric(
                    ui,
                    "Reasoning output (included above)",
                    s.usage.reasoning_output_tokens,
                );
                metric(
                    ui,
                    "Primary / child tokens",
                    format!("{} / {}", fmt(s.primary_tokens), fmt(s.subagent_tokens)),
                );
                ui.end_row();
            });
        ui.add_space(10.0);
        ui.heading("Hypothetical Standard API equivalent");
        ui.label(
            "USD for the selected token period · reasoning is included in output and billed once",
        );
        ui.horizontal_wrapped(|ui| {
            for name in ["GPT-6 Astra", "GPT-6 Sol", "GPT-6 Luna"] {
                ui.group(|ui| {
                    ui.label(name);
                    ui.heading(format!(
                        "${:.2}",
                        s.usage.cost_usd.get(name).copied().unwrap_or_default()
                    ));
                    ui.label("standard-rate estimate");
                });
            }
        });
        ui.add_space(10.0);
        ui.heading("Artificial Analysis benchmark comparison");
        ui.label("Intelligence Index v4.3.2 · model variant Max · checked 21 October 2026");
        ui.horizontal_wrapped(|ui| {
            for (name, url, score, cost) in AA {
                ui.group(|ui| {
                    ui.label(name);
                    ui.heading(format!("${cost:.2} / AA task"));
                    ui.label(format!("Intelligence score: {score}"));
                    ui.hyperlink_to("Artificial Analysis model page", url);
                });
            }
        });
        ui.label(
            RichText::new(
                "Benchmark task costs are separate from the selected-period token estimate.",
            )
            .color(Color32::GRAY),
        );
    }
    fn allowances(&self, ui: &mut egui::Ui) {
        ui.heading("Plus allowance snapshots");
        let Some(s) = &self.snapshot else {
            ui.label("Not available until first scan.");
            return;
        };
        if s.allowances.five_hour.is_none() && s.allowances.seven_day.is_none() {
            ui.label("No Plus rate-limit records found in scanned logs.");
            return;
        }
        for (label, w) in [
            ("5-hour", s.allowances.five_hour.as_ref()),
            ("7-day", s.allowances.seven_day.as_ref()),
        ] {
            if let Some(w) = w {
                ui.label(format!(
                    "{label}: {:.0}% used · resets {} · observed {}",
                    w.used_percent,
                    w.resets_at.as_deref().unwrap_or("unknown"),
                    w.observed_at.as_deref().unwrap_or("unknown")
                ));
            }
        }
        ui.label(RichText::new("These are the latest allowance snapshots recorded in logs, not a live account query.").color(Color32::GRAY));
    }
    fn stale_banner(&mut self, ui: &mut egui::Ui) {
        if let Some(s) = &self.snapshot {
            if s.age_hours().unwrap_or(0) >= 24 {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("STALE COPY · at least 24 hours old")
                            .color(Color32::YELLOW)
                            .strong(),
                    );
                    ui.label("Refreshing older logs may take a while.");
                    if ui.button("Refresh now").clicked() {
                        self.start_scan();
                    }
                });
            }
        }
    }
    fn detail_tables(&mut self, ui: &mut egui::Ui) {
        let Some(s) = &self.snapshot else { return };
        ui.heading("Top token-consuming projects, sessions and models");
        ui.columns(3, |cols| {
            cols[0].strong("Projects");
            for x in s.projects.iter().take(8) {
                cols[0].label(format!(
                    "{} · {}",
                    truncate(&x.name, 26),
                    fmt(x.stats.total_tokens)
                ));
            }
            cols[1].strong("Sessions");
            for x in s.sessions.iter().take(8) {
                cols[1].label(format!(
                    "{} · {}",
                    truncate(&x.name, 26),
                    fmt(x.stats.total_tokens)
                ));
            }
            cols[2].strong("Models");
            for x in s.models.iter().take(8) {
                cols[2].label(format!(
                    "{} · {}",
                    truncate(&x.name, 26),
                    fmt(x.stats.total_tokens)
                ));
            }
        });
        ui.add_space(10.0);
        ui.heading("Daily token history");
        daily_chart(ui, s, &mut self.chart_metric, &mut self.chart_source);
        egui::ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("daily-grid").striped(true).show(ui, |ui| {
                ui.strong("Date");
                ui.strong("Tokens");
                ui.strong("Responses");
                ui.strong("Astra");
                ui.strong("Sol");
                ui.strong("Luna");
                ui.end_row();
                for d in s.daily_history.iter().rev().take(21) {
                    ui.label(&d.date);
                    ui.label(fmt(d.stats.usage.total_tokens));
                    ui.label(fmt(d.stats.usage.response_count));
                    for m in ["GPT-6 Astra", "GPT-6 Sol", "GPT-6 Luna"] {
                        ui.label(format!(
                            "${:.2}",
                            d.stats.usage.cost_usd.get(m).copied().unwrap_or_default()
                        ));
                    }
                    ui.end_row();
                }
            })
        });
        ui.add_space(10.0);
        ui.heading("Date / project / session / source drill-down");
        egui::ScrollArea::horizontal().show(ui, |ui| {
            egui::Grid::new("drilldown").striped(true).show(ui, |ui| {
                ui.strong("Date");
                ui.strong("Project");
                ui.strong("Session");
                ui.strong("Source");
                ui.strong("Model");
                ui.strong("Effort");
                ui.strong("Working folder");
                ui.strong("Input");
                ui.strong("Cached");
                ui.strong("Fresh");
                ui.strong("Output");
                ui.end_row();
                for r in s.drilldown.iter().take(500) {
                    ui.label(&r.date);
                    ui.label(&r.project);
                    ui.label(truncate(&r.session_id, 12));
                    ui.label(&r.source);
                    ui.label(&r.model);
                    ui.label(&r.effort);
                    ui.label(&r.cwd);
                    ui.label(fmt(r.usage.input_tokens));
                    ui.label(fmt(r.usage.cached_input_tokens));
                    ui.label(fmt(r.usage.fresh_input_tokens));
                    ui.label(fmt(r.usage.output_tokens));
                    ui.end_row();
                }
            })
        });
    }
}
impl eframe::App for UsageApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_events(ctx);
        ctx.request_repaint_after(Duration::from_millis(400));
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(APP_NAME);
                ui.label(RichText::new("LOCAL · READ ONLY").color(Color32::from_rgb(82, 222, 178)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Refresh").clicked() {
                        self.start_scan()
                    }
                    if ui.button("Setup").clicked() {
                        self.setup_confirm = self.config.read_only_confirmed;
                        self.show_setup = true;
                    }
                });
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.show_setup {
                self.stale_banner(ui);
            }
            if self.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(format!(
                        "Scanning file {} of {} · {}",
                        self.progress.0, self.progress.1, self.current_file
                    ));
                    if self.progress.1 > 0 {
                        ui.add(
                            egui::ProgressBar::new(self.progress.0 as f32 / self.progress.1 as f32)
                                .desired_width(180.0),
                        );
                    }
                });
            }
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
            if self.show_setup {
                self.render_setup(ui);
            } else {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_period(ui);
                    ui.separator();
                    self.allowances(ui);
                    ui.separator();
                    self.detail_tables(ui);
                });
            }
        });
    }
}
fn build_tray(
    cc: &eframe::CreationContext<'_>,
    hwnd: isize,
    open: Arc<AtomicBool>,
    refresh: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
) -> Result<TrayIcon, Box<dyn std::error::Error>> {
    let menu = Menu::new();
    let show = MenuItem::new("Open Tokens and Code", true, None);
    let update = MenuItem::new("Refresh logs", true, None);
    let exit = MenuItem::new("Exit", true, None);
    menu.append_items(&[&show, &update, &PredefinedMenuItem::separator(), &exit])?;
    let showid = show.id().clone();
    let refreshid = update.id().clone();
    let exitid = exit.id().clone();
    let ctx = cc.egui_ctx.clone();
    let o = open.clone();
    let r = refresh.clone();
    let q = quit.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if event.id == showid {
            o.store(true, Ordering::Relaxed);
            restore_native(&ctx, hwnd)
        } else if event.id == refreshid {
            r.store(true, Ordering::Relaxed);
            ctx.request_repaint()
        } else if event.id == exitid {
            q.store(true, Ordering::Relaxed);
            restore_native(&ctx, hwnd);
            ctx.send_viewport_cmd(ViewportCommand::Close)
        }
    }));
    let ctx = cc.egui_ctx.clone();
    let oid = open.clone();
    let data: Vec<u8> = (0..16 * 16)
        .flat_map(|i| {
            let x = i % 16;
            let y = i / 16;
            let on = x > 3 && x < 12 && y > 3 && y < 12;
            if on {
                [65_u8, 220, 172, 255]
            } else {
                [20, 33, 39, 0]
            }
        })
        .collect();
    let icon = Icon::from_rgba(data, 16, 16)?;
    let tray = TrayIconBuilder::new()
        .with_tooltip("Tokens and Code · local Codex telemetry")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .build()?;
    let id = tray.id().clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if event.id() == &id
            && matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            )
        {
            oid.store(true, Ordering::Relaxed);
            restore_native(&ctx, hwnd)
        }
    }));
    Ok(tray)
}
fn restore_native(ctx: &egui::Context, hwnd: isize) {
    unsafe {
        let h = hwnd as windows_sys::Win32::Foundation::HWND;
        if !h.is_null() && IsWindow(h) != 0 {
            ShowWindow(h, SW_RESTORE);
            SetForegroundWindow(h);
        }
    }
    ctx.send_viewport_cmd(ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(ViewportCommand::Focus);
    ctx.request_repaint();
}
fn metric(ui: &mut egui::Ui, label: &str, value: impl ToString) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label).color(Color32::GRAY));
        ui.strong(value.to_string());
    });
}
fn daily_chart(ui: &mut egui::Ui, snapshot: &Snapshot, metric: &mut String, source: &mut String) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("chart-metric")
            .selected_text(metric.as_str())
            .show_ui(ui, |ui| {
                for name in [
                    "Total tokens",
                    "Input",
                    "Cached input",
                    "Fresh input",
                    "Output",
                    "Reasoning output",
                    "Astra cost",
                    "Sol cost",
                    "Luna cost",
                ] {
                    ui.selectable_value(metric, name.to_string(), name);
                }
            });
        egui::ComboBox::from_id_salt("chart-source")
            .selected_text(source.as_str())
            .show_ui(ui, |ui| {
                for name in ["All sources", "Primary only", "Subagents only"] {
                    ui.selectable_value(source, name.to_string(), name);
                }
            });
    });
    let days = &snapshot.daily_history;
    let visible = &days[days.len().saturating_sub(30)..];
    if visible.is_empty() {
        ui.label("No token usage recorded yet.");
        return;
    }
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(300.0), 130.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    let value_for = |day: &scanner::Daily| -> f64 {
        let usage = if source == "All sources" {
            &day.stats.usage
        } else {
            let mut merged = scanner::Usage::default();
            for row in snapshot.drilldown.iter().filter(|r| {
                r.date == day.date
                    && ((source == "Primary only" && r.role == "primary")
                        || (source == "Subagents only" && r.role != "primary"))
            }) {
                merged.add(&row.usage);
            }
            return value_for_usage(&merged, metric);
        };
        value_for_usage(usage, metric)
    };
    let values: Vec<f64> = visible.iter().map(value_for).collect();
    let max = values.iter().copied().fold(0.0, f64::max).max(1.0);
    let gap = 3.0;
    let bw = ((rect.width() - gap * (visible.len() as f32 - 1.0)) / visible.len() as f32).max(2.0);
    for (i, d) in visible.iter().enumerate() {
        let value = values[i];
        let h = (value as f32 / max as f32 * (rect.height() - 26.0)).max(if value > 0.0 {
            2.0
        } else {
            0.0
        });
        let left = rect.left() + i as f32 * (bw + gap);
        let bar = egui::Rect::from_min_size(
            egui::pos2(left, rect.bottom() - 22.0 - h),
            egui::vec2(bw, h),
        );
        painter.rect_filled(bar, 2.0, Color32::from_rgb(65, 210, 169));
        if i % 5 == 0 || i + 1 == visible.len() {
            painter.text(
                egui::pos2(left, rect.bottom() - 18.0),
                egui::Align2::LEFT_TOP,
                d.date.get(5..).unwrap_or(&d.date),
                egui::FontId::proportional(10.0),
                Color32::GRAY,
            );
        }
    }
}
fn value_for_usage(usage: &scanner::Usage, metric: &str) -> f64 {
    match metric {
        "Input" => usage.input_tokens as f64,
        "Cached input" => usage.cached_input_tokens as f64,
        "Fresh input" => usage.fresh_input_tokens as f64,
        "Output" => usage.output_tokens as f64,
        "Reasoning output" => usage.reasoning_output_tokens as f64,
        "Astra cost" => usage
            .cost_usd
            .get("GPT-6 Astra")
            .copied()
            .unwrap_or_default(),
        "Sol cost" => usage.cost_usd.get("GPT-6 Sol").copied().unwrap_or_default(),
        "Luna cost" => usage
            .cost_usd
            .get("GPT-6 Luna")
            .copied()
            .unwrap_or_default(),
        _ => usage.total_tokens as f64,
    }
}
fn fmt(n: u64) -> String {
    n.to_string()
        .chars()
        .rev()
        .enumerate()
        .flat_map(|(i, c)| {
            if i > 0 && i % 3 == 0 {
                vec![',', c]
            } else {
                vec![c]
            }
        })
        .collect::<String>()
        .chars()
        .rev()
        .collect()
}
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s.to_string()
    }
}
fn main() -> eframe::Result {
    let opt = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 820.0])
            .with_min_inner_size([760.0, 560.0])
            .with_title(APP_NAME),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        opt,
        Box::new(|cc| Ok(Box::new(UsageApp::new(cc)))),
    )
}
