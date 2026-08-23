use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Dictionary, Object, ObjectId, Stream};
use std::collections::{HashMap, HashSet};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("用法: {} <输入.pdf> <输出.pdf> <中间空白宽度>", args[0]);
        eprintln!("  中间空白宽度: 要裁剪掉的中间空白区域的宽度（PDF 点单位，如 80）");
        eprintln!("  程序会自动检测每页真实空白的位置；指定宽度超过实际空白时自动收敛为实际值");
        eprintln!("  示例: {} input-dual.pdf output.pdf 80", args[0]);
        std::process::exit(1);
    }

    let input_path = &args[1];
    let output_path = &args[2];
    let gap_width: f32 = args[3].parse().expect("空白宽度必须是数字");

    let mut doc = Document::load(input_path).expect("无法加载 PDF");
    let pages = doc.get_pages();

    println!("共 {} 页，准备裁剪中间空白（指定宽度 = {} pt）", pages.len(), gap_width);

    // ---------- 第一遍：扫描每页内容，检测真实中间空白 ----------
    struct PagePlan {
        page_num: u32,
        page_id: ObjectId,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        gap: Option<(f32, f32)>,
    }
    let mut plans: Vec<PagePlan> = Vec::new();

    for (page_num, &page_id) in pages.iter() {
        let page_obj = doc.get_object(page_id).expect("获取页面对象失败");
        let page_dict = match page_obj {
            Object::Dictionary(d) => d.clone(),
            _ => {
                eprintln!("跳过第 {} 页：页面对象不是字典", page_num);
                continue;
            }
        };
        let mediabox = match get_mediabox(&doc, &page_dict, page_id) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("跳过第 {} 页：无法获取 MediaBox: {}", page_num, e);
                continue;
            }
        };
        let (x1, y1, x2, y2) = (mediabox[0], mediabox[1], mediabox[2], mediabox[3]);

        let gap = match (doc.get_page_content(page_id), page_resources_dict(&doc, &page_dict)) {
            (Ok(content), Some(res)) => {
                let mut walker = Walk::new(&doc);
                walker.walk(&content, Some(res), 0);
                let g = detect_gap(&walker.intervals, x1, x2);
                match g {
                    Some((l, r)) => println!(
                        "第 {} 页：检测到中间空白 [{:.1}, {:.1}]（宽 {:.1} pt）",
                        page_num,
                        l,
                        r,
                        r - l
                    ),
                    None => println!("第 {} 页：未检测到明显空白，按页面对称处理", page_num),
                }
                g
            }
            (Err(e), _) => {
                eprintln!("跳过第 {} 页：无法获取内容流: {}", page_num, e);
                continue;
            }
            (Ok(_), None) => None,
        };

        plans.push(PagePlan {
            page_num: *page_num,
            page_id,
            x1,
            y1,
            x2,
            y2,
            gap,
        });
    }

    // 实际移除宽度不超过所有页的最小空白宽度，保证每页输出宽度一致
    let min_detected = plans
        .iter()
        .filter_map(|p| p.gap)
        .map(|(l, r)| r - l)
        .fold(f32::INFINITY, f32::min);
    let cut = gap_width.min(min_detected);
    if cut <= 1.0 {
        eprintln!("错误：空白宽度必须大于 1 pt，且页面需存在足够的中间空白");
        std::process::exit(1);
    }
    if min_detected.is_finite() && (cut - gap_width).abs() > 0.005 {
        println!(
            "提示：最窄页面的空白仅 {:.1} pt，移除宽度由 {:.1} 调整为 {:.1} pt",
            min_detected, gap_width, cut
        );
    }

    // ---------- 第二遍：重建每页内容流 ----------
    for plan in &plans {
        let PagePlan {
            page_num,
            page_id,
            x1,
            y1,
            x2,
            y2,
            gap,
        } = *plan;
        let total_width = x2 - x1;
        let height = y2 - y1;

        if total_width <= cut {
            eprintln!(
                "  跳过：页面宽度 ({:.1}) 小于等于移除宽度 ({:.1})",
                total_width, cut
            );
            continue;
        }

        // 移除区域：优先在检测到的真实空白内居中，否则退回页面对称
        let (band_left, band_right) = match gap {
            Some((l, r)) => {
                let c = (l + r) / 2.0;
                (c - cut / 2.0, c + cut / 2.0)
            }
            None => {
                let s = x1 + (total_width - cut) / 2.0;
                (s, s + cut)
            }
        };
        let band_left = band_left.max(x1).min(x2);
        let band_right = band_right.max(x1).min(x2);
        let new_width = total_width - cut;

        println!(
            "  原宽: {:.1}, 新宽: {:.1}, 移除区域: [{:.1}, {:.1}]",
            total_width, new_width, band_left, band_right
        );

        let page_obj = doc.get_object(page_id).expect("获取页面对象失败");
        let page_dict = match page_obj {
            Object::Dictionary(d) => d.clone(),
            _ => continue,
        };

        // 获取原始页面内容流（已解码合并）
        let original_content = match doc.get_page_content(page_id) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  跳过：无法获取内容流: {}", e);
                continue;
            }
        };

        // 获取 Resources（字体、图片等）
        let resources = match get_resources(&doc, &page_dict, page_id) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  跳过：无法获取 Resources: {}", e);
                continue;
            }
        };

        // 创建 Form XObject（将原页面内容封装进去）
        let form_name = format!("FormX{}", page_num);
        let form_name_bytes = form_name.into_bytes();

        let mut form_dict = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "FormType" => 1,
            "BBox" => vec![x1.into(), y1.into(), x2.into(), y2.into()],
        };

        // 将 Resources 复制到 Form XObject，确保字体等资源可用
        if let Ok(res_dict) = resources.as_dict() {
            form_dict.set("Resources", Object::Dictionary(res_dict.clone()));
        }

        let form_stream = Stream::new(form_dict, original_content);
        let form_id = doc.add_object(form_stream);

        // 确保页面有 Resources 对象，并在其中注册 XObject
        let resources_id = match page_dict.get(b"Resources") {
            Ok(Object::Reference(id)) => *id,
            Ok(Object::Dictionary(d)) => {
                // 内联字典 → 提取为独立对象
                doc.add_object(Object::Dictionary(d.clone()))
            }
            _ => doc.add_object(Dictionary::new()),
        };

        // 更新页面对 Resources 的引用
        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(page_id) {
            d.set("Resources", Object::Reference(resources_id));
        }

        // 在 Resources 中添加 XObject 条目
        if let Ok(Object::Dictionary(res_dict)) = doc.get_object_mut(resources_id) {
            let mut xobjects = match res_dict.get(b"XObject") {
                Ok(Object::Dictionary(xo)) => xo.clone(),
                _ => Dictionary::new(),
            };
            xobjects.set(form_name_bytes.clone(), Object::Reference(form_id));
            res_dict.set("XObject", Object::Dictionary(xobjects));
        }

        // 构建新的内容流：
        // 注意裁剪矩形必须在 cm 之前定义（新页面坐标系），否则会随平移一起偏移
        let new_content = Content {
            operations: vec![
                // ===== 左半边：保留 [x1, band_left] =====
                Operation::new("q", vec![]),
                Operation::new("re", vec![
                    x1.into(),
                    y1.into(),
                    (band_left - x1).into(),
                    height.into()
                ]),
                Operation::new("W", vec![]),
                Operation::new("n", vec![]),
                Operation::new("Do", vec![Object::Name(form_name_bytes.clone())]),
                Operation::new("Q", vec![]),

                // ===== 右半边：保留 [band_right, x2]，整体左移 cut =====
                Operation::new("q", vec![]),
                Operation::new("re", vec![
                    band_left.into(),
                    y1.into(),
                    (x2 - cut - band_left).into(),
                    height.into()
                ]),
                Operation::new("W", vec![]),
                Operation::new("n", vec![]),
                Operation::new("cm", vec![
                    1.0.into(),
                    0.0.into(),
                    0.0.into(),
                    1.0.into(),
                    (-cut).into(),
                    0.0.into()
                ]),
                Operation::new("Do", vec![Object::Name(form_name_bytes)]),
                Operation::new("Q", vec![]),
            ],
        };

        let new_content_stream = Stream::new(dictionary! {}, new_content.encode().unwrap());
        let new_content_id = doc.add_object(new_content_stream);

        // 更新页面字典：替换 Contents、MediaBox、CropBox
        if let Ok(Object::Dictionary(d)) = doc.get_object_mut(page_id) {
            d.set("Contents", Object::Reference(new_content_id));
            d.set("MediaBox", Object::Array(vec![
                x1.into(),
                y1.into(),
                (x2 - cut).into(),
                y2.into()
            ]));
            if d.has(b"CropBox") {
                d.set("CropBox", Object::Array(vec![
                    x1.into(),
                    y1.into(),
                    (x2 - cut).into(),
                    y2.into()
                ]));
            }
            d.remove(b"TrimBox");
            d.remove(b"BleedBox");
            d.remove(b"ArtBox");
        }
    }

    // 压缩并保存
    doc.compress();
    doc.save(output_path).expect("保存 PDF 失败");

    println!("完成！输出文件: {}", output_path);
}

/// 获取页面的 MediaBox，优先从页面字典获取，否则从父 Pages 节点继承
fn get_mediabox(
    doc: &Document,
    page_dict: &Dictionary,
    page_id: ObjectId,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let obj = page_dict.get(b"MediaBox").or_else(|_| {
        // 尝试从父节点获取
        if let Ok(Object::Reference(parent_id)) = page_dict.get(b"Parent") {
            if let Ok(Object::Dictionary(parent_dict)) = doc.get_object(*parent_id) {
                return parent_dict.get(b"MediaBox");
            }
        }
        Err(lopdf::Error::ObjectNotFound(page_id))
    })?;

    let (_, resolved) = doc.dereference(obj)?;
    let arr = resolved.as_array()?;

    arr.iter()
        .map(|obj| {
            // as_float 同时兼容 Integer 和 Real（as_f32 只接受 Real）
            obj.as_float()
                .map_err(|e| format!("MediaBox 包含非数字值: {}", e).into())
        })
        .collect()
}

/// 获取页面的 Resources，优先从页面字典获取，否则从父 Pages 节点继承
fn get_resources(
    doc: &Document,
    page_dict: &Dictionary,
    page_id: ObjectId,
) -> Result<Object, Box<dyn std::error::Error>> {
    let obj = page_dict.get(b"Resources").or_else(|_| {
        if let Ok(Object::Reference(parent_id)) = page_dict.get(b"Parent") {
            if let Ok(Object::Dictionary(parent_dict)) = doc.get_object(*parent_id) {
                return parent_dict.get(b"Resources");
            }
        }
        Err(lopdf::Error::ObjectNotFound(page_id))
    })?;

    Ok(doc.dereference(obj)?.1.clone())
}

/// 借用方式获取页面 Resources 字典（用于内容扫描）
fn page_resources_dict<'a>(
    doc: &'a Document,
    page_dict: &'a Dictionary,
) -> Option<&'a Dictionary> {
    let mut res_obj: Option<&'a Object> = page_dict.get(b"Resources").ok();
    if res_obj.is_none() {
        if let Ok(p) = page_dict.get(b"Parent") {
            if let Ok(id) = p.as_reference() {
                if let Ok(pd) = doc.get_object(id).and_then(|o| o.as_dict()) {
                    res_obj = pd.get(b"Resources").ok();
                }
            }
        }
    }
    res_obj.and_then(|o| match o {
        Object::Dictionary(d) => Some(d),
        Object::Reference(id) => doc.get_object(*id).ok().and_then(|o2| o2.as_dict().ok()),
        _ => None,
    })
}

/// Object 转 f32（兼容 Integer 和 Real）
fn onum(o: &Object) -> Option<f32> {
    match o {
        Object::Real(f) => Some(*f),
        Object::Integer(i) => Some(*i as f32),
        _ => None,
    }
}

/// 获取对象字典（Stream 对象附带字典，as_dict 无法处理）
fn obj_dict(o: &Object) -> Option<&Dictionary> {
    match o {
        Object::Dictionary(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

// ===================== 内容范围扫描 =====================

/// PDF 3x2 矩阵（行向量约定：p' = p·M）
#[derive(Clone, Copy)]
struct Mat {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Mat {
    const I: Mat = Mat {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    fn of(a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) -> Mat {
        Mat {
            a,
            b,
            c,
            d,
            e,
            f,
        }
    }

    /// 先应用 m1 再应用 m2（p·m1·m2）
    fn mul(m1: Mat, m2: Mat) -> Mat {
        Mat::of(
            m1.a * m2.a + m1.b * m2.c,
            m1.a * m2.b + m1.b * m2.d,
            m1.c * m2.a + m1.d * m2.c,
            m1.c * m2.b + m1.d * m2.d,
            m1.e * m2.a + m1.f * m2.c + m2.e,
            m1.e * m2.b + m1.f * m2.d + m2.f,
        )
    }

    fn x_of(&self, x: f32, y: f32) -> f32 {
        self.a * x + self.c * y + self.e
    }
}

#[derive(Clone)]
enum FontInfo {
    Simple {
        first: i64,
        widths: Vec<f32>,
    },
    Cid {
        widths: HashMap<u16, f32>,
        dw: f32,
    },
    Unknown,
}

fn glyph_width(font: &FontInfo, code: u32) -> f32 {
    match font {
        FontInfo::Simple { first, widths } => {
            let i = code as i64 - *first;
            if i >= 0 && (i as usize) < widths.len() {
                widths[i as usize]
            } else {
                1000.0
            }
        }
        FontInfo::Cid { widths, dw } => widths.get(&(code as u16)).copied().unwrap_or(*dw),
        FontInfo::Unknown => 1000.0,
    }
}

fn parse_w_entry(arr: &[Object], widths: &mut HashMap<u16, f32>) {
    let nums: Vec<f32> = arr.iter().filter_map(onum).collect();
    if arr.len() == 2 && nums.len() == 2 {
        widths.insert(nums[0] as u16, nums[1]);
    } else if arr.len() == 3 && nums.len() == 3 {
        let (first, last) = (nums[0] as u16, nums[1] as u16);
        if first <= last {
            for c in first..=last {
                widths.insert(c, nums[2]);
            }
        }
    }
    // [first [w1 w2 ...]] 形式
    if let Some(inner) = arr.last().and_then(|o| o.as_array().ok()) {
        if let Some(first) = arr.first().and_then(onum) {
            for (i, w) in inner
                .iter()
                .enumerate()
                .filter_map(|(i, o)| onum(o).map(|w| (i, w)))
            {
                widths.insert((first as u16).wrapping_add(i as u16), w);
            }
        }
    }
}

/// 取数组前 count 个数值元素
fn nums_at(a: &[Object], count: usize) -> Option<Vec<f32>> {
    (0..count).map(|i| a.get(i).and_then(onum)).collect()
}

fn build_font_info(doc: &Document, d: &Dictionary) -> FontInfo {
    let subtype = d.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
    match subtype {
        Some(s) if s == b"Type0" || s == b"Type0C" => {
            let mut widths: HashMap<u16, f32> = HashMap::new();
            let mut dw = 1000.0f32;
            let arr: Option<&Vec<Object>> = match d.get(b"DescendantFonts") {
                Ok(Object::Array(a)) => Some(a),
                Ok(Object::Reference(id)) => {
                    doc.get_object(*id).ok().and_then(|o| o.as_array().ok())
                }
                _ => None,
            };
            if let Some(arr) = arr {
                let cid = match arr.first() {
                    Some(Object::Reference(id)) => {
                        doc.get_object(*id).ok().and_then(|o| o.as_dict().ok())
                    }
                    Some(Object::Dictionary(dd)) => Some(dd),
                    _ => None,
                };
                if let Some(cd) = cid {
                    if let Ok(o) = cd.get(b"DW") {
                        if let Some(v) = onum(o) {
                            dw = v;
                        }
                    }
                    if let Ok(w) = cd.get(b"W").and_then(|o| o.as_array()) {
                        for sub in w {
                            if let Ok(arr2) = sub.as_array() {
                                parse_w_entry(arr2, &mut widths);
                            }
                        }
                    }
                }
            }
            FontInfo::Cid { widths, dw }
        }
        _ => {
            let first = d
                .get(b"FirstChar")
                .ok()
                .and_then(|o| onum(o))
                .map(|v| v as i64)
                .unwrap_or(0);
            let widths: Vec<f32> = d
                .get(b"Widths")
                .ok()
                .and_then(|o| o.as_array().ok())
                .map(|a| a.iter().filter_map(onum).collect())
                .unwrap_or_default();
            if widths.is_empty() {
                FontInfo::Unknown
            } else {
                FontInfo::Simple { first, widths }
            }
        }
    }
}

// ===================== 内容流词法分析 =====================

#[derive(Debug)]
enum Val {
    Num(f32),
    Name(Vec<u8>),
    Str(Vec<u8>),
    Arr(Vec<Val>),
}

enum Item {
    Op(String),
    Val(Val),
}

enum Parsed {
    Val(Val),
    Word(Vec<u8>),
    DictSkipped,
    Err,
}

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' || b == 0x0C || b == 0
}

fn is_delim(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

struct Tok<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Tok<'a> {
    fn skip_ws_comments(&mut self) {
        loop {
            while self.pos < self.data.len() && is_ws(self.data[self.pos]) {
                self.pos += 1;
            }
            if self.pos < self.data.len() && self.data[self.pos] == b'%' {
                while self.pos < self.data.len()
                    && !matches!(self.data[self.pos], b'\n' | b'\r')
                {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<u8> {
        self.data.get(self.pos + 1).copied()
    }

    fn read_word(&mut self) -> Vec<u8> {
        let s = self.pos;
        while self.pos < self.data.len()
            && !is_ws(self.data[self.pos])
            && !is_delim(self.data[self.pos])
        {
            self.pos += 1;
        }
        self.data[s..self.pos].to_vec()
    }

    fn parse_name(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(b) = self.peek() {
            if is_ws(b) || is_delim(b) {
                break;
            }
            if b == b'#' {
                let hv = self.peek2().and_then(|c| (c as char).to_digit(16));
                let lv = self
                    .data
                    .get(self.pos + 2)
                    .copied()
                    .and_then(|c| (c as char).to_digit(16));
                if let (Some(h), Some(l)) = (hv, lv) {
                    out.push((h * 16 + l) as u8);
                    self.pos += 3;
                    continue;
                }
            }
            out.push(b);
            self.pos += 1;
        }
        out
    }

    fn parse_num(&mut self) -> Option<f32> {
        let s = self.pos;
        if matches!(self.peek(), Some(b'+') | Some(b'-')) {
            self.pos += 1;
        }
        let mut has = false;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() || b == b'.' {
                self.pos += 1;
                has = true;
            } else {
                break;
            }
        }
        if !has {
            return None;
        }
        std::str::from_utf8(&self.data[s..self.pos]).ok()?.parse().ok()
    }

    fn parse_lit_string(&mut self) -> Option<Vec<u8>> {
        self.pos += 1; // '('
        let mut out = Vec::new();
        let mut depth = 1usize;
        while let Some(b) = self.peek() {
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b);
                    self.pos += 1;
                }
                b')' => {
                    depth -= 1;
                    self.pos += 1;
                    if depth == 0 {
                        return Some(out);
                    }
                    out.push(b);
                }
                b'\\' => {
                    self.pos += 1;
                    match self.peek()? {
                        b'n' => {
                            out.push(b'\n');
                            self.pos += 1;
                        }
                        b'r' => {
                            out.push(b'\r');
                            self.pos += 1;
                        }
                        b't' => {
                            out.push(b'\t');
                            self.pos += 1;
                        }
                        b'b' => {
                            out.push(0x08);
                            self.pos += 1;
                        }
                        b'f' => {
                            out.push(0x0C);
                            self.pos += 1;
                        }
                        b'(' | b')' | b'\\' => {
                            out.push(self.peek()?);
                            self.pos += 1;
                        }
                        b'\r' => {
                            self.pos += 1;
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {
                            self.pos += 1;
                        }
                        c if (b'0'..=b'7').contains(&c) => {
                            let mut v: u32 = 0;
                            let mut n = 0;
                            while n < 3 {
                                match self.peek() {
                                    Some(d) if (b'0'..=b'7').contains(&d) => {
                                        v = v * 8 + (d - b'0') as u32;
                                        self.pos += 1;
                                        n += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        other => {
                            out.push(other);
                            self.pos += 1;
                        }
                    }
                }
                _ => {
                    out.push(b);
                    self.pos += 1;
                }
            }
        }
        None
    }

    fn parse_hex_string(&mut self) -> Option<Vec<u8>> {
        self.pos += 1; // '<'
        let mut hex = Vec::new();
        while let Some(b) = self.peek() {
            if b == b'>' {
                self.pos += 1;
                break;
            }
            if is_ws(b) {
                self.pos += 1;
                continue;
            }
            if (b as char).is_ascii_hexdigit() {
                hex.push(b);
                self.pos += 1;
            } else {
                return None;
            }
        }
        if hex.len() % 2 == 1 {
            hex.push(b'0');
        }
        let mut out = Vec::with_capacity(hex.len() / 2);
        for i in (0..hex.len()).step_by(2) {
            let h = (hex[i] as char).to_digit(16)?;
            let l = (hex[i + 1] as char).to_digit(16)?;
            out.push((h * 16 + l) as u8);
        }
        Some(out)
    }

    /// 解析单个值；裸词返回 Word，字典整体跳过
    fn parse_val(&mut self) -> Parsed {
        self.skip_ws_comments();
        let b = match self.peek() {
            Some(b) => b,
            None => return Parsed::Err,
        };
        match b {
            b'/' => {
                self.pos += 1;
                Parsed::Val(Val::Name(self.parse_name()))
            }
            b'(' => match self.parse_lit_string() {
                Some(s) => Parsed::Val(Val::Str(s)),
                None => Parsed::Err,
            },
            b'<' => {
                if self.peek2() == Some(b'<') {
                    self.pos += 2;
                    if self.skip_dict() {
                        Parsed::DictSkipped
                    } else {
                        Parsed::Err
                    }
                } else {
                    match self.parse_hex_string() {
                        Some(s) => Parsed::Val(Val::Str(s)),
                        None => Parsed::Err,
                    }
                }
            }
            b'[' => {
                self.pos += 1;
                let mut arr = Vec::new();
                loop {
                    self.skip_ws_comments();
                    match self.peek() {
                        None => return Parsed::Err,
                        Some(b']') => {
                            self.pos += 1;
                            break;
                        }
                        _ => {}
                    }
                    match self.parse_val() {
                        Parsed::Val(v) => arr.push(v),
                        Parsed::Word(_) | Parsed::DictSkipped => {}
                        Parsed::Err => return Parsed::Err,
                    }
                }
                Parsed::Val(Val::Arr(arr))
            }
            b'+' | b'-' | b'.' | b'0'..=b'9' => match self.parse_num() {
                Some(n) => Parsed::Val(Val::Num(n)),
                None => Parsed::Err,
            },
            _ => Parsed::Word(self.read_word()),
        }
    }

    /// 跳过整个字典（键值对）
    fn skip_dict(&mut self) -> bool {
        loop {
            self.skip_ws_comments();
            match self.peek() {
                None => return false,
                Some(b'>') if self.peek2() == Some(b'>') => {
                    self.pos += 2;
                    return true;
                }
                _ => {}
            }
            if self.peek() == Some(b'/') {
                self.pos += 1;
                self.parse_name();
            } else {
                self.read_word();
            }
            self.skip_ws_comments();
            match self.peek() {
                Some(b'[') => {
                    self.pos += 1;
                    let mut depth = 1;
                    while let Some(b) = self.peek() {
                        self.pos += 1;
                        if b == b'[' {
                            depth += 1;
                        }
                        if b == b']' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                    }
                }
                Some(b'(') => {
                    let _ = self.parse_lit_string();
                }
                Some(b'<') => {
                    if self.peek2() == Some(b'<') {
                        self.pos += 2;
                        if !self.skip_dict() {
                            return false;
                        }
                    } else {
                        let _ = self.parse_hex_string();
                    }
                }
                Some(b) if b.is_ascii_digit() || b == b'+' || b == b'-' || b == b'.' => {
                    let _ = self.parse_num();
                    // 对象引用 "1 0 R"
                    self.skip_ws_comments();
                    if let Some(b) = self.peek() {
                        if b.is_ascii_digit() || b == b'+' || b == b'-' || b == b'.' {
                            let _ = self.parse_num();
                            self.skip_ws_comments();
                            if self.peek().map_or(false, |b| !is_ws(b) && !is_delim(b)) {
                                self.read_word();
                            }
                        }
                    }
                }
                Some(b'/') => {
                    self.pos += 1;
                    self.parse_name();
                }
                Some(_) => {
                    self.read_word();
                }
                None => return false,
            }
        }
    }

    fn next_item(&mut self) -> Option<Item> {
        match self.parse_val() {
            Parsed::Val(v) => Some(Item::Val(v)),
            Parsed::Word(w) if !w.is_empty() => {
                Some(Item::Op(String::from_utf8_lossy(&w).into_owned()))
            }
            Parsed::Word(_) => self.next_item(),
            Parsed::DictSkipped => self.next_item(),
            Parsed::Err => None,
        }
    }

    /// 处理内联图像 BI ... ID <原始字节> EI
    fn handle_inline_image(&mut self) -> bool {
        self.skip_ws_comments();
        if self.peek() != Some(b'<') || self.peek2() != Some(b'<') {
            return false;
        }
        self.pos += 2;
        let mut length: Option<usize> = None;
        loop {
            self.skip_ws_comments();
            match self.peek() {
                None => return false,
                Some(b'>') if self.peek2() == Some(b'>') => {
                    self.pos += 2;
                    break;
                }
                _ => {}
            }
            let key = if self.peek() == Some(b'/') {
                self.pos += 1;
                self.parse_name()
            } else {
                self.read_word()
            };
            self.skip_ws_comments();
            if key == b"Length" {
                if let Parsed::Val(Val::Num(n)) = self.parse_val() {
                    length = Some(n as usize);
                }
            } else {
                let _ = self.parse_val();
            }
        }
        self.skip_ws_comments();
        if self.peek() != Some(b'I') {
            return false;
        }
        if self.read_word() != b"ID" {
            return false;
        }
        // ID 后跟一个 EOL
        if self.peek() == Some(b'\r') {
            self.pos += 1;
            if self.peek() == Some(b'\n') {
                self.pos += 1;
            }
        } else if self.peek() == Some(b'\n') {
            self.pos += 1;
        }
        let l = match length {
            Some(l) => l,
            None => return false,
        };
        if self.pos + l > self.data.len() {
            return false;
        }
        self.pos += l;
        self.skip_ws_comments();
        if self.peek() != Some(b'E') {
            return false;
        }
        self.read_word() == b"EI"
    }
}

// ===================== 内容流遍历（收集墨迹 x 区间） =====================

struct Walk<'a> {
    doc: &'a Document,
    intervals: Vec<(f32, f32)>,
    ctm_stack: Vec<Mat>,
    ctm: Mat,
    ts_stack: Vec<(f32, f32, f32, f32, f32)>,
    tfs: f32,
    tw: f32,
    tc: f32,
    tz: f32,
    tl: f32,
    in_text: bool,
    tlm: Mat,
    font: Option<ObjectId>,
    fonts: HashMap<ObjectId, FontInfo>,
    path: Vec<(f32, f32)>,
    seen_forms: HashSet<ObjectId>,
}

impl<'a> Walk<'a> {
    fn new(doc: &'a Document) -> Self {
        Walk {
            doc,
            intervals: Vec::new(),
            ctm_stack: Vec::new(),
            ctm: Mat::I,
            ts_stack: Vec::new(),
            tfs: 0.0,
            tw: 0.0,
            tc: 0.0,
            tz: 100.0,
            tl: 0.0,
            in_text: false,
            tlm: Mat::I,
            font: None,
            fonts: HashMap::new(),
            path: Vec::new(),
            seen_forms: HashSet::new(),
        }
    }

    fn walk(&mut self, data: &[u8], resources: Option<&'a Dictionary>, depth: usize) {
        let mut tk = Tok { data, pos: 0 };
        let mut operands: Vec<Val> = Vec::new();
        loop {
            match tk.next_item() {
                Some(Item::Val(v)) => operands.push(v),
                Some(Item::Op(op)) => {
                    if op == "BI" {
                        operands.clear();
                        if !tk.handle_inline_image() {
                            return;
                        }
                    } else {
                        self.exec(&op, &operands, resources, depth);
                        operands.clear();
                    }
                }
                None => return,
            }
        }
    }

    fn num(args: &[Val], i: usize) -> Option<f32> {
        match args.get(i) {
            Some(Val::Num(n)) => Some(*n),
            _ => None,
        }
    }

    fn exec(
        &mut self,
        op: &str,
        args: &[Val],
        resources: Option<&'a Dictionary>,
        depth: usize,
    ) {
        use Val::*;
        match op {
            "q" => {
                self.ctm_stack.push(self.ctm);
                self.ts_stack
                    .push((self.tfs, self.tw, self.tc, self.tz, self.tl));
            }
            "Q" => {
                if let Some(c) = self.ctm_stack.pop() {
                    self.ctm = c;
                }
                if let Some(t) = self.ts_stack.pop() {
                    (self.tfs, self.tw, self.tc, self.tz, self.tl) = t;
                }
            }
            "cm" => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    Self::num(args, 0),
                    Self::num(args, 1),
                    Self::num(args, 2),
                    Self::num(args, 3),
                    Self::num(args, 4),
                    Self::num(args, 5),
                ) {
                    self.ctm = Mat::mul(Mat::of(a, b, c, d, e, f), self.ctm);
                }
            }
            "re" => {
                if let (Some(x), Some(y), Some(w), Some(h)) = (
                    Self::num(args, 0),
                    Self::num(args, 1),
                    Self::num(args, 2),
                    Self::num(args, 3),
                ) {
                    self.path = vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
                }
            }
            "m" => {
                if let (Some(x), Some(y)) = (Self::num(args, 0), Self::num(args, 1)) {
                    self.path = vec![(x, y)];
                }
            }
            "l" => {
                if let (Some(x), Some(y)) = (Self::num(args, 0), Self::num(args, 1)) {
                    self.path.push((x, y));
                }
            }
            "c" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2), Some(x3), Some(y3)) = (
                    Self::num(args, 0),
                    Self::num(args, 1),
                    Self::num(args, 2),
                    Self::num(args, 3),
                    Self::num(args, 4),
                    Self::num(args, 5),
                ) {
                    self.path.extend([(x1, y1), (x2, y2), (x3, y3)]);
                }
            }
            "v" | "y" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
                    Self::num(args, 0),
                    Self::num(args, 1),
                    Self::num(args, 2),
                    Self::num(args, 3),
                ) {
                    self.path.extend([(x1, y1), (x2, y2)]);
                }
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" => {
                if !self.path.is_empty() {
                    let mut min = f32::INFINITY;
                    let mut max = f32::NEG_INFINITY;
                    for &(x, y) in &self.path {
                        let px = self.ctm.x_of(x, y);
                        min = min.min(px);
                        max = max.max(px);
                    }
                    self.intervals.push((min, max));
                }
                self.path.clear();
            }
            "W" | "W*" | "n" => self.path.clear(),
            "BT" => {
                self.in_text = true;
                self.tlm = Mat::I;
            }
            "ET" => self.in_text = false,
            "Tm" => {
                if !self.in_text {
                    return;
                }
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    Self::num(args, 0),
                    Self::num(args, 1),
                    Self::num(args, 2),
                    Self::num(args, 3),
                    Self::num(args, 4),
                    Self::num(args, 5),
                ) {
                    self.tlm = Mat::of(a, b, c, d, e, f);
                }
            }
            "Td" => {
                if !self.in_text {
                    return;
                }
                if let (Some(tx), Some(ty)) = (Self::num(args, 0), Self::num(args, 1)) {
                    self.tlm = Mat::mul(Mat::of(1.0, 0.0, 0.0, 1.0, tx, ty), self.tlm);
                }
            }
            "TD" => {
                if !self.in_text {
                    return;
                }
                if let (Some(tx), Some(ty)) = (Self::num(args, 0), Self::num(args, 1)) {
                    self.tl = -ty;
                    self.tlm = Mat::mul(Mat::of(1.0, 0.0, 0.0, 1.0, tx, ty), self.tlm);
                }
            }
            "T*" => {
                if !self.in_text {
                    return;
                }
                self.tlm =
                    Mat::mul(Mat::of(1.0, 0.0, 0.0, 1.0, 0.0, -self.tl), self.tlm);
            }
            "TL" => self.tl = Self::num(args, 0).unwrap_or(0.0),
            "Tw" => self.tw = Self::num(args, 0).unwrap_or(0.0),
            "Tc" => self.tc = Self::num(args, 0).unwrap_or(0.0),
            "Tz" => self.tz = Self::num(args, 0).unwrap_or(100.0),
            "Tf" => {
                if let (Some(Name(n)), Some(size)) = (args.first(), Self::num(args, 1)) {
                    self.tfs = size;
                    self.resolve_font(n, resources);
                }
            }
            "Tj" => {
                if self.in_text {
                    if let Some(Str(s)) = args.last() {
                        self.text_show(s);
                    }
                }
            }
            "TJ" => {
                if self.in_text {
                    if let Some(Arr(items)) = args.last() {
                        let mut adv = 0.0f32;
                        for it in items {
                            match it {
                                Num(n) => adv += n / 1000.0 * self.tfs,
                                Str(s) => adv += self.string_advance(s),
                                _ => {}
                            }
                        }
                        self.text_emit(adv);
                    }
                }
            }
            "'" => {
                if self.in_text {
                    if let Some(Str(s)) = args.last() {
                        self.tlm = Mat::mul(
                            Mat::of(1.0, 0.0, 0.0, 1.0, 0.0, -self.tl),
                            self.tlm,
                        );
                        self.text_show(s);
                    }
                }
            }
            "\"" => {
                if self.in_text {
                    if let Some(Str(s)) = args.last() {
                        self.tw = Self::num(args, 0).unwrap_or(0.0);
                        self.tc = Self::num(args, 1).unwrap_or(0.0);
                        self.tlm = Mat::mul(
                            Mat::of(1.0, 0.0, 0.0, 1.0, 0.0, -self.tl),
                            self.tlm,
                        );
                        self.text_show(s);
                    }
                }
            }
            "Do" => {
                if let Some(Name(n)) = args.last() {
                    self.draw_xobject(n, resources, depth);
                }
            }
            _ => {}
        }
    }

    fn resolve_font(&mut self, name: &[u8], resources: Option<&'a Dictionary>) {
        let res = match resources {
            Some(r) => r,
            None => return,
        };
        let fonts = match res.get(b"Font") {
            Ok(Object::Dictionary(d)) => d,
            Ok(Object::Reference(id)) => {
                match self.doc.get_object(*id).and_then(|o| o.as_dict()) {
                    Ok(d) => d,
                    Err(_) => return,
                }
            }
            _ => return,
        };
        let entry = match fonts.get(name) {
            Ok(o) => o,
            Err(_) => return,
        };
        let id = match entry {
            Object::Reference(id) => *id,
            _ => return,
        };
        if self.fonts.contains_key(&id) {
            self.font = Some(id);
            return;
        }
        let obj = match self.doc.get_object(id) {
            Ok(o) => o,
            Err(_) => return,
        };
        let d = match obj.as_dict() {
            Ok(d) => d,
            Err(_) => return,
        };
        let info = build_font_info(self.doc, d);
        self.fonts.insert(id, info);
        self.font = Some(id);
    }

    fn string_advance(&self, s: &[u8]) -> f32 {
        let font = self.font.and_then(|id| self.fonts.get(&id));
        let codes: Vec<u32> = match font {
            Some(FontInfo::Cid { .. }) => s
                .chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| ((c[0] as u32) << 8) | c[1] as u32)
                .collect(),
            _ => s.iter().map(|&b| b as u32).collect(),
        };
        let mut adv = 0.0f32;
        for code in codes {
            let w = font.map(|f| glyph_width(f, code)).unwrap_or(1000.0);
            adv += w * self.tfs / 1000.0 + self.tc;
            if code == 32 {
                adv += self.tz / 100.0 * self.tw;
            }
        }
        adv
    }

    fn text_show(&mut self, s: &[u8]) {
        self.text_emit(self.string_advance(s));
    }

    fn text_emit(&mut self, adv: f32) {
        let ptm = Mat::mul(self.tlm, self.ctm);
        let x0 = ptm.e;
        let x1 = x0 + adv * ptm.a;
        self.intervals.push((x0.min(x1), x0.max(x1)));
    }

    fn matrix_of(d: &Dictionary, key: &[u8]) -> Option<Mat> {
        d.get(key)
            .ok()
            .and_then(|o| o.as_array().ok())
            .and_then(|a| nums_at(a, 6))
            .and_then(|v| {
                let v: [f32; 6] = v.try_into().ok()?;
                Some(Mat::of(v[0], v[1], v[2], v[3], v[4], v[5]))
            })
    }

    fn draw_xobject(
        &mut self,
        name: &[u8],
        resources: Option<&'a Dictionary>,
        depth: usize,
    ) {
        let res = match resources {
            Some(r) => r,
            None => return,
        };
        let xo = match res.get(b"XObject") {
            Ok(Object::Dictionary(d)) => d,
            Ok(Object::Reference(id)) => {
                match self.doc.get_object(*id).and_then(|o| o.as_dict()) {
                    Ok(d) => d,
                    Err(_) => return,
                }
            }
            _ => return,
        };
        let entry = match xo.get(name) {
            Ok(o) => o,
            Err(_) => return,
        };
        let id = match entry {
            Object::Reference(id) => *id,
            _ => return,
        };
        let obj = match self.doc.get_object(id) {
            Ok(o) => o,
            Err(_) => return,
        };
        let d = match obj_dict(obj) {
            Some(d) => d,
            None => return,
        };
        let subtype = d.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
        match subtype {
            Some(s) if s == b"Form" => {
                if depth >= 8 || !self.seen_forms.insert(id) {
                    return;
                }
                let matrix = Self::matrix_of(d, b"Matrix").unwrap_or(Mat::I);
                let new_ctm = Mat::mul(matrix, self.ctm);
                let bbox: Option<[f32; 4]> = d
                    .get(b"BBox")
                    .ok()
                    .and_then(|o| o.as_array().ok())
                    .and_then(|a| nums_at(a, 4))
                    .and_then(|v| v.try_into().ok());
                let form_res = d.get(b"Resources").ok().and_then(|o| match o {
                    Object::Dictionary(dd) => Some(dd),
                    Object::Reference(rid) => self
                        .doc
                        .get_object(*rid)
                        .ok()
                        .and_then(|o2| o2.as_dict().ok()),
                    _ => None,
                });
                let data = match obj
                    .as_stream()
                    .ok()
                    .and_then(|st| st.decompressed_content().ok())
                {
                    Some(data) => data,
                    None => return,
                };
                // Form 拥有独立的图形状态，进入前保存、返回后恢复
                let saved = (
                    self.ctm,
                    self.ctm_stack.len(),
                    self.ts_stack.len(),
                    (self.tfs, self.tw, self.tc, self.tz, self.tl, self.font),
                );
                let mark = self.intervals.len();
                self.ctm = new_ctm;
                self.walk(&data, form_res, depth + 1);
                self.ctm = saved.0;
                self.ctm_stack.truncate(saved.1);
                self.ts_stack.truncate(saved.2);
                (self.tfs, self.tw, self.tc, self.tz, self.tl, self.font) = saved.3;
                self.in_text = false;
                self.tlm = Mat::I;
                // 将本 Form 产生的区间裁剪到 BBox 范围（渲染时同样按 BBox 裁剪）
                if let Some(bb) = bbox {
                    let xs = [
                        new_ctm.x_of(bb[0], bb[1]),
                        new_ctm.x_of(bb[2], bb[1]),
                        new_ctm.x_of(bb[0], bb[3]),
                        new_ctm.x_of(bb[2], bb[3]),
                    ];
                    let bx0 = xs.iter().cloned().fold(f32::INFINITY, f32::min);
                    let bx1 = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                    let mut kept: Vec<(f32, f32)> = Vec::new();
                    for (a, b) in self.intervals.drain(mark..) {
                        let (a2, b2) = (a.max(bx0), b.min(bx1));
                        if a2 < b2 {
                            kept.push((a2, b2));
                        }
                    }
                    self.intervals.extend(kept);
                }
            }
            Some(s) if s == b"Image" => {
                let img_matrix = Self::matrix_of(d, b"Matrix").unwrap_or(Mat::I);
                let c = Mat::mul(img_matrix, self.ctm);
                let xs = [
                    c.x_of(0.0, 0.0),
                    c.x_of(1.0, 0.0),
                    c.x_of(0.0, 1.0),
                    c.x_of(1.0, 1.0),
                ];
                let min = xs.iter().cloned().fold(f32::INFINITY, f32::min);
                let max = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
                self.intervals.push((min, max));
            }
            _ => {}
        }
    }
}

/// 在整页墨迹区间中查找中间空白带 [左栏右缘, 右栏左缘]
fn detect_gap(intervals: &[(f32, f32)], x1: f32, x2: f32) -> Option<(f32, f32)> {
    let mid = (x1 + x2) / 2.0;
    let mut left_max: Option<f32> = None;
    let mut right_min: Option<f32> = None;
    for &(a, b) in intervals {
        if a < mid && b > mid {
            // 有内容横跨页面中部，不存在清晰的双栏空白
            return None;
        }
        if b <= mid {
            left_max = Some(left_max.map_or(b, |m| m.max(b)));
        }
        if a >= mid {
            right_min = Some(right_min.map_or(a, |m| m.min(a)));
        }
    }
    let (l, r) = (left_max?, right_min?);
    // 两侧各留 2pt 安全余量，防止宽度计算误差切到内容边缘
    let (l, r) = (l + 2.0, r - 2.0);
    if r - l < 10.0 {
        return None;
    }
    Some((l, r))
}
