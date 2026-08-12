//! 通用工具函数

use std::path::{Path, PathBuf};

/// 生成输出文件路径：同目录下 xxx_converted.xlsx
pub fn generate_output_path(input_path: &Path) -> PathBuf {
    let parent = input_path.parent().unwrap_or(Path::new("."));
    let stem = input_path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = input_path.extension().unwrap_or_default().to_string_lossy();
    parent.join(format!("{}_converted.{}", stem, ext))
}

/// 检查文件是否为 xlsx
pub fn is_xlsx_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("xlsx"))
        .unwrap_or(false)
}
