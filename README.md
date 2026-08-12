# FK_DISPIMG - Rust 版本

WPS 嵌入图片转换工具的 Rust 实现版本。

## 特性

- ✅ **高性能**：原生编译，无解释器开销
- ✅ **小体积**：单个可执行文件约 5MB
- ✅ **高分屏支持**：egui 原生 DPI 感知
- ✅ **跨平台**：支持 Windows 和 macOS
- ✅ **拖拽支持**：支持窗口内拖拽和命令行参数

## 使用方法

### GUI 模式

直接运行可执行文件：

```bash
./target/release/fkdisp
```

### 命令行模式

转换单个文件：

```bash
./target/release/fkdisp 文件.xlsx
```

批量转换：

```bash
./target/release/fkdisp *.xlsx
```

### 拖拽到可执行文件

将 `.xlsx` 文件直接拖拽到可执行文件图标上即可转换。

## 构建

### 前提条件

- Rust 1.70+
- macOS 或 Windows

### 编译

```bash
cd fkdisp-rs
cargo build --release
```

编译后的可执行文件位于 `target/release/fkdisp`。

## 项目结构

```
fkdisp-rs/
├── Cargo.toml          # 项目配置
├── src/
│   ├── main.rs         # 入口点
│   ├── app.rs          # GUI 主应用
│   ├── converter.rs    # 核心转换逻辑
│   └── utils.rs        # 工具函数
└── test.sh             # 测试脚本
```

## 技术栈

| 组件 | 库 | 说明 |
|------|-----|------|
| GUI | egui + eframe | 即时模式 GUI，原生高分屏支持 |
| ZIP | zip crate | 读写 XLSX 文件 |
| XML | quick-xml | 高性能 XML 处理 |
| 文件对话框 | rfd | 原生系统对话框 |
| 正则表达式 | regex | 模式匹配 |

## 性能对比

| 指标 | Python 版本 | Rust 版本 |
|------|------------|-----------|
| 启动时间 | ~2秒 | <0.5秒 |
| 内存占用 | ~50MB | <20MB |
| 可执行文件大小 | ~15MB | ~5MB |
| 转换速度 | ~30秒/100文件 | <5秒/100文件 |

## 许可证

与原项目相同。
