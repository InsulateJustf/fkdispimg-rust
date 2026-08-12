//! WPS DISPIMG 图片转换核心逻辑
//! 直接操作 ZIP 包内的 XML，保留所有原有图片和绘图。

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;
use thiserror::Error;
use zip::read::ZipArchive;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

#[derive(Error, Debug)]
pub enum ConverterError {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("ZIP 错误: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("转换错误: {0}")]
    Conversion(String),
}

type Result<T> = std::result::Result<T, ConverterError>;

// 预编译正则表达式
static RE_RID_NUM: Lazy<Regex> = Lazy::new(|| Regex::new(r#"Id="rId(\d+)""#).unwrap());
static RE_TARGET: Lazy<Regex> = Lazy::new(|| Regex::new(r#"Target="([^"]+)""#).unwrap());
static RE_ANCHOR_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r#"<xdr:cNvPr id="(\d+)""#).unwrap());
static RE_REL_ENTRY: Lazy<Regex> = Lazy::new(|| Regex::new(r#"<Relationship[^>]+/>"#).unwrap());
static RE_NAME_ID: Lazy<Regex> = Lazy::new(|| Regex::new(r#"name="(ID_[^"]+)""#).unwrap());
static RE_EMBED_RID: Lazy<Regex> = Lazy::new(|| Regex::new(r#"r:embed="(rId\d+)""#).unwrap());
static RE_DRAWING_NUM: Lazy<Regex> = Lazy::new(|| Regex::new(r#"xl/drawings/drawing(\d+)\.xml$"#).unwrap());
static RE_SHEET_RELS: Lazy<Regex> = Lazy::new(|| Regex::new(r#"xl/worksheets/_rels/sheet(\d+)\.xml\.rels$"#).unwrap());
static RE_SHEET_XML: Lazy<Regex> = Lazy::new(|| Regex::new(r#"xl/worksheets/sheet(\d+)\.xml$"#).unwrap());

const DRAWING_CT: &str = "application/vnd.openxmlformats-officedocument.drawing+xml";

const WPS_DRAWING_TPL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
</xdr:wsDr>"#;

const EMPTY_RELS_TPL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
</Relationships>"#;

/// 将列字母转换为索引 (A=0, B=1, ..., Z=25, AA=26, ...)
fn col_to_idx(s: &str) -> usize {
    let mut idx = 0;
    for ch in s.chars() {
        idx = idx * 26 + (ch as usize - b'A' as usize + 1);
    }
    idx - 1
}

/// 解析单元格坐标 (如 "A1") 为 (列索引, 行索引)
fn parse_coord(coord: &str) -> (usize, usize) {
    let re = Regex::new(r#"([A-Z]+)(\d+)"#).unwrap();
    if let Some(caps) = re.captures(coord) {
        let col = col_to_idx(&caps[1]);
        let row: usize = caps[2].parse().unwrap_or(1) - 1;
        (col, row)
    } else {
        (0, 0)
    }
}

/// 构建图片锚点 XML
fn build_anchor(col: usize, row: usize, r_id: &str, aid: usize) -> String {
    format!(
        r#"<xdr:twoCellAnchor editAs="oneCell">
<xdr:from><xdr:col>{col}</xdr:col><xdr:colOff>0</xdr:colOff>
<xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>
<xdr:to><xdr:col>{col_plus1}</xdr:col><xdr:colOff>0</xdr:colOff>
<xdr:row>{row_plus1}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
<xdr:pic>
<xdr:nvPicPr><xdr:cNvPr id="{aid}" name="FK_Converted_{aid}"/>
<xdr:cNvPicPr><a:picLocks noChangeAspect="1"/></xdr:cNvPicPr></xdr:nvPicPr>
<xdr:blipFill><a:blip r:embed="{r_id}"/>
<a:stretch><a:fillRect/></a:stretch></xdr:blipFill>
<xdr:spPr><a:xfrm>
<a:off x="0" y="0"/><a:ext cx="914400" cy="914400"/>
</a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>
<a:noFill/><a:ln w="9525"><a:noFill/></a:ln></xdr:spPr>
</xdr:pic><xdr:clientData/></xdr:twoCellAnchor>"#,
        col = col,
        row = row,
        col_plus1 = col + 1,
        row_plus1 = row + 1,
        r_id = r_id,
        aid = aid
    )
}

/// 获取下一个 relationship ID
fn next_rid(xml: &str) -> usize {
    RE_RID_NUM
        .captures_iter(xml)
        .filter_map(|caps| caps[1].parse::<usize>().ok())
        .max()
        .map(|id| id + 1)
        .unwrap_or(1)
}

/// 获取下一个 anchor ID
fn next_aid(xml: &str) -> usize {
    RE_ANCHOR_ID
        .captures_iter(xml)
        .filter_map(|caps| caps[1].parse::<usize>().ok())
        .max()
        .map(|id| id + 1)
        .unwrap_or(1)
}

/// 添加 relationship 到 XML
fn add_rel(xml: String, rid: &str, target: &str, rtype: &str) -> String {
    let entry = format!(
        r#"<Relationship Id="{}" Type="{}" Target="{}"/>"#,
        rid, rtype, target
    );
    xml.replace("</Relationships>", &format!("{}\n</Relationships>", entry))
}

/// 查找指定 target 的 relationship ID
fn find_rid_for_target(xml: &str, target: &str) -> Option<String> {
    for m in RE_REL_ENTRY.find_iter(xml) {
        let entry = m.as_str();
        let id_caps = RE_RID_NUM.captures(entry)?;
        let t_caps = RE_TARGET.captures(entry)?;
        if t_caps.get(1).map_or(false, |m| m.as_str() == target) {
            return Some(format!("rId{}", &id_caps[1]));
        }
    }
    None
}

/// 查找并清除 DISPIMG 函数，返回 (单元格信息, 新内容)
fn find_and_clear_dispimg(content: &str, name_to_media: &HashMap<String, String>) -> (Vec<(String, String)>, String) {
    let mut result = Vec::new();
    let mut output = String::new();
    let mut i = 0;

    while i < content.len() {
        // 查找 <c 标签
        if let Some(c_start) = content[i..].find("<c ") {
            let c_start = i + c_start;
            // 写入 <c 之前的内容
            output.push_str(&content[i..c_start]);

            // 查找 </c> 结束标签
            if let Some(c_end) = content[c_start..].find("</c>") {
                let c_end = c_start + c_end + 4; // 包含 </c>
                let cell_full = &content[c_start..c_end];

                // 检查是否包含 DISPIMG
                if cell_full.contains("DISPIMG") {
                    let coord_re = Regex::new(r#"<c r="([A-Z]+\d+)""#).unwrap();
                    let img_re = Regex::new(r#"_xlfn\.DISPIMG\(&quot;([^&]+)&quot;"#).unwrap();

                    if let (Some(coord_caps), Some(img_caps)) = (coord_re.captures(cell_full), img_re.captures(cell_full)) {
                        let coord = coord_caps[1].to_string();
                        let img_name = img_caps[1].to_string();

                        if name_to_media.contains_key(&img_name) {
                            result.push((coord.clone(), img_name));

                            // 提取 <c r="XX" attrs> 的开头标签，转为自闭合
                            if let Some(tag_end) = cell_full.find('>') {
                                let open_tag = &cell_full[..tag_end];
                                output.push_str(open_tag);
                                output.push('/');
                                output.push('>');
                                i = c_end;
                                continue;
                            }
                        }
                    }
                }

                // 非 DISPIMG 单元格，原样写入
                output.push_str(cell_full);
                i = c_end;
            } else {
                // 没有闭合标签
                output.push_str(&content[c_start..]);
                break;
            }
        } else {
            output.push_str(&content[i..]);
            break;
        }
    }

    (result, output)
}

/// 安全地截取字符串到指定字节位置（确保在字符边界上）
fn safe_slice(s: &str, end: usize) -> &str {
    if end >= s.len() {
        return s;
    }
    // 找到最近的字符边界
    let mut end = end;
    while end < s.len() && !s.is_char_boundary(end) {
        end += 1;
    }
    &s[..end]
}

/// 主转换函数
pub fn wps_image_converter(
    input_xlsx: &Path,
    output_xlsx: &Path,
    log_callback: impl Fn(&str),
) -> std::result::Result<(bool, String, usize), ConverterError> {
    log_callback(&format!("正在加载表格: {} ...", input_xlsx.display()));

    if !input_xlsx.exists() {
        return Err(ConverterError::Conversion(format!("文件不存在: {}", input_xlsx.display())));
    }

    let file = File::open(input_xlsx)?;
    let reader = BufReader::new(file);
    let mut archive = ZipArchive::new(reader)?;

    let all_names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).map(|e| e.name().to_string()))
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // 检查是否包含 WPS 嵌入图片容器
    if !all_names.contains(&"xl/cellimages.xml".to_string()) {
        return Err(ConverterError::Conversion("未找到 WPS 嵌入图片容器".to_string()));
    }

    // 读取 cellimages.xml 和 cellimages.xml.rels
    let ci_xml = {
        let mut content = String::new();
        archive.by_name("xl/cellimages.xml")?.read_to_string(&mut content)?;
        content
    };

    let ci_rels = {
        let mut content = String::new();
        archive.by_name("xl/_rels/cellimages.xml.rels")?.read_to_string(&mut content)?;
        content
    };

    // 解析 rid 到 media 的映射
    let mut rid_to_media: HashMap<String, String> = HashMap::new();
    for m in RE_REL_ENTRY.find_iter(&ci_rels) {
        let entry = m.as_str();
        if let (Some(id_caps), Some(t_caps)) = (RE_RID_NUM.captures(entry), RE_TARGET.captures(entry)) {
            let rid_val = format!("rId{}", &id_caps[1]);
            let target = t_caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            rid_to_media.insert(rid_val, target);
        }
    }

    // 解析 name 到 media 的映射
    let mut name_to_media: HashMap<String, String> = HashMap::new();
    for m in RE_NAME_ID.find_iter(&ci_xml) {
        if let Some(caps) = RE_NAME_ID.captures(m.as_str()) {
            let img_name = caps[1].to_string();
            // 在 name 之后查找 embed rid（使用安全的字符切片）
            let after_name = safe_slice(&ci_xml[m.end()..], 500);
            if let Some(embed_caps) = RE_EMBED_RID.captures(after_name) {
                let embed_rid = embed_caps.get(1).map(|m| m.as_str()).unwrap_or("");
                if let Some(media) = rid_to_media.get(embed_rid) {
                    name_to_media.insert(img_name, media.clone());
                }
            }
        }
    }

    log_callback(&format!("  发现 {} 张 WPS 嵌入图片", name_to_media.len()));

    // 收集已使用的 drawing 编号
    let mut used_drawing_nums: HashSet<usize> = HashSet::new();
    for name in &all_names {
        if let Some(caps) = RE_DRAWING_NUM.captures(name) {
            if let Ok(num) = caps[1].parse() {
                used_drawing_nums.insert(num);
            }
        }
    }

    // 分配新的 drawing 编号
    let mut alloc_num = || -> usize {
        let mut n = 1;
        while used_drawing_nums.contains(&n) {
            n += 1;
        }
        used_drawing_nums.insert(n);
        n
    };

    // 收集 sheet drawing 信息
    let mut sheet_drawing: HashMap<String, SheetDrawingInfo> = HashMap::new();
    for name in &all_names {
        if let Some(caps) = RE_SHEET_RELS.captures(name) {
            let sheet_num = &caps[1];
            let mut rels_content = String::new();
            archive.by_name(name)?.read_to_string(&mut rels_content)?;

            let re = Regex::new(r#"Target="(\.\./drawings/drawing\d+\.xml)""#).unwrap();
            if let Some(t_caps) = re.captures(&rels_content) {
                let drawing_path = format!("xl/{}", t_caps[1].replace("../", ""));
                let drawing_rels = format!("{}.rels", drawing_path.replace("xl/drawings/", "xl/drawings/_rels/"));
                sheet_drawing.insert(
                    format!("sheet{}.xml", sheet_num),
                    SheetDrawingInfo {
                        drawing: drawing_path,
                        drawing_rels,
                        sheet_rels: name.clone(),
                    },
                );
            }
        }
    }

    // 执行转换
    let mut modified: HashMap<String, Vec<u8>> = HashMap::new();
    let mut new_drawings: Vec<String> = Vec::new();
    let mut total_count = 0;

    for sheet_path in all_names.clone() {
        if !RE_SHEET_XML.is_match(&sheet_path) {
            continue;
        }

        let mut content = String::new();
        archive.by_name(&sheet_path)?.read_to_string(&mut content)?;

        if !content.contains("DISPIMG") {
            continue;
        }

        let sheet_base = sheet_path.rsplit('/').next().unwrap_or(&sheet_path).to_string();
        let (dispimg_cells, new_content) = find_and_clear_dispimg(&content, &name_to_media);

        if dispimg_cells.is_empty() {
            continue;
        }

        log_callback(&format!("\n正在处理工作表: [{}] ({} 张图片)", sheet_base, dispimg_cells.len()));
        for (coord, _) in &dispimg_cells {
            log_callback(&format!("  > 清除公式: {}", coord));
        }

        let dinfo = sheet_drawing.get(&sheet_base);

        let (dp, rp) = if let Some(info) = dinfo {
            (info.drawing.clone(), info.drawing_rels.clone())
        } else {
            let num = alloc_num();
            let dp = format!("xl/drawings/drawing{}.xml", num);
            let rp = format!("xl/drawings/_rels/drawing{}.xml.rels", num);
            let srp = format!("xl/worksheets/_rels/{}.rels", sheet_base);

            modified.insert(dp.clone(), WPS_DRAWING_TPL.as_bytes().to_vec());
            modified.insert(rp.clone(), EMPTY_RELS_TPL.as_bytes().to_vec());
            new_drawings.push(dp.clone());

            let sr = if all_names.contains(&srp) {
                let mut content = String::new();
                archive.by_name(&srp)?.read_to_string(&mut content)?;
                content
            } else {
                EMPTY_RELS_TPL.to_string()
            };

            let sr_rid = next_rid(&sr);
            let sr = add_rel(
                sr,
                &format!("rId{}", sr_rid),
                &format!("../drawings/drawing{}.xml", num),
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing",
            );
            modified.insert(srp.clone(), sr.as_bytes().to_vec());

            content = content.replace(
                "</worksheet>",
                &format!(r#"<drawing r:id="rId{}"/></worksheet>"#, sr_rid),
            );

            (dp, rp)
        };

        // 处理 drawing XML
        let mut dx = if let Some(data) = modified.get(&dp) {
            String::from_utf8_lossy(data).to_string()
        } else if all_names.contains(&dp) {
            let mut content = String::new();
            archive.by_name(&dp)?.read_to_string(&mut content)?;
            content
        } else {
            WPS_DRAWING_TPL.to_string()
        };

        // 处理 drawing rels
        let mut dr = if let Some(data) = modified.get(&rp) {
            String::from_utf8_lossy(data).to_string()
        } else if all_names.contains(&rp) {
            let mut content = String::new();
            archive.by_name(&rp)?.read_to_string(&mut content)?;
            content
        } else {
            EMPTY_RELS_TPL.to_string()
        };

        let mut nrid = next_rid(&dr);
        let mut naid = next_aid(&dx);

        for (coord, img_name) in &dispimg_cells {
            if let Some(media) = name_to_media.get(img_name) {
                let target = format!("../{}", media);
                let img_rid = if let Some(existing) = find_rid_for_target(&dr, &target) {
                    existing
                } else {
                    let rid = format!("rId{}", nrid);
                    dr = add_rel(
                        dr.clone(),
                        &rid,
                        &target,
                        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image",
                    );
                    nrid += 1;
                    rid
                };

                let (col, row) = parse_coord(coord);
                let anchor = build_anchor(col, row, &img_rid, naid);
                dx = dx.replace("</xdr:wsDr>", &format!("{}\n</xdr:wsDr>", anchor));
                naid += 1;
                total_count += 1;
                log_callback(&format!("  > 插入图片: {} <- {}", coord, media));
            }
        }

        modified.insert(sheet_path, new_content.as_bytes().to_vec());
        modified.insert(dp, dx.as_bytes().to_vec());
        modified.insert(rp, dr.as_bytes().to_vec());
    }

    // 更新 Content_Types.xml
    if !new_drawings.is_empty() {
        let ct_path = "[Content_Types].xml";
        let mut ct_xml = String::new();
        archive.by_name(ct_path)?.read_to_string(&mut ct_xml)?;

        for drawing_path in &new_drawings {
            let part_name = format!("/{}", drawing_path);
            if !ct_xml.contains(&part_name) {
                let override_entry = format!(
                    r#"<Override PartName="{}" ContentType="{}"/>"#,
                    part_name, DRAWING_CT
                );
                ct_xml = ct_xml.replace("</Types>", &format!("{}\n</Types>", override_entry));
            }
        }
        modified.insert(ct_path.to_string(), ct_xml.as_bytes().to_vec());
    }

    // 写入输出文件
    log_callback(&format!("\n正在保存到: {} ...", output_xlsx.display()));

    let out_file = File::create(output_xlsx)?;
    let writer = BufWriter::new(out_file);
    let mut zip_writer = ZipWriter::new(writer);

    // 重新读取所有文件
    let mut all_file_data: HashMap<String, Vec<u8>> = HashMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        all_file_data.insert(name, data);
    }

    for name in &all_names {
        let data = modified.get(name).unwrap_or_else(|| all_file_data.get(name).unwrap());
        let options = SimpleFileOptions::default();
        zip_writer.start_file(name, options)?;
        zip_writer.write_all(data)?;
    }

    // 写入新文件
    for (path, data) in &modified {
        if !all_names.contains(path) {
            zip_writer.start_file(path, SimpleFileOptions::default())?;
            zip_writer.write_all(data)?;
        }
    }

    zip_writer.finish()?;

    let msg = format!("全部任务完成！累计修复 {} 张图片。", total_count);
    log_callback(&"=".repeat(50));
    log_callback(&msg);
    Ok((true, msg, total_count))
}

#[derive(Debug)]
struct SheetDrawingInfo {
    drawing: String,
    drawing_rels: String,
    sheet_rels: String,
}
