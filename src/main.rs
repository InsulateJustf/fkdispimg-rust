//! FK_DISPIMG - WPS 嵌入图片转换器
//! 将 WPS Office 创建的 XLSX 文件中的图片转换为标准格式

// Windows下隐藏控制台窗口
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod converter;
mod utils;

use app::FkDispApp;
use std::path::PathBuf;

fn main() {
    // 初始化日志
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 {
        // 命令行模式：直接转换
        let files: Vec<PathBuf> = args[1..]
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_file() && utils::is_xlsx_file(p))
            .collect();

        if !files.is_empty() {
            batch_convert_cli(files);
            return;
        }
    }

    // GUI 模式
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([660.0, 600.0])
            .with_min_inner_size([560.0, 480.0])
            .with_title("FK_DISPIMG - WPS嵌入图片转换器"),
        ..Default::default()
    };

    eframe::run_native(
        "FK_DISPIMG",
        options,
        Box::new(|cc| {
            // 设置中文字体支持
            setup_fonts(&cc.egui_ctx);
            Ok(Box::new(FkDispApp::new(cc)))
        }),
    )
    .unwrap();
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 尝试加载系统中文字体
    #[cfg(target_os = "macos")]
    {
        let font_paths = [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ];

        for path in &font_paths {
            if let Ok(font_data) = std::fs::read(path) {
                fonts.font_data.insert(
                    "chinese".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "chinese".to_owned());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, "chinese".to_owned());
                break;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let font_paths = [
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simsun.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
        ];

        for path in &font_paths {
            if let Ok(font_data) = std::fs::read(path) {
                fonts.font_data.insert(
                    "chinese".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "chinese".to_owned());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, "chinese".to_owned());
                break;
            }
        }
    }

    ctx.set_fonts(fonts);
}

fn batch_convert_cli(files: Vec<PathBuf>) {
    let total = files.len();
    println!("批量转换 {} 个文件...", total);

    let mut success_count = 0;
    let mut fail_count = 0;

    for (i, input) in files.iter().enumerate() {
        let output = utils::generate_output_path(input);
        println!("\n[{}/{}] {}", i + 1, total, input.display());

        match converter::wps_image_converter(input, &output, |msg| {
            println!("{}", msg);
        }) {
            Ok((true, _, _)) => {
                success_count += 1;
            }
            Ok((false, msg, _)) => {
                fail_count += 1;
                println!("  ✗ 失败: {}", msg);
            }
            Err(e) => {
                fail_count += 1;
                println!("  ✗ 异常: {}", e);
            }
        }
    }

    println!("\n{}", "=".repeat(50));
    println!(
        "批量转换完成！成功 {} 个，失败 {} 个，共 {} 个文件。",
        success_count, fail_count, total
    );
}
