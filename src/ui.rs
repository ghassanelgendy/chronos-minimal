use crate::models::{
    AppSettings, ScreenTimeData, format_seconds_display, get_day_summary_for_date,
    get_week_days, summarize_period, SummaryPeriod,
};
use crate::storage::{
    clear_all_data, export_data_snapshot, load_settings, save_settings,
};
use crate::startup;
use crate::supabase;
use chrono::{Local, NaiveDate};
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Tab {
    Activity,
    CloudSync,
    Preferences,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ViewMode {
    Day,
    ThisWeek,
    LastWeek,
    ThisMonth,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum FilterMode {
    All,
    AppsOnly,
    WebsitesOnly,
}

#[derive(Debug, Clone)]
pub struct GnomeThemeConfig {
    pub is_dark: bool,
    pub is_whitesur: bool,
    pub buttons_on_left: bool,
    pub has_close: bool,
    pub has_minimize: bool,
    pub has_maximize: bool,
    pub gtk_theme: String,
    pub color_scheme: String,
    pub button_layout: String,
}

impl Default for GnomeThemeConfig {
    fn default() -> Self {
        Self::detect()
    }
}

impl GnomeThemeConfig {
    pub fn detect() -> Self {
        #[cfg(target_os = "linux")]
        {
            let gtk_theme = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default()
                .trim()
                .trim_matches('\'')
                .to_string();

            let color_scheme = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "color-scheme"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default()
                .trim()
                .trim_matches('\'')
                .to_string();

            let button_layout = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .unwrap_or_default()
                .trim()
                .trim_matches('\'')
                .to_string();

            let is_dark = color_scheme.contains("dark") || gtk_theme.to_lowercase().contains("dark");
            let is_whitesur = gtk_theme.to_lowercase().contains("whitesur");

            // Format of button_layout is "left_buttons:right_buttons" (e.g. "close,minimize,maximize:")
            let (left_part, right_part) = match button_layout.split_once(':') {
                Some((l, r)) => (l, r),
                None => (button_layout.as_str(), ""),
            };

            let buttons_on_left = !left_part.is_empty();
            let active_part = if buttons_on_left { left_part } else { right_part };

            let has_close = active_part.contains("close");
            let has_minimize = active_part.contains("minimize");
            let has_maximize = active_part.contains("maximize");

            Self {
                is_dark,
                is_whitesur,
                buttons_on_left,
                has_close,
                has_minimize,
                has_maximize,
                gtk_theme,
                color_scheme,
                button_layout,
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            Self {
                is_dark: true,
                is_whitesur: false,
                buttons_on_left: false,
                has_close: true,
                has_minimize: true,
                has_maximize: true,
                gtk_theme: "Default".to_string(),
                color_scheme: "dark".to_string(),
                button_layout: ":minimize,maximize,close".to_string(),
            }
        }
    }
}

pub struct ChronosApp {
    pub data: Arc<std::sync::Mutex<ScreenTimeData>>,
    pub settings: Arc<std::sync::Mutex<AppSettings>>,
    pub tracking_enabled: Arc<AtomicBool>,
    pub tracker_running: Arc<AtomicBool>,
    pub show_dashboard_flag: Arc<AtomicBool>,
    pub gnome_theme: GnomeThemeConfig,
    pub selected_tab: Tab,
    pub selected_date: NaiveDate,
    pub view_mode: ViewMode,
    pub filter_mode: FilterMode,
    pub status_message: String,
    pub supabase_url: String,
    pub supabase_anon_key: String,
    pub supabase_user_id: String,
    pub upload_interval: u32,
    pub idle_threshold: u32,
    pub enable_sync: bool,
    pub start_at_logon: bool,
    pub start_minimized: bool,
    pub close_to_tray: bool,
    pub app_entry_installed: bool,
}

impl ChronosApp {
    pub fn new(
        data: Arc<std::sync::Mutex<ScreenTimeData>>,
        settings: Arc<std::sync::Mutex<AppSettings>>,
        tracking_enabled: Arc<AtomicBool>,
        tracker_running: Arc<AtomicBool>,
        show_dashboard_flag: Arc<AtomicBool>,
        initial_tab: Option<usize>,
    ) -> Self {
        let (url, key, uid, interval, idle, sync, logon, minimized, close_tray) = {
            let s = settings.lock().unwrap();
            (
                s.supabase_url.clone(),
                s.supabase_anon_key.clone(),
                s.supabase_user_id.clone(),
                s.supabase_upload_interval_minutes,
                s.idle_threshold_seconds,
                s.enable_supabase_sync,
                s.start_with_windows,
                s.start_minimized_to_tray,
                s.close_to_tray,
            )
        };

        let selected_tab = match initial_tab {
            Some(1) => Tab::CloudSync,
            Some(2) => Tab::Preferences,
            _ => Tab::Activity,
        };

        let today = Local::now().date_naive();
        let gnome_theme = GnomeThemeConfig::detect();

        Self {
            data,
            settings,
            tracking_enabled,
            tracker_running,
            show_dashboard_flag,
            gnome_theme,
            selected_tab,
            selected_date: today,
            view_mode: ViewMode::Day,
            filter_mode: FilterMode::All,
            status_message: String::new(),
            supabase_url: url,
            supabase_anon_key: key,
            supabase_user_id: uid,
            upload_interval: interval,
            idle_threshold: idle,
            enable_sync: sync,
            start_at_logon: logon,
            start_minimized: minimized,
            close_to_tray: close_tray,
            app_entry_installed: startup::is_app_entry_installed(),
        }
    }
}

pub fn apply_custom_theme(ctx: &egui::Context, config: &GnomeThemeConfig) {
    let mut visuals = if config.is_dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    if config.is_whitesur || config.is_dark {
        // macOS / WhiteSur-Dark inspired theme palette with sleek dark background
        visuals.panel_fill = egui::Color32::from_rgb(26, 26, 29);
        visuals.window_fill = egui::Color32::from_rgb(32, 32, 36);
        visuals.faint_bg_color = egui::Color32::from_rgb(22, 22, 24);
        visuals.extreme_bg_color = egui::Color32::from_rgb(16, 16, 18);

        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(34, 34, 38);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(48, 48, 54));
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);

        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(42, 42, 46);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(56, 56, 62));
        visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);

        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(58, 58, 64);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(80, 80, 88));
        visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);

        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(0, 122, 255);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(0, 100, 220));
        visuals.widgets.active.rounding = egui::Rounding::same(8.0);

        visuals.selection.bg_fill = egui::Color32::from_rgb(0, 122, 255);
        visuals.window_rounding = egui::Rounding::same(12.0);
    } else {
        visuals.panel_fill = egui::Color32::from_rgb(245, 245, 247);
        visuals.window_fill = egui::Color32::from_rgb(255, 255, 255);
        visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
        visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
        visuals.widgets.active.rounding = egui::Rounding::same(8.0);
        visuals.selection.bg_fill = egui::Color32::from_rgb(0, 122, 255);
        visuals.window_rounding = egui::Rounding::same(12.0);
    }

    ctx.set_visuals(visuals);
}

impl eframe::App for ChronosApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(std::time::Duration::from_secs(1));

        // Apply detected system theme
        apply_custom_theme(ctx, &self.gnome_theme);

        let is_focused = ctx.input(|i| i.focused);
        crate::tracker::IS_CHRONOS_FOCUSED.store(is_focused, Ordering::SeqCst);

        // Check if tray icon or background signal requested showing the dashboard window
        if self.show_dashboard_flag.swap(false, Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // Intercept close button (X) and minimize to tray/indicator if configured
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.close_to_tray {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }

        egui::TopBottomPanel::top("header_panel").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading("⏱ Chronos Screentime");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let is_tracking = self.tracking_enabled.load(Ordering::SeqCst);
                    let (btn_text, btn_color) = if is_tracking {
                        ("⏸ Pause Tracking", egui::Color32::from_rgb(180, 48, 48))
                    } else {
                        ("▶ Resume Tracking", egui::Color32::from_rgb(36, 140, 36))
                    };
                    if ui.add(egui::Button::new(btn_text).fill(btn_color)).clicked() {
                        self.tracking_enabled.store(!is_tracking, Ordering::SeqCst);
                    }
                });
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.selected_tab, Tab::Activity, "📊 Activity");
                ui.selectable_value(&mut self.selected_tab, Tab::CloudSync, "☁ Cloud Sync");
                ui.selectable_value(&mut self.selected_tab, Tab::Preferences, "⚙ Preferences");
            });
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.selected_tab {
                Tab::Activity => self.show_activity_tab(ui),
                Tab::CloudSync => self.show_cloud_sync_tab(ui),
                Tab::Preferences => self.show_preferences_tab(ui),
            }

            if !self.status_message.is_empty() {
                ui.add_space(8.0);
                ui.separator();
                ui.label(egui::RichText::new(&self.status_message).italics().color(egui::Color32::GRAY));
            }
        });
    }
}

impl ChronosApp {
    fn show_activity_tab(&mut self, ui: &mut egui::Ui) {
        let today = Local::now().date_naive();

        // ── Period / Mode Selector & Date Navigation ─────────────────────────
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("View:").strong());
            ui.selectable_value(&mut self.view_mode, ViewMode::Day, "📅 Day");
            ui.selectable_value(&mut self.view_mode, ViewMode::ThisWeek, "📊 This Week");
            ui.selectable_value(&mut self.view_mode, ViewMode::LastWeek, "⏮ Last Week");
            ui.selectable_value(&mut self.view_mode, ViewMode::ThisMonth, "📆 This Month");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Today").clicked() {
                    self.selected_date = today;
                    self.view_mode = ViewMode::Day;
                }
                if ui.button("▶").clicked() {
                    self.selected_date += chrono::Duration::days(1);
                    self.view_mode = ViewMode::Day;
                }
                if ui.button("◀").clicked() {
                    self.selected_date -= chrono::Duration::days(1);
                    self.view_mode = ViewMode::Day;
                }
            });
        });

        ui.add_space(4.0);

        // ── Interactive Days of the Week Navigation Bar ───────────────────────
        let data_guard = self.data.lock().unwrap();
        let week_days = get_week_days(&data_guard, self.selected_date);

        ui.group(|ui| {
            ui.horizontal(|ui| {
                if ui.small_button("« Prev Week").clicked() {
                    self.selected_date -= chrono::Duration::weeks(1);
                    self.view_mode = ViewMode::Day;
                }

                let day_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                for (idx, (d, total_secs)) in week_days.iter().enumerate() {
                    let is_selected = self.view_mode == ViewMode::Day && *d == self.selected_date;
                    let is_today = *d == today;
                    let day_label = format!(
                        "{}\n{} ({})",
                        day_names.get(idx).unwrap_or(&""),
                        d.format("%d"),
                        if *total_secs > 0 { format_seconds_display(*total_secs) } else { "-".to_string() }
                    );

                    let mut btn = egui::Button::new(
                        egui::RichText::new(&day_label).size(11.0).strong()
                    );
                    if is_selected {
                        btn = btn.fill(egui::Color32::from_rgb(0, 110, 220));
                    } else if is_today {
                        btn = btn.fill(egui::Color32::from_rgb(40, 80, 120));
                    }

                    if ui.add_sized([56.0, 36.0], btn).clicked() {
                        self.selected_date = *d;
                        self.view_mode = ViewMode::Day;
                    }
                }

                if ui.small_button("Next Week »").clicked() {
                    self.selected_date += chrono::Duration::weeks(1);
                    self.view_mode = ViewMode::Day;
                }
            });
        });

        ui.add_space(6.0);

        // ── Calculate Totals & Lines for selected View Mode ───────────────────
        let (period_title, total_seconds, total_switches, _total_apps, mut lines) = match self.view_mode {
            ViewMode::Day => {
                let (secs, lines, switches) = get_day_summary_for_date(&data_guard, self.selected_date);
                let title = if self.selected_date == today {
                    format!("Today ({})", self.selected_date.format("%A, %b %d, %Y"))
                } else if self.selected_date == today - chrono::Duration::days(1) {
                    format!("Yesterday ({})", self.selected_date.format("%A, %b %d, %Y"))
                } else {
                    self.selected_date.format("%A, %b %d, %Y").to_string()
                };
                let apps_count = lines.len() as u32;
                (title, secs, switches as u64, apps_count, lines)
            }
            ViewMode::ThisWeek => {
                let (totals, lines) = summarize_period(&data_guard, SummaryPeriod::ThisWeek, false);
                ("This Week".to_string(), totals.total_seconds, totals.total_switches, totals.total_apps, lines)
            }
            ViewMode::LastWeek => {
                let (totals, lines) = summarize_period(&data_guard, SummaryPeriod::LastWeek, false);
                ("Last Week".to_string(), totals.total_seconds, totals.total_switches, totals.total_apps, lines)
            }
            ViewMode::ThisMonth => {
                let (totals, lines) = summarize_period(&data_guard, SummaryPeriod::ThisMonth, false);
                ("This Month".to_string(), totals.total_seconds, totals.total_switches, totals.total_apps, lines)
            }
        };

        // Filter lines based on filter_mode
        match self.filter_mode {
            FilterMode::All => {}
            FilterMode::AppsOnly => {
                lines.retain(|l| !l.is_website);
            }
            FilterMode::WebsitesOnly => {
                lines.retain(|l| l.is_website);
            }
        }

        // ── Summary Card ─────────────────────────────────────────────────────
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&period_title).size(13.0).color(egui::Color32::GRAY));
                    ui.label(
                        egui::RichText::new(format_seconds_display(total_seconds))
                            .size(20.0)
                            .strong()
                            .color(egui::Color32::from_rgb(0, 160, 255)),
                    );
                });

                ui.add_space(24.0);
                ui.separator();
                ui.add_space(12.0);

                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Switches").size(12.0).color(egui::Color32::GRAY));
                    ui.label(egui::RichText::new(format!("{}", total_switches)).size(16.0).strong());
                });

                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new("Items Tracked").size(12.0).color(egui::Color32::GRAY));
                    ui.label(egui::RichText::new(format!("{}", lines.len())).size(16.0).strong());
                });
            });
        });

        ui.add_space(8.0);

        // ── Filter Controls & Ranked List ────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Filter:").strong());
            ui.selectable_value(&mut self.filter_mode, FilterMode::All, "All Items");
            ui.selectable_value(&mut self.filter_mode, FilterMode::AppsOnly, "📱 Applications");
            ui.selectable_value(&mut self.filter_mode, FilterMode::WebsitesOnly, "🌐 Websites");
        });

        ui.separator();

        if lines.is_empty() {
            ui.add_space(12.0);
            ui.label(egui::RichText::new("No activity recorded for this period.").italics());
        } else {
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("activity_grid")
                    .striped(true)
                    .min_col_width(120.0)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("#").strong());
                        ui.label(egui::RichText::new("Item Name").strong());
                        ui.label(egui::RichText::new("Type").strong());
                        ui.label(egui::RichText::new("Sessions").strong());
                        ui.label(egui::RichText::new("Duration").strong());
                        ui.label(egui::RichText::new("Share").strong());
                        ui.end_row();

                        let max_secs = if total_seconds > 0 { total_seconds as f32 } else { 1.0 };

                        for (idx, item) in lines.iter().enumerate() {
                            ui.label(format!("{}", idx + 1));
                            let icon_prefix = if item.is_website { "🌐 " } else { "📱 " };
                            ui.label(format!("{}{}", icon_prefix, item.name));
                            ui.label(if item.is_website { "Website" } else { "Application" });
                            ui.label(format!("{}", item.session_count));
                            ui.label(format_seconds_display(item.total_seconds));

                            let pct = (item.total_seconds as f32 / max_secs).clamp(0.0, 1.0);
                            ui.add(egui::ProgressBar::new(pct).show_percentage());
                            ui.end_row();
                        }
                    });
            });
        }
    }

    fn show_cloud_sync_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Supabase Cloud Sync Settings");
        ui.separator();

        let mut changed = false;
        if ui.checkbox(&mut self.enable_sync, "Enable Supabase Sync").changed() {
            changed = true;
        }

        ui.add_space(8.0);
        egui::Grid::new("sync_settings_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Supabase API URL:");
            if ui.text_edit_singleline(&mut self.supabase_url).changed() {
                changed = true;
            }
            ui.end_row();

            ui.label("Anon Client Key:");
            if ui.add(egui::TextEdit::singleline(&mut self.supabase_anon_key).password(true)).changed() {
                changed = true;
            }
            ui.end_row();

            ui.label("User Identifier:");
            if ui.text_edit_singleline(&mut self.supabase_user_id).changed() {
                changed = true;
            }
            ui.end_row();

            ui.label("Sync Interval (minutes):");
            if ui.add(egui::DragValue::new(&mut self.upload_interval).range(1..=10080)).changed() {
                changed = true;
            }
            ui.end_row();
        });

        if changed {
            let mut s = self.settings.lock().unwrap();
            s.enable_supabase_sync = self.enable_sync;
            s.supabase_url = self.supabase_url.clone();
            s.supabase_anon_key = self.supabase_anon_key.clone();
            s.supabase_user_id = self.supabase_user_id.clone();
            s.supabase_upload_interval_minutes = self.upload_interval;
            save_settings(&s);
        }

        ui.add_space(16.0);
        ui.horizontal(|ui| {
            if ui.button("⚡ Test Sync / Upload Now").clicked() {
                self.status_message = "Testing Supabase connection...".to_string();
                let url = self.supabase_url.clone();
                let key = self.supabase_anon_key.clone();
                let uid = self.supabase_user_id.clone();
                let data = self.data.lock().unwrap().clone();
                let device_id = std::env::var("COMPUTERNAME")
                    .or_else(|_| std::env::var("HOSTNAME"))
                    .unwrap_or_else(|_| "Linux-PC".to_string());

                let rt = tokio::runtime::Runtime::new();
                if let Ok(rt) = rt {
                    let result = rt.block_on(supabase::upload_screentime_data(
                        &data, &url, &key, &uid, &device_id, 0,
                    ));
                    if result.success {
                        self.status_message = format!(
                            "Success! Uploaded {} app logs, {} website logs.",
                            result.apps_inserted, result.websites_inserted
                        );
                    } else {
                        self.status_message = format!(
                            "Upload failed: {}",
                            result.error_message.unwrap_or_else(|| "Unknown error".to_string())
                        );
                    }
                }
            }
        });
    }

    fn show_preferences_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("Application Preferences");
        ui.separator();

        let mut changed = false;

        // ── System integration ───────────────────────────────────────────────
        ui.label(egui::RichText::new("System Integration").strong());
        ui.add_space(4.0);

        if ui.checkbox(&mut self.start_at_logon, "Start Chronos automatically at system logon").changed() {
            changed = true;
            if let Err(e) = startup::set_run_at_startup(self.start_at_logon) {
                self.status_message = format!("Failed setting autostart: {}", e);
            } else {
                self.status_message = format!("Autostart set to {}", self.start_at_logon);
            }
        }

        // App-drawer toggle (Linux / GNOME app launcher integration)
        if ui.checkbox(&mut self.app_entry_installed, "Show in application launcher (app drawer)").changed() {
            if self.app_entry_installed {
                match startup::install_app_entry() {
                    Ok(()) => {
                        self.status_message =
                            "Chronos added to app drawer. You may need to log out and back in, or run \
                             'update-desktop-database ~/.local/share/applications' to see it immediately."
                            .to_string();
                    }
                    Err(e) => {
                        self.app_entry_installed = false;
                        self.status_message = format!("Failed installing app entry: {}", e);
                    }
                }
            } else {
                match startup::uninstall_app_entry() {
                    Ok(()) => {
                        self.status_message = "Chronos removed from app drawer.".to_string();
                    }
                    Err(e) => {
                        self.app_entry_installed = true;
                        self.status_message = format!("Failed removing app entry: {}", e);
                    }
                }
            }
        }
        ui.label(
            egui::RichText::new(
                "Installs a .desktop entry + icon so Chronos appears in GNOME / KDE app launchers.",
            )
            .small()
            .italics(),
        );

        ui.add_space(4.0);
        if ui.checkbox(&mut self.close_to_tray, "Minimize to AppIndicator / System Tray on window close").changed() {
            changed = true;
        }

        ui.add_space(4.0);
        if ui.checkbox(&mut self.start_minimized, "Start minimized to AppIndicator / System Tray").changed() {
            changed = true;
        }

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.label("Idle Inactivity Threshold (seconds):");
            if ui.add(egui::DragValue::new(&mut self.idle_threshold).range(10..=3600)).changed() {
                changed = true;
            }
        });
        ui.label(egui::RichText::new("Pauses tracking automatically when no keyboard/mouse input is detected.").small().italics());

        if changed {
            let mut s = self.settings.lock().unwrap();
            s.start_with_windows = self.start_at_logon;
            s.start_minimized_to_tray = self.start_minimized;
            s.close_to_tray = self.close_to_tray;
            s.idle_threshold_seconds = self.idle_threshold;
            save_settings(&s);
        }

        ui.add_space(16.0);
        ui.heading("🎨 System Theme Integration");
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("GTK Theme:");
            ui.label(egui::RichText::new(&self.gnome_theme.gtk_theme).strong().color(egui::Color32::from_rgb(0, 150, 255)));
        });
        ui.horizontal(|ui| {
            ui.label("Color Scheme:");
            ui.label(egui::RichText::new(if self.gnome_theme.is_dark { "Dark (prefer-dark)" } else { "Light (prefer-light)" }).strong());
        });
        ui.horizontal(|ui| {
            ui.label("Window Button Layout:");
            ui.label(egui::RichText::new(if self.gnome_theme.buttons_on_left { "Left (macOS / WhiteSur Traffic Lights)" } else { "Right (Standard Controls)" }).strong());
        });

        ui.add_space(20.0);
        ui.heading("Data & Export Management");
        ui.separator();

        ui.horizontal(|ui| {
            if ui.button("📥 Export Data Snapshot").clicked() {
                let snapshot = self.data.lock().unwrap().clone();
                match export_data_snapshot(&snapshot) {
                    Ok(path) => {
                        self.status_message = format!("Exported snapshot to: {}", path.display());
                    }
                    Err(e) => {
                        self.status_message = format!("Export failed: {}", e);
                    }
                }
            }

            if ui.button("🗑 Reset All Local Tracking Data").clicked() {
                clear_all_data();
                *self.data.lock().unwrap() = ScreenTimeData::default();
                self.status_message = "All local tracking data has been cleared.".to_string();
            }
        });

        ui.add_space(20.0);
        ui.heading("Process Control");
        ui.separator();
        if ui.add(egui::Button::new("🛑 Halt & Exit Chronos Screentime").fill(egui::Color32::from_rgb(180, 40, 40))).clicked() {
            self.tracker_running.store(false, Ordering::SeqCst);
            std::process::exit(0);
        }
    }
}

pub fn show_dashboard_window(
    data: Arc<std::sync::Mutex<ScreenTimeData>>,
    settings: Arc<std::sync::Mutex<AppSettings>>,
    tracking_enabled: Arc<AtomicBool>,
    tracker_running: Arc<AtomicBool>,
    show_dashboard_flag: Arc<AtomicBool>,
    select_tab: Option<usize>,
    start_minimized: bool,
) {
    // Load the application icon (PNG bytes) for the window title bar / taskbar.
    let icon = load_app_icon();

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([550.0, 500.0])
        .with_title("Chronos Screentime")
        .with_app_id("chronos-screentime")
        .with_visible(!start_minimized);

    if let Some(icon_data) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon_data));
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Chronos Screentime",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(ChronosApp::new(
                data,
                settings,
                tracking_enabled,
                tracker_running,
                show_dashboard_flag,
                select_tab,
            )))
        }),
    );
}

/// Decode the bundled PNG icon into egui's IconData format.
fn load_app_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../icon-9.png");
    let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
    let img = img.into_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width: w,
        height: h,
    })
}

pub fn show_today_window(data: Arc<std::sync::Mutex<ScreenTimeData>>) {
    let settings = Arc::new(std::sync::Mutex::new(load_settings()));
    let tracking = Arc::new(AtomicBool::new(true));
    let running = Arc::new(AtomicBool::new(true));
    let restore = Arc::new(AtomicBool::new(false));
    show_dashboard_window(data, settings, tracking, running, restore, Some(0), false);
}
