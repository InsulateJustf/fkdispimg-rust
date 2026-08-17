//! GUI 主应用

use crate::converter;
use crate::utils;
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone)]
enum ConversionState {
    Idle,
    Running,
    Done { success: usize, fail: usize, total: usize },
}

pub struct FkDispApp {
    files: Vec<PathBuf>,
    state: ConversionState,
    log_messages: Arc<Mutex<Vec<String>>>,
    scroll_to_bottom: Arc<Mutex<bool>>,
    drop_hover: bool,
}

impl FkDispApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            files: Vec::new(),
            state: ConversionState::Idle,
            log_messages: Arc::new(Mutex::new(Vec::new())),
            scroll_to_bottom: Arc::new(Mutex::new(false)),
            drop_hover: false,
        }
    }

    fn add_files(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            if path.is_file() && utils::is_xlsx_file(&path) && !self.files.contains(&path) {
                self.files.push(path);
            } else if path.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&path) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_file() && utils::is_xlsx_file(&p) && !self.files.contains(&p) {
                            let name = p.file_name().unwrap_or_default().to_string_lossy();
                            if !name.starts_with("~$") { self.files.push(p); }
                        }
                    }
                }
            }
        }
    }

    fn start_conversion(&mut self, ctx: &egui::Context) {
        if matches!(self.state, ConversionState::Running) || self.files.is_empty() { return; }
        self.state = ConversionState::Running;
        if let Ok(mut log) = self.log_messages.lock() { log.clear(); }

        let files = self.files.clone();
        let log_messages = self.log_messages.clone();
        let scroll_to_bottom = self.scroll_to_bottom.clone();
        let ctx = ctx.clone();

        thread::spawn(move || {
            let total = files.len();
            let mut ok = 0;
            let mut fail = 0;

            for (i, input) in files.iter().enumerate() {
                let output = utils::generate_output_path(input);
                log_msg(&log_messages, &scroll_to_bottom, &format!("[{}/{}] {}", i + 1, total, input.file_name().unwrap_or_default().to_string_lossy()));
                match converter::wps_image_converter(input, &output, |msg| { log_msg(&log_messages, &scroll_to_bottom, msg); }) {
                    Ok((true, _, _)) => ok += 1,
                    Ok((false, msg, _)) => { fail += 1; log_msg(&log_messages, &scroll_to_bottom, &format!("  ✗ {}", msg)); }
                    Err(e) => { fail += 1; log_msg(&log_messages, &scroll_to_bottom, &format!("  ✗ {}", e)); }
                }
                ctx.request_repaint();
            }

            log_msg(&log_messages, &scroll_to_bottom, &"-".repeat(30));
            log_msg(&log_messages, &scroll_to_bottom, &format!("完成: 成功 {}，失败 {}，共 {}", ok, fail, total));
            log_msg(&log_messages, &scroll_to_bottom, &format!("__DONE__{}|{}|{}", ok, fail, total));
            ctx.request_repaint();
        });
    }

    fn check_done(&mut self) {
        if let Ok(log) = self.log_messages.lock() {
            for msg in log.iter() {
                if let Some(rest) = msg.strip_prefix("__DONE__") {
                    let p: Vec<&str> = rest.split('|').collect();
                    if p.len() == 3 {
                        self.state = ConversionState::Done {
                            success: p[0].parse().unwrap_or(0),
                            fail: p[1].parse().unwrap_or(0),
                            total: p[2].parse().unwrap_or(0),
                        };
                    }
                    break;
                }
            }
        }
    }
}

fn log_msg(messages: &Arc<Mutex<Vec<String>>>, scroll_to_bottom: &Arc<Mutex<bool>>, msg: &str) {
    if let Ok(mut log) = messages.lock() { log.push(msg.to_string()); }
    if let Ok(mut scroll) = scroll_to_bottom.lock() { *scroll = true; }
}

impl eframe::App for FkDispApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_done();

        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() { self.add_files(dropped); }
        self.drop_hover = ctx.input(|i| !i.raw.hovered_files.is_empty());

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("FK_DISPIMG - WPS嵌入图片转换器");
            ui.separator();

            // 上部：文件列表区域
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.strong("待转换文件");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("{} 个文件", self.files.len()));
                    });
                });

                ui.horizontal(|ui| {
                    let r = matches!(self.state, ConversionState::Running);
                    if ui.add_enabled(!r, egui::Button::new("添加文件")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Excel", &["xlsx"]).pick_files() { self.add_files(p); }
                    }
                    if ui.add_enabled(!r, egui::Button::new("添加文件夹")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() { self.add_files(vec![p]); }
                    }
                    if ui.add_enabled(!r, egui::Button::new("清空")).clicked() { self.files.clear(); }
                });

                egui::ScrollArea::vertical()
                    .max_height(100.0)
                    .min_scrolled_height(100.0)
                    .show(ui, |ui| {
                        let mut rm = None;
                        for (i, p) in self.files.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let filename = p.file_name().unwrap_or_default().to_string_lossy();
                                ui.label(format!("{}. {}", i + 1, filename));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("X").clicked() { rm = Some(i); }
                                });
                            });
                        }
                        if let Some(i) = rm { self.files.remove(i); }
                    });
            });

            ui.add_space(8.0);

            // 转换按钮
            let r = matches!(self.state, ConversionState::Running);
            let text = match &self.state {
                ConversionState::Running => "转换中...".to_string(),
                ConversionState::Done { success, fail, .. } => {
                    if *fail == 0 { format!("完成 (成功 {})", success) }
                    else { format!("完成 (成功{} 失败{})", success, fail) }
                }
                ConversionState::Idle => "开始转换".to_string(),
            };
            let color = match &self.state {
                ConversionState::Running => egui::Color32::from_rgb(100, 100, 100),
                ConversionState::Done { fail, .. } => {
                    if *fail == 0 { egui::Color32::from_rgb(46, 125, 50) } else { egui::Color32::from_rgb(230, 81, 0) }
                }
                ConversionState::Idle => egui::Color32::from_rgb(33, 150, 243),
            };

            let btn = egui::Button::new(egui::RichText::new(text).size(16.0).color(egui::Color32::WHITE))
                .fill(color)
                .min_size(egui::vec2(ui.available_width(), 36.0));
            if ui.add_enabled(!r && !self.files.is_empty(), btn).clicked() {
                self.state = ConversionState::Idle;
                self.start_conversion(ctx);
            }

            ui.add_space(8.0);

            // 拖拽区
            let drop_height = 36.0_f32;
            let (drop_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), drop_height), egui::Sense::hover());
            let bg = if self.drop_hover { egui::Color32::from_rgb(227, 242, 253) } else { egui::Color32::from_rgb(250, 250, 250) };
            ui.painter().rect_filled(drop_rect, 4.0, bg);
            ui.painter().rect_stroke(drop_rect, 4.0, egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(200, 200, 200)));
            ui.painter().text(
                drop_rect.center(),
                egui::Align2::CENTER_CENTER,
                "拖拽 .xlsx 文件到此处",
                egui::FontId::proportional(13.0),
                egui::Color32::GRAY,
            );

            ui.add_space(8.0);

            // 下部：日志区域 - 使用 group 包裹，让 ScrollArea 填充剩余空间
            ui.group(|ui| {
                ui.strong("转换日志");
                ui.separator();
                
                // 使用 available_height 获取剩余空间，减去标题和分隔线的高度
                let log_height = (ui.available_height() - 10.0).max(100.0);
                
                // 关键：stick_to_bottom(true) 会在有新内容时自动滚动到底部
                let scroll_area = egui::ScrollArea::vertical()
                    .max_height(log_height)
                    .stick_to_bottom(true);
                
                scroll_area.show(ui, |ui| {
                    if let Ok(log) = self.log_messages.lock() {
                        let has_content = log.iter().any(|m| !m.starts_with("__DONE__"));
                        if !has_content {
                            ui.colored_label(egui::Color32::GRAY, "等待转换...");
                        } else {
                            for msg in log.iter() {
                                if !msg.starts_with("__DONE__") {
                                    ui.monospace(msg);
                                }
                            }
                        }
                    }
                    
                    // 检查是否需要滚动到底部
                    if let Ok(mut scroll) = self.scroll_to_bottom.lock() {
                        if *scroll {
                            // 在日志内容后添加一个不可见的锚点，并滚动到它
                            ui.label("");
                            ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                            *scroll = false;
                        }
                    }
                });
            });
        });
    }
}
