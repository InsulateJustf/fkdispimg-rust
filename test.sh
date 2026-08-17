#!/bin/bash
# 测试 FK_DISPIMG Rust 版本

echo "=== FK_DISPIMG Rust 版本测试 ==="
echo ""

# 检查可执行文件是否存在
if [ ! -f "./target/release/fkdisp" ]; then
    echo "错误：找不到可执行文件"
    exit 1
fi

echo "可执行文件大小："
ls -lh ./target/release/fkdisp
echo ""

# 测试命令行模式
echo "测试命令行模式..."
if [ -f "../1.xlsx" ]; then
    echo "使用测试文件: ../1.xlsx"
    ./target/release/fkdisp "../1.xlsx"
else
    echo "警告：找不到测试文件 ../1.xlsx"
fi

echo ""
echo "测试完成！"
