#![allow(dead_code)] // 各测试目标仅用本模块部分助手，未用项不告警
//! 共享测试助手：合成 PDF 文档构造器、二进制运行、gs 渲染、PGM 解析、文本层计数、操作断言。
//! 本目录（common/）不会被 cargo 当作独立测试目标，仅作为各测试文件的子模块。

use lopdf::content::Content;
use lopdf::{dictionary, Document, Dictionary, Object, ObjectId, Stream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// 本进程唯一的临时文件路径
pub fn tmp_file(name: &str) -> PathBuf {
    let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join(format!("pdf-crop-dual-test-{}-{}", std::process::id(), n));
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    dir.join(name)
}

/// 运行二进制；返回 (退出码, stdout, stderr)
pub fn run_tool(args: &[&str]) -> (i32, String, String) {
    let bin = env!("CARGO_BIN_EXE_pdf-crop-dual");
    let o = Command::new(bin).args(args).output().expect("启动二进制失败");
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

// ===================== 合成文档构造器 =====================

/// Type1 简单字体（FirstChar + Widths）
pub fn type1_font(doc: &mut Document, first_char: i64, widths: &[f32]) -> ObjectId {
    let dict = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "TestFont",
        "FirstChar" => first_char,
        "Widths" => widths.iter().map(|w| Object::Real(*w)).collect::<Vec<_>>(),
    };
    doc.add_object(Object::Dictionary(dict))
}

/// CIDFont（DW + W 数组；w 为 /W 的完整内容）
pub fn cid_font(doc: &mut Document, dw: f32, w: Vec<Object>) -> ObjectId {
    let mut d = dictionary! {
        "Type" => "Font",
        "Subtype" => "CIDFontType2",
        "DW" => dw,
    };
    if !w.is_empty() {
        d.set("W", Object::Array(w));
    }
    doc.add_object(Object::Dictionary(d))
}

/// Type0 复合字体（DescendantFonts → cid）
pub fn type0_font(doc: &mut Document, cid_id: ObjectId) -> ObjectId {
    let dict = dictionary! {
        "Type" => "Font",
        "Subtype" => "Type0",
        "BaseFont" => "TestCID",
        "DescendantFonts" => vec![Object::Reference(cid_id)],
    };
    doc.add_object(Object::Dictionary(dict))
}

/// Form XObject 流（Matrix/BBox/Resources 可选）
pub fn form_xobject(
    doc: &mut Document,
    content: &[u8],
    matrix: Option<[f32; 6]>,
    bbox: Option<[f32; 4]>,
    resources: Option<&Dictionary>,
) -> ObjectId {
    let mut d = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Form",
        "FormType" => 1,
    };
    if let Some(m) = matrix {
        d.set("Matrix", Object::Array(m.iter().map(|v| Object::Real(*v)).collect()));
    }
    if let Some(b) = bbox {
        d.set("BBox", Object::Array(b.iter().map(|v| Object::Real(*v)).collect()));
    }
    if let Some(r) = resources {
        d.set("Resources", Object::Dictionary(r.clone()));
    }
    doc.add_object(Object::Stream(Stream::new(d, content.to_vec())))
}

/// Image XObject（仅字典，无真实图像数据即可被 Walk/重写器量测）
pub fn image_xobject(doc: &mut Document, matrix: [f32; 6]) -> ObjectId {
    let d = dictionary! {
        "Type" => "XObject",
        "Subtype" => "Image",
        "Matrix" => matrix.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
    };
    doc.add_object(Object::Dictionary(d))
}

/// 构建页面级 Resources（字体/XObject 条目均为间接引用）
pub fn page_resources(
    fonts: &[(&[u8], ObjectId)],
    xobjects: &[(&[u8], ObjectId)],
) -> Dictionary {
    let mut res = Dictionary::new();
    if !fonts.is_empty() {
        let mut f = Dictionary::new();
        for (n, id) in fonts {
            f.set(n.to_vec(), Object::Reference(*id));
        }
        res.set("Font", Object::Dictionary(f));
    }
    if !xobjects.is_empty() {
        let mut x = Dictionary::new();
        for (n, id) in xobjects {
            x.set(n.to_vec(), Object::Reference(*id));
        }
        res.set("XObject", Object::Dictionary(x));
    }
    res
}

/// 构造「Pages 父节点 + 单页」结构，返回 (page_id, parent_id)
pub fn page_tree(doc: &mut Document, page_dict: Dictionary, mut parent_extra: Dictionary) -> (ObjectId, ObjectId) {
    let page_id = doc.add_object(Object::Dictionary(page_dict));
    parent_extra.set("Type", "Pages");
    parent_extra.set("Kids", vec![Object::Reference(page_id)]);
    parent_extra.set("Count", 1);
    let parent_id = doc.add_object(Object::Dictionary(parent_extra));
    if let Ok(Object::Dictionary(d)) = doc.get_object_mut(page_id) {
        d.set("Parent", Object::Reference(parent_id));
    }
    (page_id, parent_id)
}

// ===================== gs 渲染 =====================

/// 探测 gs 是否可用（部分 shell 下 which 结果不可靠，直接执行探测）
pub fn gs_available() -> bool {
    Command::new("gs")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// gs 渲染单页为 72dpi PGM（1px = 1pt）
pub fn render_pgm_page(pdf: &Path, page: u32) -> Pgm {
    let out = tmp_file(&format!("p{page}.pgm"));
    // gs 10.x 对有值参数要求 `=` 形式（空格形式报 undefinedfilename/rangecheck）
    let o = Command::new("gs")
        .args(["-dNOPAUSE", "-dBATCH", "-dQUIET", "-sDEVICE=pgmraw", "-r72"])
        .arg(format!("-dFirstPage={page}"))
        .arg(format!("-dLastPage={page}"))
        .arg(format!("-sOutputFile={}", out.display()))
        .arg(pdf)
        .output()
        .expect("执行 gs 失败");
    // -dQUIET 下 gs 报错走 stdout，断言信息须双路合并
    let gs_log = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(o.status.success(), "gs 渲染失败: {gs_log}");
    // gs 打开输出文件失败时可能仍返回 0，须确认文件确实产出且非空
    let data = std::fs::read(&out).unwrap_or_else(|e| panic!("读取 PGM 失败({e}): {gs_log}"));
    assert!(!data.is_empty(), "PGM 输出为空: {gs_log}");
    Pgm::parse(&data).unwrap_or_else(|| panic!("解析 PGM 失败: {gs_log}"))
}

/// gs 渲染整份 PDF 的文本层（gs 10.x 无页分隔符，逐页定位用 render_txt_page）
pub fn render_txt_file(pdf: &Path) -> String {
    let out = tmp_file("full.txt");
    let o = Command::new("gs")
        .args(["-dNOPAUSE", "-dBATCH", "-dQUIET", "-sDEVICE=txtwrite"])
        .arg(format!("-sOutputFile={}", out.display()))
        .arg(pdf)
        .output()
        .expect("执行 gs 失败");
    let gs_log = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(o.status.success(), "gs txtwrite 失败: {gs_log}");
    std::fs::read_to_string(&out).unwrap_or_else(|e| panic!("读取文本层失败({e}): {gs_log}"))
}

/// gs 渲染单页文本层（-dFirstPage/-dLastPage 限定单页，用于逐页定位）
pub fn render_txt_page(pdf: &Path, page: u32) -> String {
    let out = tmp_file(&format!("tp{page}.txt"));
    let o = Command::new("gs")
        .args(["-dNOPAUSE", "-dBATCH", "-dQUIET", "-sDEVICE=txtwrite"])
        .arg(format!("-dFirstPage={page}"))
        .arg(format!("-dLastPage={page}"))
        .arg(format!("-sOutputFile={}", out.display()))
        .arg(pdf)
        .output()
        .expect("执行 gs 失败");
    let gs_log = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(o.status.success(), "gs 单页 txtwrite 失败: {gs_log}");
    std::fs::read_to_string(&out).unwrap_or_else(|e| panic!("读取单页文本层失败({e}): {gs_log}"))
}

/// 文本层码点数：过滤 [ \t\r\n\x00-\x1f]（即 U+00..=U+20）后数 UTF-8 码点
pub fn count_text_codepoints(s: &str) -> usize {
    s.chars().filter(|c| !matches!(*c as u32, 0x00..=0x20)).count()
}

// ===================== PGM =====================

/// P5 原始 PGM
#[derive(Debug, Clone, PartialEq)]
pub struct Pgm {
    pub width: usize,
    pub height: usize,
    pub maxval: u32,
    pub bytes: Vec<u8>,
}

impl Pgm {
    /// 解析 P5 头：逐 token 读取，'#' 注释行整体跳过；
    /// maxval 之后按 P5 规范恰好一个空白字符，其后为像素数据
    pub fn parse(data: &[u8]) -> Option<Pgm> {
        let mut pos = 0usize;
        let next_token = |pos: &mut usize| -> Option<String> {
            loop {
                while *pos < data.len() && (data[*pos] as char).is_ascii_whitespace() {
                    *pos += 1;
                }
                if *pos < data.len() && data[*pos] == b'#' {
                    while *pos < data.len() && data[*pos] != b'\n' {
                        *pos += 1;
                    }
                    continue;
                }
                let s = *pos;
                while *pos < data.len() && !(data[*pos] as char).is_ascii_whitespace() {
                    *pos += 1;
                }
                if s == *pos {
                    return None;
                }
                break Some(String::from_utf8_lossy(&data[s..*pos]).into_owned());
            }
        };
        if next_token(&mut pos)? != "P5" {
            return None;
        }
        let w: usize = next_token(&mut pos)?.parse().ok()?;
        let h: usize = next_token(&mut pos)?.parse().ok()?;
        let maxval: u32 = next_token(&mut pos)?.parse().ok()?;
        // 仅支持 8 位灰度；w*h 防溢出
        if maxval > 255 {
            return None;
        }
        let size = w.checked_mul(h)?;
        if pos < data.len() && (data[pos] as char).is_ascii_whitespace() {
            pos += 1;
        }
        if data.len() < pos + size {
            return None;
        }
        Some(Pgm {
            width: w,
            height: h,
            maxval,
            bytes: data[pos..pos + size].to_vec(),
        })
    }

    /// 暗像素（灰度 < 200）的最小/最大列；无暗像素返回 None
    pub fn dark_cols(&self) -> Option<(usize, usize)> {
        let mut min = usize::MAX;
        let mut max = 0usize;
        for y in 0..self.height {
            let row = y * self.width;
            for x in 0..self.width {
                if self.bytes[row + x] < 200 {
                    if x < min {
                        min = x;
                    }
                    if x > max {
                        max = x;
                    }
                }
            }
        }
        if min > max {
            None
        } else {
            Some((min, max))
        }
    }

    /// 手工裁剪：左段前 left_cols 列原样保留，其后整体取原始第 (left_cols + cut) 列起左移 cut 列。
    /// 调用方保证 left_cols/cut 为整数像素（72dpi 下 px == pt）。
    pub fn manual_crop(&self, left_cols: usize, cut: usize) -> Pgm {
        assert!(left_cols + cut <= self.width, "left_cols+cut 超出原宽");
        // PGM 为行主序：逐行裁剪（保留前 left_cols 列，删去其后 cut 列）
        let mut bytes = Vec::with_capacity((self.width - cut) * self.height);
        for row in self.bytes.chunks_exact(self.width) {
            bytes.extend_from_slice(&row[..left_cols]);
            bytes.extend_from_slice(&row[left_cols + cut..]);
        }
        Pgm {
            width: self.width - cut,
            height: self.height,
            maxval: self.maxval,
            bytes,
        }
    }
}

// ===================== Content 断言 =====================

/// 操作视图：操作符 + 数值/字符串/名称操作数（便于逐操作断言）。
/// 数组操作数逐元素摊平（如 TJ [(A) -20 (B)] → strs [A,B]、nums [-20]）；
/// 其他操作数类型（如 Dictionary）仍丢弃
#[derive(Debug, Clone, PartialEq)]
pub struct OpView {
    pub op: String,
    pub nums: Vec<f32>,
    pub strs: Vec<Vec<u8>>,
    pub names: Vec<Vec<u8>>,
}

pub fn view_ops(content: &Content) -> Vec<OpView> {
    content
        .operations
        .iter()
        .map(|o| {
            let mut nums = Vec::new();
            let mut strs = Vec::new();
            let mut names = Vec::new();
            let mut collect = |v: &Object| {
                match v {
                    Object::Real(f) => nums.push(*f),
                    Object::Integer(i) => nums.push(*i as f32),
                    Object::String(s, _) => strs.push(s.clone()),
                    Object::Name(n) => names.push(n.clone()),
                    _ => {}
                }
            };
            for v in &o.operands {
                if let Object::Array(items) = v {
                    for it in items {
                        collect(it);
                    }
                } else {
                    collect(v);
                }
            }
            OpView {
                op: o.operator.clone(),
                nums,
                strs,
                names,
            }
        })
        .collect()
}

/// 期望操作：(操作符, 数值操作数, 字符串操作数, 名称操作数)
pub type ExpOp = (&'static str, Vec<f32>, Vec<Vec<u8>>, Vec<Vec<u8>>);

/// 期望操作便捷构造（无字符串/名称操作数）
pub fn eo(op: &'static str, nums: Vec<f32>) -> ExpOp {
    (op, nums, Vec::new(), Vec::new())
}

/// 逐操作断言 Content 与期望一致（f32 容差 1e-3）
pub fn assert_ops(actual: &Content, expected: &[ExpOp], ctx: &str) {
    let a = view_ops(actual);
    assert_eq!(a.len(), expected.len(), "{ctx}: 操作数量不一致");
    for (i, (av, (eop, enums, estrs, enames))) in a.iter().zip(expected.iter()).enumerate() {
        assert_eq!(&av.op, eop, "{ctx}: 第 {i} 个操作符不一致");
        assert_eq!(av.nums.len(), enums.len(), "{ctx}: 操作 #{i} {eop} 数值操作数数量");
        for (j, (x, y)) in av.nums.iter().zip(enums.iter()).enumerate() {
            assert!(
                (x - y).abs() < 1e-3,
                "{ctx}: 操作 #{i} {eop} 参数 {j}: 实际 {x} 期望 {y}"
            );
        }
        assert_eq!(&av.strs, estrs, "{ctx}: 操作 #{i} {eop} 字符串操作数不一致");
        assert_eq!(&av.names, enames, "{ctx}: 操作 #{i} {eop} 名称操作数不一致");
    }
}
