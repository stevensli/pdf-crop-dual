//! PDF 内容分析库：对象访问、几何、字体宽度、内容流词法器与遍历、空白检测、内容重写。

use lopdf::content::{Content, Operation};
use lopdf::{Document, Dictionary, Object, ObjectId, StringFormat};
use std::collections::{HashMap, HashSet};

/// 获取条目：优先页面字典，否则从父 Pages 节点继承；父节点非字典时报 ObjectNotFound
fn page_inherit<'a>(
    doc: &'a Document,
    page_dict: &'a Dictionary,
    page_id: ObjectId,
    key: &[u8],
) -> Result<&'a Object, lopdf::Error> {
    page_dict.get(key).or_else(|_| {
        if let Ok(Object::Reference(parent_id)) = page_dict.get(b"Parent") {
            if let Ok(Object::Dictionary(parent_dict)) = doc.get_object(*parent_id) {
                return parent_dict.get(key);
            }
        }
        Err(lopdf::Error::ObjectNotFound(page_id))
    })
}

/// 获取页面的 MediaBox
pub fn get_mediabox(
    doc: &Document,
    page_dict: &Dictionary,
    page_id: ObjectId,
) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    let obj = page_inherit(doc, page_dict, page_id, b"MediaBox")?;
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

/// 获取页面的 Resources
pub fn get_resources(
    doc: &Document,
    page_dict: &Dictionary,
    page_id: ObjectId,
) -> Result<Object, Box<dyn std::error::Error>> {
    let obj = page_inherit(doc, page_dict, page_id, b"Resources")?;
    Ok(doc.dereference(obj)?.1.clone())
}

/// 借用方式获取页面 Resources 字典（用于内容扫描）
pub fn page_resources_dict<'a>(
    doc: &'a Document,
    page_dict: &'a Dictionary,
    page_id: ObjectId,
) -> Option<&'a Dictionary> {
    page_inherit(doc, page_dict, page_id, b"Resources")
        .ok()
        .and_then(|o| match o {
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

/// 取子字典：Ok(Dictionary) 或 Ok(Reference)→deref，其余 None
fn sub_dict<'a>(doc: &'a Document, d: &'a Dictionary, key: &[u8]) -> Option<&'a Dictionary> {
    match d.get(key) {
        Ok(Object::Dictionary(dd)) => Some(dd),
        Ok(Object::Reference(id)) => doc.get_object(*id).ok().and_then(|o| o.as_dict().ok()),
        _ => None,
    }
}

/// 查找 XObject：res→/XObject→name→对象；条目仅接受 Reference
fn find_xobject<'a>(
    doc: &'a Document,
    res: &'a Dictionary,
    name: &[u8],
) -> Option<(ObjectId, &'a Object)> {
    let xo = sub_dict(doc, res, b"XObject")?;
    let entry = xo.get(name).ok()?;
    let id = match entry {
        Object::Reference(id) => *id,
        _ => return None,
    };
    Some((id, doc.get_object(id).ok()?))
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

    /// 平移矩阵
    fn translate(x: f32, y: f32) -> Mat {
        Mat::of(1.0, 0.0, 0.0, 1.0, x, y)
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

/// 点集经矩阵投影后的 x 范围 (min, max)；调用方须保证点集非空
fn x_extents(m: Mat, pts: &[(f32, f32)]) -> (f32, f32) {
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for &(x, y) in pts {
        let px = m.x_of(x, y);
        min = min.min(px);
        max = max.max(px);
    }
    (min, max)
}

/// 将 mark 之后产生的区间按 [bx0, bx1] 钳位；仅保留非空结果（a2 < b2）
fn clip_intervals_to_bbox(
    intervals: &mut Vec<(f32, f32)>,
    mark: usize,
    bx0: f32,
    bx1: f32,
) {
    let mut kept: Vec<(f32, f32)> = Vec::new();
    for (a, b) in intervals.drain(mark..) {
        let (a2, b2) = (a.max(bx0), b.min(bx1));
        if a2 < b2 {
            kept.push((a2, b2));
        }
    }
    intervals.extend(kept);
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

/// DescendantFonts 数组 → 首个 CIDFont 字典（两跳 deref）
fn descendant_cid_font<'a>(
    doc: &'a Document,
    d: &'a Dictionary,
) -> Option<&'a Dictionary> {
    let arr: &Vec<Object> = match d.get(b"DescendantFonts") {
        Ok(Object::Array(a)) => a,
        Ok(Object::Reference(id)) => doc.get_object(*id).ok()?.as_array().ok()?,
        _ => return None,
    };
    match arr.first()? {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok(),
        Object::Dictionary(dd) => Some(dd),
        _ => None,
    }
}

/// Type0/Type0C 复合字体宽度信息；缺 DescendantFonts 时返回空 Cid
fn build_cid_font_info<'a>(doc: &'a Document, d: &'a Dictionary) -> FontInfo {
    let mut widths: HashMap<u16, f32> = HashMap::new();
    let mut dw = 1000.0f32;
    if let Some(cd) = descendant_cid_font(doc, d) {
        if let Some(v) = cd.get(b"DW").ok().and_then(onum) {
            dw = v;
        }
        if let Ok(w) = cd.get(b"W").and_then(|o| o.as_array()) {
            for sub in w {
                if let Ok(arr2) = sub.as_array() {
                    parse_w_entry(arr2, &mut widths);
                }
            }
        }
    }
    FontInfo::Cid { widths, dw }
}

fn build_font_info<'a>(doc: &'a Document, d: &'a Dictionary) -> FontInfo {
    let subtype = d.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
    match subtype {
        Some(s) if s == b"Type0" || s == b"Type0C" => build_cid_font_info(doc, d),
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

/// 在 Resources 的 /Font 子字典中按名查找字体对象 id（条目须为间接引用）
fn font_object_id(doc: &Document, res: Option<&Dictionary>, name: &[u8]) -> Option<ObjectId> {
    let res = res?;
    let fonts = sub_dict(doc, res, b"Font")?;
    match fonts.get(name).ok()? {
        Object::Reference(id) => Some(*id),
        _ => None,
    }
}

/// 解析字体名：查 /Font 子字典，FontInfo 缺失时构建并入缓存；成功返回字体 id
fn resolve_font_id(
    doc: &Document,
    res: Option<&Dictionary>,
    name: &[u8],
    fonts: &mut HashMap<ObjectId, FontInfo>,
) -> Option<ObjectId> {
    let id = font_object_id(doc, res, name)?;
    if !fonts.contains_key(&id) {
        let d = doc.get_object(id).ok().and_then(obj_dict)?;
        fonts.insert(id, build_font_info(doc, d));
    }
    Some(id)
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
                    // None = 行续（\r / \n，消耗但不产生字节）
                    if let Some(c) = self.parse_escape()? {
                        out.push(c);
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

    /// 解析单个转义字符（pos 已在 '\\' 之后）：
    /// 外层 None = 数据结束；内层 None = \r / \n 行续（消耗但不产生字节）
    fn parse_escape(&mut self) -> Option<Option<u8>> {
        let c = self.peek()?;
        Some(match c {
            b'n' => {
                self.pos += 1;
                Some(b'\n')
            }
            b'r' => {
                self.pos += 1;
                Some(b'\r')
            }
            b't' => {
                self.pos += 1;
                Some(b'\t')
            }
            b'b' => {
                self.pos += 1;
                Some(0x08)
            }
            b'f' => {
                self.pos += 1;
                Some(0x0C)
            }
            c @ (b'(' | b')' | b'\\') => {
                self.pos += 1;
                Some(c)
            }
            b'\r' => {
                self.pos += 1;
                if self.peek() == Some(b'\n') {
                    self.pos += 1;
                }
                None
            }
            b'\n' => {
                self.pos += 1;
                None
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
                Some(v as u8)
            }
            other => {
                self.pos += 1;
                Some(other)
            }
        })
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
            b'[' => match self.parse_array() {
                Some(arr) => Parsed::Val(Val::Arr(arr)),
                None => Parsed::Err,
            },
            b'+' | b'-' | b'.' | b'0'..=b'9' => match self.parse_num() {
                Some(n) => Parsed::Val(Val::Num(n)),
                None => Parsed::Err,
            },
            _ => Parsed::Word(self.read_word()),
        }
    }

    /// 解析数组（含 '[' 消耗）；Word/DictSkipped 元素丢弃
    fn parse_array(&mut self) -> Option<Vec<Val>> {
        self.pos += 1; // '['
        let mut arr = Vec::new();
        loop {
            self.skip_ws_comments();
            match self.peek() {
                None => return None,
                Some(b']') => {
                    self.pos += 1;
                    return Some(arr);
                }
                _ => {}
            }
            match self.parse_val() {
                Parsed::Val(v) => arr.push(v),
                Parsed::Word(_) | Parsed::DictSkipped => {}
                Parsed::Err => return None,
            }
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
            // 读取键（名称或裸词）
            if self.peek() == Some(b'/') {
                self.pos += 1;
                self.parse_name();
            } else {
                self.read_word();
            }
            if !self.skip_value() {
                return false;
            }
        }
    }

    /// 跳过字典中的一个值（含数组配平、字符串、嵌套字典、"n n R" 对象引用）；失败返回 false
    fn skip_value(&mut self) -> bool {
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
        true
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
        let length = match self.inline_image_length() {
            Some(l) => l,
            None => return false,
        };
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
        if self.pos + length > self.data.len() {
            return false;
        }
        self.pos += length;
        self.skip_ws_comments();
        if self.peek() != Some(b'E') {
            return false;
        }
        self.read_word() == b"EI"
    }

    /// 读内联图像 BI 字典（调用方已消耗 "<<"）并返回 /Length；缺 Length 或出错返回 None
    fn inline_image_length(&mut self) -> Option<usize> {
        let mut length: Option<usize> = None;
        loop {
            self.skip_ws_comments();
            match self.peek() {
                None => return None,
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
        length
    }
}

// ===================== 文本 advance（Walk 与 Rewriter 共用，保证口径单点实现） =====================

/// 字符串 advance：逐码字宽 + 字距 Tc；空格额外加 Tz/100·Tw。
/// CID 字体按 2 字节解码，其余按 1 字节；无字体信息按 1em（过估是安全方向）
fn str_advance(font: Option<&FontInfo>, s: &[u8], tfs: f32, tw: f32, tc: f32, tz: f32) -> f32 {
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
        adv += w * tfs / 1000.0 + tc;
        if code == 32 {
            adv += tz / 100.0 * tw;
        }
    }
    adv
}

/// TJ 数组的总 advance：Num 为缩进（千分之一 em），Str 按字宽累计
fn tj_advance(
    font: Option<&FontInfo>,
    items: &[Val],
    tfs: f32,
    tw: f32,
    tc: f32,
    tz: f32,
) -> f32 {
    let mut adv = 0.0f32;
    for it in items {
        match it {
            Val::Num(n) => adv += n / 1000.0 * tfs,
            Val::Str(s) => adv += str_advance(font, s, tfs, tw, tc, tz),
            _ => {}
        }
    }
    adv
}

// ===================== 内容流遍历（收集墨迹 x 区间） =====================

pub struct Walk<'a> {
    doc: &'a Document,
    pub intervals: Vec<(f32, f32)>,
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
    pub fn new(doc: &'a Document) -> Self {
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

    pub fn walk(&mut self, data: &[u8], resources: Option<&'a Dictionary>, depth: usize) {
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
        match op {
            "q" | "Q" | "cm" => self.exec_state(op, args),
            "re" | "m" | "l" | "c" | "v" | "y" | "S" | "s" | "f" | "F" | "f*" | "B"
            | "B*" | "b" | "b*" | "W" | "W*" | "n" => self.exec_path(op, args),
            "BT" | "ET" | "Tm" | "Td" | "TD" | "T*" | "TL" | "Tw" | "Tc" | "Tz" | "Tf"
            | "Tj" | "TJ" | "'" | "\"" => self.exec_text(op, args, resources),
            "Do" => {
                if let Some(Val::Name(n)) = args.last() {
                    self.draw_xobject(n, resources, depth);
                }
            }
            _ => {}
        }
    }

    /// 图形状态：q / Q / cm
    fn exec_state(&mut self, op: &str, args: &[Val]) {
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
            _ => {}
        }
    }

    /// 路径：re/m/l/c/v/y 构造，绘制操作汇总区间，W|W*|n 丢弃
    fn exec_path(&mut self, op: &str, args: &[Val]) {
        match op {
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
                    let (min, max) = x_extents(self.ctm, &self.path);
                    self.intervals.push((min, max));
                }
                self.path.clear();
            }
            "W" | "W*" | "n" => self.path.clear(),
            _ => {}
        }
    }

    /// 文本：BT/ET 与状态操作（TL/Tw/Tc/Tz/Tf）；定位与显示分派到专用方法
    fn exec_text(
        &mut self,
        op: &str,
        args: &[Val],
        resources: Option<&'a Dictionary>,
    ) {
        match op {
            "BT" => {
                self.in_text = true;
                self.tlm = Mat::I;
            }
            "ET" => self.in_text = false,
            "Tm" | "Td" | "TD" | "T*" => self.exec_text_move(op, args),
            "Tj" | "TJ" | "'" | "\"" => self.exec_text_show(op, args),
            "TL" => self.tl = Self::num(args, 0).unwrap_or(0.0),
            "Tw" => self.tw = Self::num(args, 0).unwrap_or(0.0),
            "Tc" => self.tc = Self::num(args, 0).unwrap_or(0.0),
            "Tz" => self.tz = Self::num(args, 0).unwrap_or(100.0),
            "Tf" => {
                if let (Some(Val::Name(n)), Some(size)) = (args.first(), Self::num(args, 1)) {
                    self.tfs = size;
                    self.resolve_font(n, resources);
                }
            }
            _ => {}
        }
    }

    /// 文本定位：Tm 绝对设置；Td/TD/T* 相对移动（偏移在文本空间，
    /// 经行矩阵线性部分缩放，与重写器同口径）
    fn exec_text_move(&mut self, op: &str, args: &[Val]) {
        if !self.in_text {
            return;
        }
        match op {
            "Tm" => {
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
            "Td" | "TD" => {
                if let (Some(tx), Some(ty)) = (Self::num(args, 0), Self::num(args, 1)) {
                    if op == "TD" {
                        self.tl = -ty;
                    }
                    self.tlm = Mat::mul(Mat::translate(tx, ty), self.tlm);
                }
            }
            "T*" => self.tlm = Mat::mul(Mat::translate(0.0, -self.tl), self.tlm),
            _ => {}
        }
    }

    /// 文本显示：Tj/TJ 直接显示；'/" 先移到下一行行首
    fn exec_text_show(&mut self, op: &str, args: &[Val]) {
        if !self.in_text {
            return;
        }
        match op {
            "Tj" => {
                if let Some(Val::Str(s)) = args.last() {
                    self.text_show(s);
                }
            }
            "TJ" => {
                if let Some(Val::Arr(items)) = args.last() {
                    self.text_emit(self.tj_advance(items));
                }
            }
            "'" => {
                if let Some(Val::Str(s)) = args.last() {
                    self.text_next_line_show(s);
                }
            }
            "\"" => {
                if let Some(Val::Str(s)) = args.last() {
                    self.tw = Self::num(args, 0).unwrap_or(0.0);
                    self.tc = Self::num(args, 1).unwrap_or(0.0);
                    self.text_next_line_show(s);
                }
            }
            _ => {}
        }
    }

    /// 当前字体信息
    fn font_info(&self) -> Option<&FontInfo> {
        self.font.and_then(|id| self.fonts.get(&id))
    }

    /// TJ 数组的总 advance（口径见 tj_advance 自由函数）
    fn tj_advance(&self, items: &[Val]) -> f32 {
        tj_advance(self.font_info(), items, self.tfs, self.tw, self.tc, self.tz)
    }

    /// 移到下一行行首并显示字符串（' 与 " 共享）
    fn text_next_line_show(&mut self, s: &[u8]) {
        self.tlm = Mat::mul(Mat::translate(0.0, -self.tl), self.tlm);
        self.text_show(s);
    }

    fn resolve_font(&mut self, name: &[u8], resources: Option<&'a Dictionary>) {
        if let Some(id) = resolve_font_id(self.doc, resources, name, &mut self.fonts) {
            self.font = Some(id);
        }
    }

    /// 字符串 advance（口径见 str_advance 自由函数）
    fn string_advance(&self, s: &[u8]) -> f32 {
        str_advance(self.font_info(), s, self.tfs, self.tw, self.tc, self.tz)
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
        let (id, obj) = match find_xobject(self.doc, res, name) {
            Some(x) => x,
            None => return,
        };
        let d = match obj_dict(obj) {
            Some(d) => d,
            None => return,
        };
        let subtype = d.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
        match subtype {
            Some(s) if s == b"Form" => self.walk_form(id, obj, d, depth),
            Some(s) if s == b"Image" => self.emit_image_extent(d),
            _ => {}
        }
    }

    /// Form XObject：深度/环检查 → 保存图形状态 → 递归 walk → 恢复 → 区间按 BBox 裁剪
    fn walk_form(
        &mut self,
        id: ObjectId,
        obj: &'a Object,
        d: &'a Dictionary,
        depth: usize,
    ) {
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
        let form_res = sub_dict(self.doc, d, b"Resources");
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
            let corners = [
                (bb[0], bb[1]),
                (bb[2], bb[1]),
                (bb[0], bb[3]),
                (bb[2], bb[3]),
            ];
            let (bx0, bx1) = x_extents(new_ctm, &corners);
            clip_intervals_to_bbox(&mut self.intervals, mark, bx0, bx1);
        }
    }

    /// Image XObject：单位正方形四角经 Matrix×CTM 后推入 x 区间
    fn emit_image_extent(&mut self, d: &Dictionary) {
        let img_matrix = Self::matrix_of(d, b"Matrix").unwrap_or(Mat::I);
        let c = Mat::mul(img_matrix, self.ctm);
        let (min, max) = x_extents(c, &[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)]);
        self.intervals.push((min, max));
    }
}

/// 在整页墨迹区间中查找中间空白带 [左栏右缘, 右栏左缘]
pub fn detect_gap(intervals: &[(f32, f32)], x1: f32, x2: f32) -> Option<(f32, f32)> {
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

// ===================== 页面级内容重写（格式保留式裁剪） =====================
//
// 将原页面内容流重写为「左侧原样、右侧物理左移 cut」的新内容流：内容只存在
// 一份、原有 Form XObject 结构不变，避免「整页封装 Form + 裁剪绘制两次」造成
// 的文本层重复（编辑工具中段落被拆块）。
//
// 分类规则（与 clip 方案渲染严格对齐）：
//   墨迹右缘 < band_left        → 保留原位置
//   墨迹左缘 > band_left + cut  → 整体左移 cut（右侧可见区在原始坐标中从
//                                 band_left + cut 开始，与 clip 裁剪区一致）
//   跨越移除带                  → 返回 None（调用方回退 clip 方案）

/// 重写页面级内容流；遇到超出支持范围的语法返回 None
pub fn rewrite_page<'a>(
    doc: &'a Document,
    content: &[u8],
    res: Option<&'a Dictionary>,
    band_left: f32,
    cut: f32,
) -> Option<Content> {
    Rewriter::new(doc, res, band_left, cut).run(content)
}

struct Rewriter<'a> {
    doc: &'a Document,
    res: Option<&'a Dictionary>,
    band_left: f32,
    cut: f32,
    out: Vec<Operation>,
    // 图形状态（q/Q 保存恢复，含字体——Walk 未存字体，此处需存以保证 Q 后字宽正确）
    ctm: Mat,
    qstack: Vec<(Mat, f32, f32, f32, f32, f32, Option<ObjectId>)>,
    tfs: f32,
    tw: f32,
    tc: f32,
    tz: f32,
    tl: f32,
    font: Option<ObjectId>,
    fonts: HashMap<ObjectId, FontInfo>,
    // 文本状态：tlm = 行矩阵（行首），tm = 文本矩阵（当前字形位置，显示后前移）
    in_text: bool,
    tlm: Mat,
    tm: Mat,
    // 当前绝对位置已施加的移位（0 或 -cut），BT 时复位
    last_side: Option<f32>,
    block: Vec<Operation>,
    // 路径累积：m/re 开始新子路径；首个 m/re 之前的构造操作无法归类，进 preamble
    preamble: Vec<Operation>,
    subpaths: Vec<Subpath>,
    cur: Option<Subpath>,
}

struct Subpath {
    pts: Vec<(f32, f32)>,
    ops: Vec<Operation>,
}

impl<'a> Rewriter<'a> {
    fn new(doc: &'a Document, res: Option<&'a Dictionary>, band_left: f32, cut: f32) -> Self {
        Rewriter {
            doc,
            res,
            band_left,
            cut,
            out: Vec::new(),
            ctm: Mat::I,
            qstack: Vec::new(),
            tfs: 0.0,
            tw: 0.0,
            tc: 0.0,
            tz: 100.0,
            tl: 0.0,
            font: None,
            fonts: HashMap::new(),
            in_text: false,
            tlm: Mat::I,
            tm: Mat::I,
            last_side: None,
            block: Vec::new(),
            preamble: Vec::new(),
            subpaths: Vec::new(),
            cur: None,
        }
    }

    fn run(&mut self, data: &[u8]) -> Option<Content> {
        let mut tk = Tok { data, pos: 0 };
        let mut operands: Vec<Val> = Vec::new();
        loop {
            match tk.next_item() {
                Some(Item::Val(v)) => operands.push(v),
                Some(Item::Op(op)) => {
                    if op == "BI" {
                        return None; // 内联图像不支持
                    }
                    if !self.exec(&op, &operands) {
                        return None;
                    }
                    operands.clear();
                }
                None => break,
            }
        }
        if self.in_text {
            return None; // BT 未闭合
        }
        // 无绘制操作的残留路径构造不产生墨迹，原样通过
        for o in self.preamble.drain(..) {
            self.out.push(o);
        }
        for c in self.subpaths.drain(..) {
            self.out.extend(c.ops);
        }
        if let Some(c) = self.cur.take() {
            self.out.extend(c.ops);
        }
        Some(Content {
            operations: std::mem::take(&mut self.out),
        })
    }

    fn exec(&mut self, op: &str, args: &[Val]) -> bool {
        if self.in_text {
            // 文本块内：文本操作走定位修正；颜色/线条外观/平坦度/渲染模式/
            // 标记内容等操作不影响文本 x 定位，原样入缓冲；其余（含 cm/q/Q）→ 回退
            return match op {
                "ET" | "Tm" | "Td" | "TD" | "T*" | "TL" | "Tw" | "Tc" | "Tz" | "Tf" | "Tj"
                | "TJ" | "'" | "\"" => self.exec_text(op, args),
                "g" | "G" | "rg" | "RG" | "k" | "K" | "sc" | "SC" | "scn" | "SCN" | "cs"
                | "CS" | "gs" | "w" | "J" | "j" | "M" | "d" | "ri" | "i" | "Tr" | "Ts" | "MP"
                | "BMC" | "EMC" => {
                    self.block.push(self.op_from(op, args));
                    true
                }
                _ => false,
            };
        }
        match op {
            "q" | "Q" | "cm" => self.exec_state(op, args),
            "re" | "m" | "l" | "c" | "v" | "y" | "h" | "S" | "s" | "f" | "F" | "f*" | "B"
            | "B*" | "b" | "b*" | "W" | "W*" | "n" => self.exec_path(op, args),
            "BT" => {
                self.in_text = true;
                self.tlm = Mat::I;
                self.tm = Mat::I;
                self.last_side = None;
                self.block.clear();
                self.emit("BT", args);
                true
            }
            "TL" | "Tw" | "Tc" | "Tz" | "Tf" => self.exec_text_state(op, args),
            "Tm" | "Td" | "TD" | "T*" | "Tj" | "TJ" | "'" | "\"" => {
                // 文本对象外的文本定位/显示操作不生效，原样通过
                self.emit(op, args);
                true
            }
            "Do" => self.exec_do(args),
            "w" | "J" | "j" | "M" | "d" | "ri" | "i" | "gs" | "CS" | "cs" | "SC" | "sc" | "SCN"
            | "scn" | "MP" | "BMC" | "EMC" => {
                // 状态类与标记内容操作不产生墨迹，原样通过
                self.emit(op, args);
                true
            }
            _ => false, // 未知操作无法确定墨迹范围 → 回退
        }
    }

    /// 设备 x 坐标所属侧：左侧 0.0 / 右侧 -cut；落在移除带内返回 None
    fn side_of_x(&self, x: f32) -> Option<f32> {
        if x < self.band_left {
            Some(0.0)
        } else if x > self.band_left + self.cut {
            Some(-self.cut)
        } else {
            None
        }
    }

    /// 设备空间左移 cut 对应的操作空间平移（退化矩阵返回 None）
    fn shift_vec(&self) -> Option<(f32, f32)> {
        let det = self.ctm.a * self.ctm.d - self.ctm.b * self.ctm.c;
        if det.abs() < 1e-9 {
            return None;
        }
        Some((-self.cut * self.ctm.d / det, self.cut * self.ctm.b / det))
    }

    /// 图形状态：q / Q / cm（文本块内调用方已拦截）
    fn exec_state(&mut self, op: &str, args: &[Val]) -> bool {
        match op {
            "q" => {
                self.qstack.push((
                    self.ctm, self.tfs, self.tw, self.tc, self.tz, self.tl, self.font,
                ));
                self.emit(op, args);
                true
            }
            "Q" => {
                if let Some(s) = self.qstack.pop() {
                    (self.ctm, self.tfs, self.tw, self.tc, self.tz, self.tl, self.font) = s;
                }
                self.emit(op, args);
                true
            }
            "cm" => {
                if let (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) = (
                    Walk::num(args, 0),
                    Walk::num(args, 1),
                    Walk::num(args, 2),
                    Walk::num(args, 3),
                    Walk::num(args, 4),
                    Walk::num(args, 5),
                ) {
                    self.ctm = Mat::mul(Mat::of(a, b, c, d, e, f), self.ctm);
                }
                self.emit(op, args);
                true
            }
            _ => true,
        }
    }

    /// 文本块内操作：定位（Tm/Td/TD）、状态（TL/Tw/Tc/Tz/Tf）分派到专用方法
    fn exec_text(&mut self, op: &str, args: &[Val]) -> bool {
        match op {
            "ET" => {
                self.out.extend(self.block.drain(..));
                self.out.push(Operation::new("ET", vec![]));
                self.in_text = false;
                true
            }
            "Tm" => self.exec_tm(args),
            "Td" | "TD" => self.exec_td(op, args),
            "T*" | "Tj" | "TJ" | "'" | "\"" => self.exec_text_show(op, args),
            "TL" | "Tw" | "Tc" | "Tz" | "Tf" => self.exec_text_state(op, args),
            _ => false,
        }
    }

    /// 文本显示：Tj/TJ 校验范围并前移；T*/'/" 因缺少可携带修正的操作数，
    /// 分解为 Td + Tj（" 先补发 Tw/Tc）
    fn exec_text_show(&mut self, op: &str, args: &[Val]) -> bool {
        match op {
            "T*" => {
                // T* = Td(0, -TL)
                let w = match self.rel_move(0.0, -self.tl) {
                    Some(w) => w,
                    None => return false,
                };
                self.block
                    .push(Operation::new("Td", vec![w.0.into(), (-self.tl + w.1).into()]));
                true
            }
            "Tj" | "TJ" => {
                if !self.ensure_block_position() {
                    return false;
                }
                let adv = match (op, args.last()) {
                    ("Tj", Some(Val::Str(s))) => Some(self.string_advance(s)),
                    ("TJ", Some(Val::Arr(items))) => Some(self.tj_advance(items)),
                    _ => None,
                };
                if let Some(a) = adv {
                    if !self.check_and_advance(a) {
                        return false;
                    }
                }
                self.block.push(self.op_from(op, args));
                true
            }
            "'" => {
                let s = match args.last() {
                    Some(Val::Str(s)) => s,
                    _ => {
                        self.block.push(self.op_from("'", args));
                        return true;
                    }
                };
                self.next_line_show(s)
            }
            "\"" => {
                let (tw, tc, s) = match (Walk::num(args, 0), Walk::num(args, 1), args.get(2)) {
                    (Some(tw), Some(tc), Some(Val::Str(s))) => (tw, tc, s),
                    _ => return false,
                };
                self.tw = tw;
                self.tc = tc;
                self.block.push(Operation::new("Tw", vec![tw.into()]));
                self.block.push(Operation::new("Tc", vec![tc.into()]));
                self.next_line_show(s)
            }
            _ => false,
        }
    }

    /// ' 与 " 的公共尾部：换行到下一行行首（分解为 Td）并显示字符串
    fn next_line_show(&mut self, s: &[u8]) -> bool {
        let w = match self.rel_move(0.0, -self.tl) {
            Some(w) => w,
            None => return false,
        };
        self.block
            .push(Operation::new("Td", vec![w.0.into(), (-self.tl + w.1).into()]));
        let adv = self.string_advance(s);
        if !self.check_and_advance(adv) {
            return false;
        }
        self.block.push(Operation::new("Tj", vec![str_obj(s)]));
        true
    }

    /// Tm：设置绝对位置。文本原点 (e,f) 经 CTM 映射到设备 (e,f)·C+t，不受 Tm
    /// 自身线性部分影响，故移位修正只取 CTM 逆：δ(e,f) = (s,0)·C⁻¹；
    /// 发射位置的移位必须恰为 s_t（与前一位置无关）
    fn exec_tm(&mut self, args: &[Val]) -> bool {
        let (a, b, c, d, e, f) = match (
            Walk::num(args, 0),
            Walk::num(args, 1),
            Walk::num(args, 2),
            Walk::num(args, 3),
            Walk::num(args, 4),
            Walk::num(args, 5),
        ) {
            (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f)) => (a, b, c, d, e, f),
            _ => {
                // 非法操作数：与 Walk 一致不更新状态，原样通过
                self.block.push(self.op_from("Tm", args));
                return true;
            }
        };
        let new_tlm = Mat::of(a, b, c, d, e, f);
        let ptm = Mat::mul(new_tlm, self.ctm);
        let s_t = match self.side_of_x(ptm.e) {
            Some(s) => s,
            None => return false,
        };
        let (de, df) = if s_t == 0.0 {
            (0.0, 0.0)
        } else {
            let det = self.ctm.a * self.ctm.d - self.ctm.b * self.ctm.c;
            if det.abs() < 1e-9 {
                return false;
            }
            (s_t * self.ctm.d / det, -s_t * self.ctm.b / det)
        };
        let mut op = self.op_from("Tm", args);
        add_to_real(&mut op, 4, de);
        add_to_real(&mut op, 5, df);
        self.block.push(op);
        self.tlm = new_tlm;
        self.tm = new_tlm;
        self.last_side = Some(s_t);
        true
    }

    /// Td / TD：相对行移动（规范语义：偏移在文本坐标系中，不受 Tm 缩放影响）
    fn exec_td(&mut self, op: &str, args: &[Val]) -> bool {
        let (tx, ty) = match (Walk::num(args, 0), Walk::num(args, 1)) {
            (Some(x), Some(y)) => (x, y),
            _ => {
                self.block.push(self.op_from(op, args));
                return true;
            }
        };
        if op == "TD" {
            self.tl = -ty;
        }
        let w = match self.rel_move(tx, ty) {
            Some(w) => w,
            None => return false,
        };
        let mut opn = self.op_from(op, args);
        add_to_real(&mut opn, 0, w.0);
        add_to_real(&mut opn, 1, w.1);
        self.block.push(opn);
        true
    }

    /// 相对行移动（Td/TD/T* 共享）：(tx,ty) 在文本空间、经行矩阵线性部分缩放
    /// （规范语义，与 Walk 的 mul(translate, tlm) 同口径）；判定新位置所属侧，
    /// 返回文本空间修正偏移（使发射位置较真实位置多移 s_t - s_p）
    fn rel_move(&mut self, tx: f32, ty: f32) -> Option<(f32, f32)> {
        let new_tlm = Mat::of(
            self.tlm.a,
            self.tlm.b,
            self.tlm.c,
            self.tlm.d,
            self.tlm.e + tx * self.tlm.a + ty * self.tlm.c,
            self.tlm.f + tx * self.tlm.b + ty * self.tlm.d,
        );
        let ptm = Mat::mul(new_tlm, self.ctm);
        let s_t = self.side_of_x(ptm.e)?;
        let s_p = self.last_side.unwrap_or(0.0);
        let delta = s_t - s_p;
        let r = if delta == 0.0 {
            (0.0, 0.0)
        } else {
            let det = ptm.a * ptm.d - ptm.b * ptm.c;
            if det.abs() < 1e-9 {
                return None;
            }
            (delta * ptm.d / det, -delta * ptm.b / det)
        };
        self.tlm = new_tlm;
        self.tm = new_tlm;
        self.last_side = Some(s_t);
        Some(r)
    }

    /// 块内尚无 Tm/Td 且当前显示位置由 CTM 决定：若位置在右侧，合成 Tm 携带移位
    fn ensure_block_position(&mut self) -> bool {
        if self.last_side.is_some() {
            return true;
        }
        let ptm = Mat::mul(self.tlm, self.ctm);
        let s_t = match self.side_of_x(ptm.e) {
            Some(s) => s,
            None => return false,
        };
        if s_t < 0.0 {
            let det = ptm.a * ptm.d - ptm.b * ptm.c;
            if det.abs() < 1e-9 {
                return false;
            }
            let (de, df) = (s_t * ptm.d / det, -s_t * ptm.b / det);
            self.block.push(Operation::new(
                "Tm",
                vec![
                    self.tlm.a.into(),
                    self.tlm.b.into(),
                    self.tlm.c.into(),
                    self.tlm.d.into(),
                    (self.tlm.e + de).into(),
                    (self.tlm.f + df).into(),
                ],
            ));
        }
        self.last_side = Some(s_t);
        true
    }

    /// 显示操作的范围校验 + 文本矩阵前移
    fn check_and_advance(&mut self, adv: f32) -> bool {
        let ptm = Mat::mul(self.tm, self.ctm);
        let x0 = ptm.e;
        let x1 = x0 + adv * ptm.a;
        let (lo, hi) = (x0.min(x1), x0.max(x1));
        if !(hi < self.band_left || lo > self.band_left + self.cut) {
            return false;
        }
        if adv != 0.0 {
            // 沿行方向前移（不影响行首 tlm）
            self.tm = Mat::of(
                self.tm.a,
                self.tm.b,
                self.tm.c,
                self.tm.d,
                self.tm.a * adv + self.tm.e,
                self.tm.b * adv + self.tm.f,
            );
        }
        true
    }

    /// TL/Tw/Tc/Tz/Tf：更新状态；块内入缓冲，块外直接通过
    fn exec_text_state(&mut self, op: &str, args: &[Val]) -> bool {
        match op {
            "TL" => self.tl = Walk::num(args, 0).unwrap_or(0.0),
            "Tw" => self.tw = Walk::num(args, 0).unwrap_or(0.0),
            "Tc" => self.tc = Walk::num(args, 0).unwrap_or(0.0),
            "Tz" => self.tz = Walk::num(args, 0).unwrap_or(100.0),
            "Tf" => {
                if let (Some(Val::Name(n)), Some(size)) = (args.first(), Walk::num(args, 1)) {
                    self.tfs = size;
                    self.resolve_font(n);
                }
            }
            _ => return false,
        }
        if self.in_text {
            self.block.push(self.op_from(op, args));
        } else {
            self.emit(op, args);
        }
        true
    }

    /// 路径：re/m/l/c/v/y 累积子路径，绘制/裁剪/结束操作时按子路径分类发射
    fn exec_path(&mut self, op: &str, args: &[Val]) -> bool {
        match op {
            "re" => {
                let opn = self.op_from("re", args);
                if let (Some(x), Some(y), Some(w), Some(h)) = (
                    Walk::num(args, 0),
                    Walk::num(args, 1),
                    Walk::num(args, 2),
                    Walk::num(args, 3),
                ) {
                    self.start_subpath(
                        vec![(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
                        opn,
                    );
                } else {
                    self.preamble.push(opn);
                }
                true
            }
            "m" => {
                let opn = self.op_from("m", args);
                if let (Some(x), Some(y)) = (Walk::num(args, 0), Walk::num(args, 1)) {
                    self.start_subpath(vec![(x, y)], opn);
                } else {
                    self.preamble.push(opn);
                }
                true
            }
            "l" | "c" | "v" | "y" => {
                let opn = self.op_from(op, args);
                let n = if op == "l" { 1 } else { if op == "c" { 3 } else { 2 } };
                let pts: Vec<(f32, f32)> = (0..n)
                    .map(|i| (Walk::num(args, i * 2), Walk::num(args, i * 2 + 1)))
                    .filter_map(|(a, b)| a.zip(b).map(|(x, y)| (x, y)))
                    .collect();
                if let Some(c) = self.cur.as_mut() {
                    c.pts.extend(pts);
                    c.ops.push(opn);
                } else {
                    self.preamble.push(opn);
                }
                true
            }
            "h" => {
                // 闭合子路径：不增加新点（闭合线段 x 范围必在已有点范围内）
                let opn = self.op_from(op, args);
                if let Some(c) = self.cur.as_mut() {
                    c.ops.push(opn);
                } else {
                    self.preamble.push(opn);
                }
                true
            }
            "S" | "s" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "W" | "W*" | "n" => {
                self.end_path(op, args)
            }
            _ => true,
        }
    }

    fn start_subpath(&mut self, pts: Vec<(f32, f32)>, op: Operation) {
        if let Some(c) = self.cur.take() {
            self.subpaths.push(c);
        }
        self.cur = Some(Subpath {
            pts,
            ops: vec![op],
        });
    }

    fn end_path(&mut self, op: &str, args: &[Val]) -> bool {
        if let Some(c) = self.cur.take() {
            self.subpaths.push(c);
        }
        let mut shift = false;
        for sp in &self.subpaths {
            if sp.pts.is_empty() {
                continue;
            }
            let (lo, hi) = x_extents(self.ctm, &sp.pts);
            if hi < self.band_left {
                continue;
            }
            if lo > self.band_left + self.cut {
                shift = true;
                continue;
            }
            return false; // 子路径跨越移除带
        }
        let wv = match (shift, self.shift_vec()) {
            (true, Some(w)) => w,
            (true, None) => return false,
            (false, _) => (0.0, 0.0),
        };
        for o in self.preamble.drain(..) {
            self.out.push(o);
        }
        for sp in self.subpaths.drain(..) {
            if shift && !sp.pts.is_empty() && x_extents(self.ctm, &sp.pts).0 > self.band_left + self.cut {
                for mut o in sp.ops {
                    shift_path_op(&mut o, wv);
                    self.out.push(o);
                }
            } else {
                self.out.extend(sp.ops);
            }
        }
        self.emit(op, args);
        true
    }

    /// Do：Form 用临时 Walk 量测真实墨迹范围（与空白检测同口径），Image 按单位正方形
    fn exec_do(&mut self, args: &[Val]) -> bool {
        let name = match args.last() {
            Some(Val::Name(n)) => n.clone(),
            _ => {
                self.emit("Do", args);
                return true;
            }
        };
        let res = match self.res {
            Some(r) => r,
            None => {
                self.emit("Do", args);
                return true;
            }
        };
        let (_, obj) = match find_xobject(self.doc, res, &name) {
            Some(x) => x,
            None => {
                self.emit("Do", args);
                return true;
            }
        };
        let d = match obj_dict(obj) {
            Some(d) => d,
            None => {
                self.emit("Do", args);
                return true;
            }
        };
        let subtype = d.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
        match subtype {
            Some(s) if s == b"Form" => self.do_form(obj, d, args),
            Some(s) if s == b"Image" => {
                let matrix = Walk::matrix_of(d, b"Matrix").unwrap_or(Mat::I);
                let c = Mat::mul(matrix, self.ctm);
                let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)];
                let (lo, hi) = x_extents(c, &corners);
                self.emit_shifted(args, lo, hi)
            }
            _ => {
                self.emit("Do", args);
                true
            }
        }
    }

    fn do_form(&mut self, obj: &'a Object, d: &'a Dictionary, args: &[Val]) -> bool {
        let matrix = Walk::matrix_of(d, b"Matrix").unwrap_or(Mat::I);
        let combined = Mat::mul(matrix, self.ctm);
        let data = match obj
            .as_stream()
            .ok()
            .and_then(|st| st.decompressed_content().ok())
        {
            Some(data) => data,
            None => {
                self.emit("Do", args);
                return true;
            }
        };
        let form_res = sub_dict(self.doc, d, b"Resources");
        // 以 Form 自身墨迹范围分类；文本状态与第一遍一致传入，保证度量口径相同
        let mut w = Walk::new(self.doc);
        w.ctm = combined;
        w.tfs = self.tfs;
        w.tw = self.tw;
        w.tc = self.tc;
        w.tz = self.tz;
        w.tl = self.tl;
        w.font = self.font;
        w.walk(&data, form_res, 0);
        if w.intervals.is_empty() {
            // Form 无墨迹：无内容可移，原样通过
            self.emit("Do", args);
            return true;
        }
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for &(a, b) in &w.intervals {
            lo = lo.min(a);
            hi = hi.max(b);
        }
        self.emit_shifted(args, lo, hi)
    }

    /// 按设备 x 范围发射 Do：全左原样、全右包 q/cm/Q 移位、跨带回退
    fn emit_shifted(&mut self, args: &[Val], lo: f32, hi: f32) -> bool {
        let opn = self.op_from("Do", args);
        if hi < self.band_left {
            self.out.push(opn);
            true
        } else if lo > self.band_left + self.cut {
            let wv = match self.shift_vec() {
                Some(w) => w,
                None => return false,
            };
            self.out.push(Operation::new("q", vec![]));
            self.out.push(Operation::new(
                "cm",
                vec![
                    1.0.into(),
                    0.0.into(),
                    0.0.into(),
                    1.0.into(),
                    wv.0.into(),
                    wv.1.into(),
                ],
            ));
            self.out.push(opn);
            self.out.push(Operation::new("Q", vec![]));
            true
        } else {
            false
        }
    }

    /// 字体解析（Resources 用页面级 self.res；口径见 resolve_font_id）
    fn resolve_font(&mut self, name: &[u8]) {
        if let Some(id) = resolve_font_id(self.doc, self.res, name, &mut self.fonts) {
            self.font = Some(id);
        }
    }

    /// 当前字体信息
    fn font_info(&self) -> Option<&FontInfo> {
        self.font.and_then(|id| self.fonts.get(&id))
    }

    /// TJ 数组的总 advance（口径见 tj_advance 自由函数）
    fn tj_advance(&self, items: &[Val]) -> f32 {
        tj_advance(self.font_info(), items, self.tfs, self.tw, self.tc, self.tz)
    }

    /// 字符串 advance（口径见 str_advance 自由函数）
    fn string_advance(&self, s: &[u8]) -> f32 {
        str_advance(self.font_info(), s, self.tfs, self.tw, self.tc, self.tz)
    }

    fn op_from(&self, name: &str, args: &[Val]) -> Operation {
        let operands: Vec<Object> = args.iter().map(val_to_obj).collect();
        Operation::new(name, operands)
    }

    fn emit(&mut self, name: &str, args: &[Val]) {
        self.out.push(self.op_from(name, args));
    }
}

/// Val → lopdf Object（字符串用字面量形式，write_string 会自动转义）
fn val_to_obj(v: &Val) -> Object {
    match v {
        Val::Num(n) => Object::Real(*n),
        Val::Name(n) => Object::Name(n.clone()),
        Val::Str(s) => Object::String(s.clone(), StringFormat::Literal),
        Val::Arr(items) => Object::Array(items.iter().map(val_to_obj).collect()),
    }
}

/// Val::Str 便捷构造
fn str_obj(s: &[u8]) -> Object {
    Object::String(s.to_vec(), StringFormat::Literal)
}

/// 将操作符第 idx 个数字操作数加上 delta（此处生成的操作数均为 Real）
fn add_to_real(op: &mut Operation, idx: usize, delta: f32) {
    if delta != 0.0 {
        if let Some(Object::Real(v)) = op.operands.get_mut(idx) {
            *v += delta;
        }
    }
}

/// 路径构造操作符的坐标按操作空间平移 (dx, dy)
fn shift_path_op(op: &mut Operation, w: (f32, f32)) {
    let idx: &[(usize, usize)] = match op.operator.as_str() {
        "m" | "l" | "re" => &[(0, 1)],
        "c" => &[(0, 1), (2, 3), (4, 5)],
        "v" | "y" => &[(0, 1), (2, 3)],
        _ => return,
    };
    for &(xi, yi) in idx {
        if let Some(Object::Real(v)) = op.operands.get_mut(xi) {
            *v += w.0;
        }
        if let Some(Object::Real(v)) = op.operands.get_mut(yi) {
            *v += w.1;
        }
    }
}
