# pdf-crop-dual 测试套件实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 pdf-crop-dual 建立完备的单元测试与集成测试，全部测试代码位于 `tests/` 目录。

**Architecture:** 先将 `main.rs` 的 5 个纯逻辑函数迁入 `lib.rs` 并开放测试所需私有项的可见性（不改行为）；然后按模块在 `tests/` 下写 9 个测试文件（内存合成 lopdf 文档驱动单元测试），最后用真实 `test.pdf` + 二进制 + gs 写 e2e（lopfd 结构断言 + 像素级/文本层断言）。设计规格见 `docs/superpowers/specs/2026-09-12-test-suite-design.md`。

**Tech Stack:** Rust 2024、lopdf 0.45（唯一依赖，测试零新增依赖）、gs 10.02.1（e2e 渲染，缺失时优雅跳过）。

**背景约定（执行者必读）：**
- 本项目是「为既有代码补测试」（characterization testing）：测试预期值按**当前实际行为**断言；若某测试失败，先核实验断言是否写错（改测试）；若确认代码行为与 CLAUDE.md 不变量冲突，**停下报告**，不擅自改 src。
- 代码注释与测试名/断言消息用中文，与项目一致。
- 环境实测基线：test.pdf 74 页 1008×661.5pt；最小空白 122.9pt（第 1 页）；第 30/48/74 页无墨迹→回退 clip 方案；spec=100 → 输出宽 908；spec=500 → 收敛 122.9（stdout「提示」）；spec=0.5 → exit 1「错误：空白宽度必须大于 1 pt，且页面需存在足够的中间空白」。
- 本计划在当前分支 master 直接执行（用户未要求 worktree）。

## 文件布局

```
修改  src/lib.rs        加 pub 可见性；接收 5 个迁入函数；imports 补 Stream/dictionary
修改  src/main.rs       删除 5 个已迁函数；更新 imports 与 compute_cut 调用点
修改  CLAUDE.md         （Task 13）「没有测试框架」一节改为 cargo test 说明
新建  tests/common/mod.rs    共享助手（合成文档、二进制运行、gs、PGM、文本层、断言）
新建  tests/geometry.rs      Mat / x_extents / clip_intervals_to_bbox
新建  tests/lexer.rs         Tok 词法器
新建  tests/fonts.rs         FontInfo / W 解析 / advance
新建  tests/object_access.rs get_mediabox / get_resources / page_resources_dict
新建  tests/detect_gap.rs    空白检测
新建  tests/walk.rs          Walk 内容流遍历
新建  tests/rewrite.rs       rewrite_page 格式保留重写
新建  tests/main_logic.rs    迁入的 5 个纯逻辑函数
新建  tests/e2e.rs           CLI + test.pdf 端到端（lopfd 结构 + gs 像素/文本层）
```

---

### Task 0: 迁移前基线快照

**Files:** 无代码改动（仅生成基线文件到 /tmp/migcheck/）

- [ ] **Step 1: 构建并生成迁移前输出**

```bash
cd /home/stevens/development/cc/pdf-crop-dual
cargo build
mkdir -p /tmp/migcheck
target/debug/pdf-crop-dual test.pdf /tmp/migcheck/before.pdf 100 >/tmp/migcheck/before.log 2>&1
echo "exit=$?"; ls -la /tmp/migcheck/before.pdf
```

Expected: `exit=0`，before.pdf 生成（约 1.5MB）。

---

### Task 1: src 重构——可见性 + 迁移 5 个纯函数

**Files:**
- Modify: `src/lib.rs`（imports、pub 可见性、文件尾部追加 5 函数）
- Modify: `src/main.rs`（imports、compute_cut 调用点、删除 5 函数）

- [ ] **Step 1: lib.rs imports 补 Stream 与 dictionary 宏**

`src/lib.rs` 第 4 行：

```rust
// 原：use lopdf::{Document, Dictionary, Object, ObjectId, StringFormat};
use lopdf::{dictionary, Document, Dictionary, Object, ObjectId, Stream, StringFormat};
```

- [ ] **Step 2: lib.rs 加 pub 可见性（共 4 组）**

按下列清单在 `src/lib.rs` 中做精确替换（每个 `fn`/`struct`/`enum`/`const` 前加 `pub `）：

1. 几何（约 112-193 行区域）：`struct Mat` → `pub struct Mat`；`const I: Mat` → `pub const I: Mat`；`fn of` / `fn translate` / `fn mul` / `fn x_of` → `pub fn`；`fn x_extents` → `pub fn x_extents`；`fn clip_intervals_to_bbox` → `pub fn clip_intervals_to_bbox`。
2. 字体（约 196-339 行区域）：`enum FontInfo` → `pub enum FontInfo`；`fn glyph_width`、`fn parse_w_entry`、`fn descendant_cid_font`、`fn build_cid_font_info`、`fn build_font_info`、`fn resolve_font_id` → `pub fn`。
3. 词法器（约 343-818 行区域）：`enum Val`、`enum Item`、`enum Parsed` → `pub enum`；`struct Tok<'a>` → `pub struct Tok<'a>`，其字段 `data`/`pos` 加 `pub`；`impl<'a> Tok<'a>` 内全部 16 个方法（`skip_ws_comments`、`peek`、`peek2`、`read_word`、`parse_name`、`parse_num`、`parse_lit_string`、`parse_escape`、`parse_hex_string`、`parse_val`、`parse_array`、`skip_dict`、`skip_value`、`next_item`、`handle_inline_image`、`inline_image_length`）→ `pub fn`。
4. advance 与重写工具：`fn str_advance`、`fn tj_advance`、`fn val_to_obj`、`fn add_to_real`、`fn shift_path_op` → `pub fn`。

保持私有（勿动）：`onum`、`obj_dict`、`sub_dict`、`page_inherit`、`find_xobject`、`nums_at`、`font_object_id`、`str_obj`、`Rewriter` 及其方法、`Subpath`、`Walk` 的私有字段。

- [ ] **Step 3: lib.rs 文件尾部追加迁入的 5 个函数**

在 `src/lib.rs` 末尾（`shift_path_op` 之后）追加：

```rust
// ===================== 页面重建辅助函数（自 main.rs 迁入，行为不变） =====================

/// 收敛实际移除宽度：不超过所有页最小空白宽度；cut<=1 时 Err
/// 返回 (实际 cut, 最小检测到的空白宽；无任何 gap 页时为 f32::INFINITY)
pub fn compute_cut(gaps: &[Option<(f32, f32)>], gap_width: f32) -> Result<(f32, f32), String> {
    let min_detected = gaps
        .iter()
        .copied()
        .flatten()
        .map(|(l, r)| r - l)
        .fold(f32::INFINITY, f32::min);
    let cut = gap_width.min(min_detected);
    if cut <= 1.0 {
        return Err("空白宽度必须大于 1 pt，且页面需存在足够的中间空白".into());
    }
    Ok((cut, min_detected))
}

/// 构建封装原页面内容的 Form XObject 流（BBox = 原页面框，Resources 从页复制）
pub fn build_form_stream(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    resources: &Object,
    content: Vec<u8>,
) -> Stream {
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
    Stream::new(form_dict, content)
}

/// 将 Form XObject 注册进页面 /Resources /XObject（内联字典提取为独立对象）
pub fn register_form_xobject(
    doc: &mut Document,
    page_dict: &Dictionary,
    page_id: ObjectId,
    form_name: &[u8],
    form_id: ObjectId,
) {
    // 确保页面有 Resources 对象
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
        xobjects.set(form_name.to_vec(), Object::Reference(form_id));
        res_dict.set("XObject", Object::Dictionary(xobjects));
    }
}

/// 构建新页面内容流：左半保留 [x1, band_left]，右半保留并整体左移 cut。
/// 注意裁剪矩形必须在 cm 之前定义（新页面坐标系），否则会随平移一起偏移
pub fn build_crop_content(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    band_left: f32,
    cut: f32,
    form_name: &[u8],
) -> Content {
    let height = y2 - y1;
    Content {
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
            Operation::new("Do", vec![Object::Name(form_name.to_vec())]),
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
            Operation::new("Do", vec![Object::Name(form_name.to_vec())]),
            Operation::new("Q", vec![]),
        ],
    }
}

/// 替换页面 Contents/MediaBox/CropBox（CropBox 仅当已存在），删除 TrimBox/BleedBox/ArtBox
pub fn update_page_boxes(
    doc: &mut Document,
    page_id: ObjectId,
    new_content_id: ObjectId,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    cut: f32,
) {
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
```

- [ ] **Step 4: main.rs 更新 imports 与 compute_cut 调用点**

`src/main.rs` 第 1-7 行替换为（保留 `use std::env;`——`parse_args` 仍用 `env::args()`；`Dictionary` 类型迁移后 main.rs 不再使用，移除）：

```rust
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use std::env;

use pdf_crop_dual::{
    build_crop_content, build_form_stream, compute_cut, detect_gap, get_mediabox, get_resources,
    page_resources_dict, register_form_xobject, rewrite_page, update_page_boxes, Walk,
};
```

第 25-26 行（`compute_cut` 调用点）替换为：

```rust
    // 实际移除宽度不超过所有页的最小空白宽度，保证每页输出宽度一致
    let gaps: Vec<Option<(f32, f32)>> = plans.iter().map(|p| p.gap).collect();
    let (cut, min_detected) = match compute_cut(&gaps, gap_width) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("错误：{}", msg);
            std::process::exit(1);
        }
    };
    if min_detected.is_finite() && (cut - gap_width).abs() > 0.005 {
        println!(
            "提示：最窄页面的空白仅 {:.1} pt，移除宽度由 {:.1} 调整为 {:.1} pt",
            min_detected, gap_width, cut
        );
    }
```

- [ ] **Step 5: 删除 main.rs 中的 5 个已迁函数**

从 `src/main.rs` 完整删除以下 5 个函数（含 doc 注释，约第 117-136、227-247、249-281、283-332、334-365 行区域）：`compute_cut`、`build_form_stream`、`register_form_xobject`、`build_crop_content`、`update_page_boxes`。保留 `PagePlan`、`scan_page`、`rebuild_page`、`parse_args`、`main`。

- [ ] **Step 6: 构建验证**

Run: `cargo build 2>&1 | tail -5`
Expected: `Finished` 无 error。若出现 unused import 警告（如 main.rs 中 `Content`/`Operation` 已移除），确认 imports 已按 Step 4 更新。

- [ ] **Step 7: 行为不变验证（像素全等）**

```bash
target/debug/pdf-crop-dual test.pdf /tmp/migcheck/after.pdf 100 >/tmp/migcheck/after.log 2>&1
diff /tmp/migcheck/before.log /tmp/migcheck/after.log && echo "LOG-OK"
rm -rf /tmp/migcheck/render-b /tmp/migcheck/render-a && mkdir -p /tmp/migcheck/render-b /tmp/migcheck/render-a
gs -dNOPAUSE -dBATCH -dQUIET -sDEVICE=pgmraw -r72 -sOutputFile=/tmp/migcheck/render-b/p-%d.pgm /tmp/migcheck/before.pdf
gs -dNOPAUSE -dBATCH -dQUIET -sDEVICE=pgmraw -r72 -sOutputFile=/tmp/migcheck/render-a/p-%d.pgm /tmp/migcheck/after.pdf
python3 - <<'EOF'
def parse_pgm(path):
    data = open(path, "rb").read()
    pos, toks = 0, []
    while len(toks) < 4:
        while pos < len(data) and data[pos:pos+1].isspace(): pos += 1
        if data[pos:pos+1] == b"#":
            while data[pos:pos+1] != b"\n": pos += 1
            continue
        s = pos
        while pos < len(data) and not data[pos:pos+1].isspace(): pos += 1
        toks.append(data[s:pos])
    w, h = int(toks[1]), int(toks[2])
    pos += 1
    return w, h, data[pos:pos + w * h]

bad = 0
for i in range(1, 75):
    wb, hb, pb = parse_pgm(f"/tmp/migcheck/render-b/p-{i}.pgm")
    wa, ha, pa = parse_pgm(f"/tmp/migcheck/render-a/p-{i}.pgm")
    if (wb, hb) != (wa, ha) or pb != pa:
        bad += 1
        print(f"page {i} DIFFERS")
print("PIXEL-OK" if bad == 0 else f"PIXEL-FAIL ({bad} pages)")
EOF
```

Expected: `LOG-OK`（两 log 除页码行外一致；页码行格式相同可直接 `diff` 全量比较，若页间顺序输出完全一致则直接 `diff before.log after.log` 通过亦可）与 `PIXEL-OK`。

- [ ] **Step 8: Commit**

```bash
git add src/lib.rs src/main.rs
git commit -m "重构：main.rs 纯逻辑函数迁入 lib 并开放测试可见性（行为不变）

compute_cut 改签名返回 Result 以便测试错误路径；5 个函数行为逐字节不变，
已用 test.pdf 74 页 gs 72dpi 渲染像素全等比对验证。

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 2: 共享测试助手 tests/common/mod.rs

**Files:**
- Create: `tests/common/mod.rs`

- [ ] **Step 1: 写入完整文件**

```rust
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
```

- [ ] **Step 2: 编译验证（临时引用文件冒烟）**

common/mod.rs 单独存在时 cargo 不编译它（无测试目标引用）。用临时引用文件验证：

1. 创建 `tests/zz_smoke.rs`，内容为：

```rust
#[allow(dead_code)]
mod common;
```

2. Run: `cargo test --test zz_smoke`
Expected: 编译通过且 0 警告，运行结果 `0 passed; 0 failed`

3. 删除 `tests/zz_smoke.rs`（临时文件，不提交）

- [ ] **Step 3: Commit**

```bash
git add tests/common/mod.rs
git commit -m "test：新增共享测试助手（合成文档构造器、gs 渲染、PGM 解析、操作断言）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 3: tests/geometry.rs（Mat / x_extents / clip）

**Files:**
- Create: `tests/geometry.rs`

- [ ] **Step 1: 写入完整文件**

```rust
//! Mat 矩阵、x_extents、clip_intervals_to_bbox 单元测试

mod common;

use pdf_crop_dual::{clip_intervals_to_bbox, x_extents, Mat};

#[test]
fn 单位矩阵保持坐标() {
    assert_eq!(Mat::I.x_of(3.0, 4.0), 3.0);
}

#[test]
fn 平移矩阵移动点() {
    let m = Mat::translate(10.0, 5.0);
    assert_eq!(m.x_of(3.0, 4.0), 13.0);
}

#[test]
fn mul先应用m1再应用m2() {
    let scale2 = Mat::of(2.0, 0.0, 0.0, 2.0, 0.0, 0.0);
    // 先平移 10 再缩放 2：x' = 2(x+10) = 2x + 20
    let m = Mat::mul(Mat::translate(10.0, 0.0), scale2);
    assert_eq!(m.x_of(1.0, 0.0), 22.0);
    // 先缩放 2 再平移 10：x' = 2x + 10
    let m = Mat::mul(scale2, Mat::translate(10.0, 0.0));
    assert_eq!(m.x_of(1.0, 0.0), 12.0);
}

#[test]
fn mul平移先于旋转() {
    let r90 = Mat::of(0.0, -1.0, 1.0, 0.0, 0.0, 0.0);
    let m = Mat::mul(Mat::translate(10.0, 30.0), r90);
    // 先平移：(x+10, y+30)；再按该矩阵映射 (u, v) → (v, -u)：x' = y + 30
    // e' 中的 m1.f*m2.c 项（30·1）在此首次非零，漏写即失败
    assert_eq!(m.x_of(0.0, 0.0), 30.0);
    assert_eq!(m.x_of(1.0, 2.0), 32.0);
}

#[test]
fn 平移合成() {
    let m = Mat::mul(Mat::translate(1.0, 2.0), Mat::translate(3.0, 4.0));
    assert_eq!(m.x_of(0.0, 0.0), 4.0);
}

#[test]
fn 旋转矩阵交换轴() {
    // 90° 旋转（该矩阵实现映射 (x, y) → (y, -x)），故 x' = y
    let m = Mat::of(0.0, -1.0, 1.0, 0.0, 0.0, 0.0);
    assert_eq!(m.x_of(3.0, 7.0), 7.0);
    assert_eq!(m.x_of(-2.0, 5.0), 5.0);
}

#[test]
fn x_extents缩放与平移() {
    let m = Mat::of(2.0, 0.0, 0.0, 2.0, 5.0, 0.0);
    let (lo, hi) = x_extents(m, &[(1.0, 0.0), (3.0, 0.0), (2.0, 5.0)]);
    assert_eq!(lo, 7.0); // 2*1 + 5
    assert_eq!(hi, 11.0); // 2*3 + 5
}

#[test]
fn x_extents负缩放min_max正确互换() {
    let m = Mat::of(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    let (lo, hi) = x_extents(m, &[(1.0, 0.0), (3.0, 0.0), (2.0, 0.0)]);
    assert_eq!(lo, -3.0);
    assert_eq!(hi, -1.0);
}

#[test]
fn clip钳位跨边区间并剔除区间外者() {
    let mut iv = vec![(5.0, 15.0), (100.0, 200.0), (20.0, 30.0)];
    clip_intervals_to_bbox(&mut iv, 0, 0.0, 10.0);
    assert_eq!(iv, vec![(5.0, 10.0)]);
}

#[test]
fn clip左缘钳位与左外侧剔除() {
    // 左缘钳位（a < bx0）、左侧完全在外剔除、框内原样通过、双缘同时钳位
    let mut iv = vec![(-5.0, 3.0), (-20.0, -5.0), (2.0, 8.0), (-5.0, 100.0)];
    clip_intervals_to_bbox(&mut iv, 0, 0.0, 10.0);
    assert_eq!(iv, vec![(0.0, 3.0), (2.0, 8.0), (0.0, 10.0)]);
}

#[test]
fn clip只处理mark之后的区间() {
    // 前缀区间刻意选用裁剪下会变化的形态：忽略 mark（全部裁剪）的实现会失败
    let mut iv = vec![(5.0, 15.0), (5.0, 15.0)];
    clip_intervals_to_bbox(&mut iv, 1, 0.0, 10.0);
    assert_eq!(iv, vec![(5.0, 15.0), (5.0, 10.0)]);
}

#[test]
fn clip退化区间被丢弃() {
    let mut iv = vec![(10.0, 10.0), (12.0, 8.0)];
    clip_intervals_to_bbox(&mut iv, 0, 0.0, 10.0);
    assert!(iv.is_empty());
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --test geometry 2>&1 | tail -8`
Expected: `test result: ok. 12 passed; 0 failed`（首次运行同时编译 common 模块，验证 Task 2 代码）。

- [ ] **Step 3: Commit**

```bash
git add tests/geometry.rs
git commit -m "test：矩阵几何单元测试（Mat 约定与乘法顺序、x_extents、BBox 钳位）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 4: tests/lexer.rs（Tok 词法器）

**Files:**
- Create: `tests/lexer.rs`

- [ ] **Step 1: 写入完整文件**

```rust
//! Tok 内容流词法器单元测试
#![allow(non_snake_case)]


mod common;

use pdf_crop_dual::{Item, Parsed, Tok, Val};

fn items(data: &str) -> Vec<Item> {
    let mut tk = Tok {
        data: data.as_bytes(),
        pos: 0,
    };
    let mut out = Vec::new();
    while let Some(it) = tk.next_item() {
        out.push(it);
    }
    out
}

fn parse(data: &str) -> Parsed {
    let mut tk = Tok {
        data: data.as_bytes(),
        pos: 0,
    };
    tk.parse_val()
}

fn val(it: &Item) -> &Val {
    match it {
        Item::Val(v) => v,
        Item::Op(_) => panic!("期望 Val，实际是 Op"),
    }
}

fn is_num(v: &Val, x: f32) -> bool {
    matches!(v, Val::Num(n) if (n - x).abs() < 1e-6)
}

fn is_name(v: &Val, n: &[u8]) -> bool {
    matches!(v, Val::Name(x) if x == n)
}

fn is_str(v: &Val, s: &[u8]) -> bool {
    matches!(v, Val::Str(x) if x == s)
}

fn is_op(it: &Item, v: &str) -> bool {
    matches!(it, Item::Op(o) if o == v)
}

#[test]
fn 数字形式() {
    let v = items("123 -4.5 +7 .25 3.");
    assert_eq!(v.len(), 5);
    for (i, x) in v.iter().zip([123.0, -4.5, 7.0, 0.25, 3.0]) {
        assert!(is_num(val(i), x), "数字解析错误: {:?} != {x}", val(i));
    }
}

#[test]
fn 孤立符号是错误() {
    assert!(matches!(parse("-"), Parsed::Err));
    assert!(items("-x").is_empty());
    assert!(items("").is_empty());
    assert!(items("  \n\t ").is_empty());
}

#[test]
fn 名称与十六进制转义() {
    let v = items("/F1 /Co#6Cor");
    assert!(is_name(val(&v[0]), b"F1"));
    assert!(is_name(val(&v[1]), b"Color")); // C o #6C('l') o r
    // % 是注释起始，终止名称
    let v = items("/A%B");
    assert_eq!(v.len(), 1);
    assert!(is_name(val(&v[0]), b"A"));
}

#[test]
fn 字面量串转义() {
    let r = |input: &str, expect: &[u8]| {
        assert!(
            matches!(parse(input), Parsed::Val(Val::Str(s)) if s == expect),
            "输入 {input:?} 未按预期解析"
        );
    };
    r(r"(a\nb)", b"a\nb");
    r(r"(\(\)\\)", b"()\\");
    r(r"(\101)", b"A"); // 八进制
    r(r"(\12)", &[10]); // 八进制 12 = 十进制 10
    r(r"(\8)", b"8"); // 非法八进制退化为原字符
}

#[test]
fn 字面量串行续() {
    assert!(matches!(parse("(a\\\nb)"), Parsed::Val(Val::Str(s)) if s == b"ab"));
    assert!(matches!(parse("(a\\\r\nb)"), Parsed::Val(Val::Str(s)) if s == b"ab"));
}

#[test]
fn 字面量串嵌套括号() {
    assert!(matches!(parse("(a(b)c)"), Parsed::Val(Val::Str(s)) if s == b"a(b)c"));
}

#[test]
fn 字面量串未闭合是错误() {
    assert!(matches!(parse("(abc"), Parsed::Err));
}

#[test]
fn 十六进制串() {
    assert!(matches!(parse("<4849>"), Parsed::Val(Val::Str(s)) if s == b"HI"));
    assert!(matches!(parse("<48 49>"), Parsed::Val(Val::Str(s)) if s == b"HI"));
    // 奇数位补 0：4 → "40" → 0x40
    assert!(matches!(parse("<4>"), Parsed::Val(Val::Str(s)) if s == &[0x40]));
    assert!(matches!(parse("<zz>"), Parsed::Err));
}

#[test]
fn 数组嵌套() {
    let v = items("[1 2.5 (s) /n [3]]");
    assert_eq!(v.len(), 1);
    assert!(matches!(&v[0], Item::Val(Val::Arr(a)) if a.len() == 5
        && is_num(&a[0], 1.0) && is_num(&a[1], 2.5)
        && is_str(&a[2], b"s") && is_name(&a[3], b"n")
        && matches!(&a[4], Val::Arr(b) if b.len() == 1 && is_num(&b[0], 3.0))));
}

#[test]
fn 数组内裸词与字典被丢弃() {
    let v = items("[BT 1 << /A 2 >> 2]");
    assert!(matches!(&v[0], Item::Val(Val::Arr(a)) if a.len() == 2
        && is_num(&a[0], 1.0) && is_num(&a[1], 2.0)));
}

#[test]
fn 字典整体跳过且位置正确() {
    assert!(matches!(
        parse("<< /A 1 /B [1 2] /C (x) /D 5 0 R >>"),
        Parsed::DictSkipped
    ));
    assert!(matches!(parse("<< >>"), Parsed::DictSkipped));
    // 字典后的 token 位置正确
    let v = items("<< /D 5 0 R >> BT");
    assert_eq!(v.len(), 1);
    assert!(is_op(&v[0], "BT"));
}

#[test]
fn 注释() {
    let v = items("% c\n1");
    assert_eq!(v.len(), 1);
    assert!(is_num(val(&v[0]), 1.0));
    // 文件尾无换行注释
    assert_eq!(items("1 % trailing comment").len(), 1);
}

#[test]
fn 混合流顺序() {
    let v = items("1.5 -2 /F1 (abc) [1 2 3] BT");
    assert_eq!(v.len(), 6);
    assert!(is_num(val(&v[0]), 1.5));
    assert!(is_num(val(&v[1]), -2.0));
    assert!(is_name(val(&v[2]), b"F1"));
    assert!(is_str(val(&v[3]), b"abc"));
    assert!(matches!(&v[4], Item::Val(Val::Arr(a)) if a.len() == 3));
    assert!(is_op(&v[5], "BT"));
}

#[test]
fn 内联图像BI_ID_EI() {
    let data = b"BI << /Width 1 /Length 4 >> ID\nwxyz\nEI S";
    let mut tk = Tok { data, pos: 0 };
    assert!(matches!(tk.next_item(), Some(Item::Op(ref o)) if o == "BI"));
    assert!(tk.handle_inline_image());
    assert!(matches!(tk.next_item(), Some(Item::Op(ref o)) if o == "S"));
}

#[test]
fn 内联图像CRLF变体() {
    let data = b"BI << /Length 4 >> ID\r\nwxyz\r\nEI";
    let mut tk = Tok { data, pos: 0 };
    assert!(matches!(tk.next_item(), Some(Item::Op(ref o)) if o == "BI"));
    assert!(tk.handle_inline_image());
    assert!(tk.next_item().is_none());
}

#[test]
fn 内联图像损坏() {
    // 缺 EI
    let data = b"BI << /Length 4 >> ID\nwxyz\nXX";
    let mut tk = Tok { data, pos: 0 };
    let _ = tk.next_item();
    assert!(!tk.handle_inline_image());
    // Length 越界
    let data = b"BI << /Length 100 >> ID\nwxyz EI";
    let mut tk = Tok { data, pos: 0 };
    let _ = tk.next_item();
    assert!(!tk.handle_inline_image());
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --test lexer 2>&1 | tail -15`
Expected: `test result: ok. 16 passed; 0 failed`。

- [ ] **Step 3: Commit**

```bash
git add tests/lexer.rs
git commit -m "test：内容流词法器单元测试（数字/名称/串转义/数组/字典跳过/注释/内联图像）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 5: tests/fonts.rs（字体宽度与 advance）

**Files:**
- Create: `tests/fonts.rs`

- [ ] **Step 1: 写入完整文件**

```rust
//! 字体宽度解析与文本 advance 单元测试
#![allow(non_snake_case)]


mod common;

use common::{cid_font, page_resources, type0_font, type1_font};
use lopdf::{dictionary, Document, Object};
use pdf_crop_dual::{
    build_font_info, glyph_width, parse_w_entry, resolve_font_id, str_advance, tj_advance,
    FontInfo, Val,
};
use std::collections::HashMap;

fn w_pair(first: f32, w: f32) -> Object {
    Object::Array(vec![first.into(), w.into()])
}

fn w_range(first: f32, last: f32, w: f32) -> Object {
    Object::Array(vec![first.into(), last.into(), w.into()])
}

fn w_pair_array(first: f32, ws: &[f32]) -> Object {
    let inner = Object::Array(ws.iter().map(|w| Object::Real(*w)).collect());
    Object::Array(vec![Object::Real(first), inner])
}

#[test]
fn str_advance无字体按1em() {
    assert_eq!(str_advance(None, b"AB", 12.0, 0.0, 0.0, 100.0), 24.0);
}

#[test]
fn str_advance_simple字体() {
    let f = FontInfo::Simple {
        first: 32,
        widths: vec![500.0, 1000.0],
    };
    // 空格(32)→500, '!'(33)→1000；@tfs=10 → 5 + 10
    assert_eq!(str_advance(Some(&f), b" !", 10.0, 0.0, 0.0, 100.0), 15.0);
    // 码字超出 Widths 范围 → 1em 回退
    assert_eq!(str_advance(Some(&f), b"C", 10.0, 0.0, 0.0, 100.0), 10.0);
}

#[test]
fn glyph_width_simple越界回退1em() {
    let f = FontInfo::Simple {
        first: 32,
        widths: vec![500.0],
    };
    assert_eq!(glyph_width(&f, 32), 500.0);
    assert_eq!(glyph_width(&f, 33), 1000.0);
    assert_eq!(glyph_width(&f, 31), 1000.0);
}

#[test]
fn glyph_width_cid缺省用DW() {
    let mut widths = HashMap::new();
    widths.insert(0x0041u16, 600.0);
    let f = FontInfo::Cid {
        widths,
        dw: 800.0,
    };
    assert_eq!(glyph_width(&f, 0x0041), 600.0);
    assert_eq!(glyph_width(&f, 0x0042), 800.0);
}

#[test]
fn str_advance_cid按2字节解码() {
    let mut widths = HashMap::new();
    widths.insert(0x0041u16, 600.0);
    let f = FontInfo::Cid {
        widths,
        dw: 800.0,
    };
    assert_eq!(
        str_advance(Some(&f), b"\x00\x41\x00\x42", 10.0, 0.0, 0.0, 100.0),
        14.0
    );
    // 尾部奇数字节被忽略
    assert_eq!(
        str_advance(Some(&f), b"\x00\x41\x00\x42\x00", 10.0, 0.0, 0.0, 100.0),
        14.0
    );
}

#[test]
fn str_advance_Tc逐字形累加() {
    assert_eq!(str_advance(None, b"AB", 10.0, 0.0, 0.5, 100.0), 21.0);
}

#[test]
fn str_advance_Tz仅对空格生效() {
    // 空格码 32：附加 tz/100 * tw
    assert_eq!(str_advance(None, b"A B", 10.0, 2.0, 0.0, 100.0), 32.0);
    assert_eq!(str_advance(None, b"A B", 10.0, 2.0, 0.0, 50.0), 31.0);
}

#[test]
fn tj_advance含缩进项() {
    let items = vec![Val::Str(b"AB".to_vec()), Val::Num(-200.0), Val::Str(b"C".to_vec())];
    // 20 - 2 + 10
    assert_eq!(tj_advance(None, &items, 10.0, 0.0, 0.0, 100.0), 28.0);
}

#[test]
fn parse_w_entry三种形式() {
    let mut m = HashMap::new();
    parse_w_entry(&[5.0.into(), 600.0.into()], &mut m);
    assert_eq!(m.get(&5u16), Some(&600.0));

    let mut m = HashMap::new();
    parse_w_entry(&[5.0.into(), 8.0.into(), 500.0.into()], &mut m);
    assert_eq!(m.len(), 4);
    for c in 5..=8 {
        assert_eq!(m.get(&c), Some(&500.0));
    }

    // first > last：不插入
    let mut m = HashMap::new();
    parse_w_entry(&[9.0.into(), 5.0.into(), 500.0.into()], &mut m);
    assert!(m.is_empty());

    let mut m = HashMap::new();
    let entry = w_pair_array(10.0, &[100.0, 200.0, 300.0]);
    parse_w_entry(entry.as_array().unwrap(), &mut m);
    assert_eq!(m.get(&10u16), Some(&100.0));
    assert_eq!(m.get(&11u16), Some(&200.0));
    assert_eq!(m.get(&12u16), Some(&300.0));
}

#[test]
fn parse_w_entry_u16环绕() {
    let mut m = HashMap::new();
    let entry = w_pair_array(65535.0, &[100.0, 200.0]);
    parse_w_entry(entry.as_array().unwrap(), &mut m);
    assert_eq!(m.get(&65535u16), Some(&100.0));
    assert_eq!(m.get(&0u16), Some(&200.0));
}

#[test]
fn build_font_info_type1() {
    let mut doc = Document::new();
    let id = type1_font(&mut doc, 32, &[500.0, 1000.0]);
    let d = doc.get_object(id).unwrap().as_dict().unwrap().clone();
    match build_font_info(&doc, &d) {
        FontInfo::Simple { first, widths } => {
            assert_eq!(first, 32);
            assert_eq!(widths, vec![500.0, 1000.0]);
        }
        FontInfo::Cid { .. } => panic!("期望 Simple"),
        FontInfo::Unknown => panic!("期望 Simple"),
    }
}

#[test]
fn build_font_info缺Widths为Unknown() {
    let mut doc = Document::new();
    let dict = dictionary! { "Subtype" => "Type1" };
    let id = doc.add_object(Object::Dictionary(dict));
    let d = doc.get_object(id).unwrap().as_dict().unwrap().clone();
    match build_font_info(&doc, &d) {
        FontInfo::Unknown => {}
        _ => panic!("期望 Unknown"),
    }
}

#[test]
fn build_font_info_type0_cid() {
    let mut doc = Document::new();
    let w = vec![
        w_range(5.0, 8.0, 500.0),
        w_pair_array(10.0, &[100.0, 200.0]),
        w_pair(9.0, 600.0),
    ];
    let cid = cid_font(&mut doc, 700.0, w);
    let id = type0_font(&mut doc, cid);
    let d = doc.get_object(id).unwrap().as_dict().unwrap().clone();
    match build_font_info(&doc, &d) {
        FontInfo::Cid { widths, dw } => {
            assert_eq!(dw, 700.0);
            for c in 5..=8 {
                assert_eq!(widths.get(&c), Some(&500.0));
            }
            assert_eq!(widths.get(&10u16), Some(&100.0));
            assert_eq!(widths.get(&11u16), Some(&200.0));
            assert_eq!(widths.get(&9u16), Some(&600.0));
            assert_eq!(widths.len(), 7); // 4（5..=8）+ 2（10,11）+ 1（9）
        }
        _ => panic!("期望 Cid"),
    }
}

#[test]
fn build_font_info缺DescendantFonts() {
    let mut doc = Document::new();
    let dict = dictionary! { "Subtype" => "Type0" };
    let id = doc.add_object(Object::Dictionary(dict));
    let d = doc.get_object(id).unwrap().as_dict().unwrap().clone();
    match build_font_info(&doc, &d) {
        FontInfo::Cid { widths, dw } => {
            assert!(widths.is_empty());
            assert_eq!(dw, 1000.0);
        }
        _ => panic!("期望 Cid"),
    }
}

#[test]
fn resolve_font_id缓存与失败路径() {
    let mut doc = Document::new();
    let f = type1_font(&mut doc, 32, &[500.0]);
    let res = page_resources(&[(b"F1", f)], &[]);
    let mut fonts = HashMap::new();
    assert_eq!(resolve_font_id(&doc, Some(&res), b"F1", &mut fonts), Some(f));
    assert_eq!(fonts.len(), 1);
    // 二次调用命中缓存，不重复解析：预置哨兵，调用后应被保留
    *fonts.get_mut(&f).unwrap() = FontInfo::Unknown;
    assert_eq!(resolve_font_id(&doc, Some(&res), b"F1", &mut fonts), Some(f));
    assert!(matches!(fonts.get(&f), Some(FontInfo::Unknown)));
    // 名称不存在
    assert_eq!(resolve_font_id(&doc, Some(&res), b"Nope", &mut fonts), None);
    // res 为 None
    assert_eq!(resolve_font_id(&doc, None, b"F1", &mut fonts), None);
    // 条目非间接引用
    let mut f = dictionary! {};
    f.set(b"Subtype", Object::Name(b"Type1".to_vec()));
    let mut fonts_sub = dictionary! {};
    fonts_sub.set(b"F1", Object::Dictionary(f));
    let mut res2 = dictionary! {};
    res2.set(b"Font", Object::Dictionary(fonts_sub));
    assert_eq!(resolve_font_id(&doc, Some(&res2), b"F1", &mut fonts), None);
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --test fonts 2>&1 | tail -10`
Expected: `test result: ok. 15 passed; 0 failed`。

- [ ] **Step 3: Commit**

```bash
git add tests/fonts.rs
git commit -m "test：字体宽度与 advance 单元测试（Simple/CID、W 三形式、Tc/Tz、缓存）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 6: tests/object_access.rs（MediaBox/Resources 访问）

**Files:**
- Create: `tests/object_access.rs`

- [ ] **Step 1: 写入完整文件**

```rust
//! get_mediabox / get_resources / page_resources_dict 单元测试
#![allow(non_snake_case)]


mod common;

use common::page_tree;
use lopdf::{dictionary, Document, Dictionary, Object};
use pdf_crop_dual::{get_mediabox, get_resources, page_resources_dict};

fn real_vec(xs: &[f32]) -> Object {
    Object::Array(xs.iter().map(|x| Object::Real(*x)).collect())
}

fn int_vec(xs: &[i32]) -> Object {
    Object::Array(xs.iter().map(|x| Object::Integer(*x as i64)).collect())
}

#[test]
fn mediabox页内Real() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "MediaBox" => real_vec(&[0.0, 0.0, 1008.0, 661.5]) },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(
        get_mediabox(&doc, d, page_id).unwrap(),
        vec![0.0, 0.0, 1008.0, 661.5]
    );
}

#[test]
fn mediabox页内Integer兼容() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "MediaBox" => int_vec(&[0, 0, 1008, 661]) },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(get_mediabox(&doc, d, page_id).unwrap(), vec![0.0, 0.0, 1008.0, 661.0]);
}

#[test]
fn mediabox从Parent继承() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page" },
        dictionary! { "MediaBox" => real_vec(&[10.0, 20.0, 300.0, 400.0]) },
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(
        get_mediabox(&doc, d, page_id).unwrap(),
        vec![10.0, 20.0, 300.0, 400.0]
    );
}

#[test]
fn mediabox缺失报错() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(&mut doc, dictionary! { "Type" => "Page" }, Dictionary::new());
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(get_mediabox(&doc, d, page_id).is_err());
}

#[test]
fn mediabox含非数字报错() {
    let mut doc = Document::new();
    let arr = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(100),
        Object::Name(b"Bad".to_vec()),
    ]);
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "MediaBox" => arr },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    let e = get_mediabox(&doc, d, page_id).unwrap_err();
    assert!(e.to_string().contains("非数字值"), "实际: {e}");
}

#[test]
fn resources页内内联() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => dictionary! { "Font" => dictionary! {} } },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(get_resources(&doc, d, page_id).unwrap().as_dict().unwrap().has(b"Font"));
}

#[test]
fn resources经引用() {
    let mut doc = Document::new();
    let res_id = doc.add_object(Object::Dictionary(dictionary! { "Font" => dictionary! {} }));
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => Object::Reference(res_id) },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(get_resources(&doc, d, page_id).unwrap().as_dict().unwrap().has(b"Font"));
}

#[test]
fn resources从Parent继承() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page" },
        dictionary! { "Resources" => dictionary! { "Font" => dictionary! {} } },
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(get_resources(&doc, d, page_id).unwrap().as_dict().unwrap().has(b"Font"));
}

#[test]
fn page_resources_dict三种情形() {
    let mut doc = Document::new();
    // 内联字典
    let (p1, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => dictionary! { "Font" => dictionary! {} } },
        Dictionary::new(),
    );
    let d1 = doc.get_object(p1).unwrap().as_dict().unwrap();
    assert!(page_resources_dict(&doc, d1, p1).unwrap().has(b"Font"));
    // 间接引用
    let res_id = doc.add_object(Object::Dictionary(dictionary! { "Font" => dictionary! {} }));
    let (p2, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => Object::Reference(res_id) },
        Dictionary::new(),
    );
    let d2 = doc.get_object(p2).unwrap().as_dict().unwrap();
    assert!(page_resources_dict(&doc, d2, p2).unwrap().has(b"Font"));
    // 缺失
    let (p3, _) = page_tree(&mut doc, dictionary! { "Type" => "Page" }, Dictionary::new());
    let d3 = doc.get_object(p3).unwrap().as_dict().unwrap();
    assert!(page_resources_dict(&doc, d3, p3).is_none());
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --test object_access 2>&1 | tail -6`
Expected: `test result: ok. 9 passed; 0 failed`。

- [ ] **Step 3: Commit**

```bash
git add tests/object_access.rs
git commit -m "test：对象访问单元测试（MediaBox Integer 兼容/继承/报错、Resources 三途径）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 7: tests/detect_gap.rs（空白检测）

**Files:**
- Create: `tests/detect_gap.rs`

- [ ] **Step 1: 写入完整文件**

```rust
//! detect_gap 中间空白检测单元测试

mod common;

use pdf_crop_dual::detect_gap;

const X1: f32 = 0.0;
const X2: f32 = 1008.0; // 中线 504

#[test]
fn 正常双栏() {
    // 左最大右缘 400 → +2；右最小左缘 560 → -2
    assert_eq!(
        detect_gap(&[(72.0, 400.0), (560.0, 900.0)], X1, X2),
        Some((402.0, 558.0))
    );
    // 非对称框（x1≠0）：mid=(x1+x2)/2=514，贴线数据可区分 mid=x2/2 类回归
    assert_eq!(
        detect_gap(&[(72.0, 510.0), (530.0, 900.0)], 10.0, 1018.0),
        Some((512.0, 528.0))
    );
}

#[test]
fn 跨中线无空白() {
    assert_eq!(detect_gap(&[(400.0, 600.0)], X1, X2), None);
    // 跨线 + 两侧齐全：跨线 early-return，而非只取同侧区间
    assert_eq!(detect_gap(&[(72.0, 300.0), (400.0, 600.0), (700.0, 950.0)], X1, X2), None);
}

#[test]
fn 余量后不足10pt无空白() {
    // l = 500+2 = 502, r = 510-2 = 508 → 宽 6 < 10
    assert_eq!(detect_gap(&[(0.0, 500.0), (510.0, 800.0)], X1, X2), None);
}

#[test]
fn 余量后恰好10pt保留() {
    // l = 496+2 = 498, r = 510-2 = 508 → 宽 10，不 < 10
    assert_eq!(
        detect_gap(&[(0.0, 496.0), (510.0, 800.0)], X1, X2),
        Some((498.0, 508.0))
    );
}

#[test]
fn 单边内容与空列表无空白() {
    assert_eq!(detect_gap(&[(0.0, 400.0)], X1, X2), None);
    assert_eq!(detect_gap(&[(600.0, 900.0)], X1, X2), None);
    assert_eq!(detect_gap(&[], X1, X2), None);
}

#[test]
fn 贴中线边界归属() {
    // b == mid 归左、a == mid 归右（严格不等式才判跨线）：宽间隙下贴线区间照常分类
    assert_eq!(detect_gap(&[(0.0, 504.0), (600.0, 900.0)], X1, X2), Some((506.0, 598.0)));
    assert_eq!(detect_gap(&[(72.0, 400.0), (504.0, 900.0)], X1, X2), Some((402.0, 502.0)));
    // 贴线且余量后负宽 → None
    assert_eq!(detect_gap(&[(400.0, 504.0), (504.0, 600.0)], X1, X2), None);
}

#[test]
fn 多区间取极值且与顺序无关() {
    let v = detect_gap(
        &[(72.0, 380.0), (700.0, 950.0), (100.0, 400.0), (560.0, 900.0), (150.0, 390.0), (620.0, 880.0)],
        X1,
        X2,
    )
    .unwrap();
    assert_eq!(v, (402.0, 558.0));
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --test detect_gap 2>&1 | tail -6`
Expected: `test result: ok. 7 passed; 0 failed`。

- [ ] **Step 3: Commit**

```bash
git add tests/detect_gap.rs
git commit -m "test：空白检测单元测试（双栏/跨线/10pt 阈值/中线边界/乱序）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 8: tests/walk.rs（内容流遍历与墨迹区间）

**Files:**
- Create: `tests/walk.rs`

- [ ] **Step 1: 写入完整文件**

```rust
//! Walk 内容流遍历单元测试（墨迹 x 区间收集、Form/Image、文本定位、边界情形）
#![allow(non_snake_case)]


mod common;

use common::{form_xobject, image_xobject, page_resources, type1_font};
use lopdf::{Dictionary, Document, Object, ObjectId};
use pdf_crop_dual::Walk;

/// 含 F1 字体的文档：FirstChar=32，Widths 覆盖码字 32..=66
/// （空格=500、'A'=500、'B'=1000、其余 0）→ @10pt 'A'=5pt, 'B'=10pt
fn doc_with_font() -> (Document, ObjectId) {
    let mut doc = Document::new();
    let mut widths = vec![0.0f32; 35];
    widths[0] = 500.0; // 空格（码字 32）
    widths[33] = 500.0; // 'A'（码字 65）
    widths[34] = 1000.0; // 'B'（码字 66）
    let f = type1_font(&mut doc, 32, &widths);
    let res = page_resources(&[(b"F1", f)], &[]);
    let res_id = doc.add_object(Object::Dictionary(res));
    (doc, res_id)
}

fn res_of(doc: &Document, id: ObjectId) -> &Dictionary {
    doc.get_object(id).unwrap().as_dict().unwrap()
}

fn walk(doc: &Document, content: &str, res: Option<&Dictionary>) -> Vec<(f32, f32)> {
    let mut w = Walk::new(doc);
    w.walk(content.as_bytes(), res, 0);
    w.intervals
}

// ===================== 文本 =====================

#[test]
fn 文本无字体按1em() {
    // F9 不存在 → 字宽回退 1em：'A'+'B' = 20
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F9 10 Tf 100 500 Td (AB) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(100.0, 120.0)]);
}

#[test]
fn 文本Tm绝对定位() {
    // 'A' = 500@12pt = 6
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 12 Tf 1 0 0 1 200 100 Tm (A) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(200.0, 206.0)]);
}

#[test]
fn Td偏移在文本空间经Tm缩放() {
    // Tm 线性部分 (2,2)：Td(50,0) → e' = 50*2 + 100 = 200；advance 也乘 ptm.a=2
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 2 0 0 2 100 100 Tm 50 0 Td (A) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(200.0, 210.0)]);
}

#[test]
fn Td叠加在Tm之后() {
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 1 0 0 1 100 500 Tm 50 0 Td (A) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(150.0, 155.0)]);
}

#[test]
fn CTM平移影响文本位置() {
    let (doc, res_id) = doc_with_font();
    let iv = walk(
        &doc,
        "q 1 0 0 1 100 0 cm BT /F1 10 Tf 100 500 Td (AB) Tj ET Q",
        Some(res_of(&doc, res_id)),
    );
    // x0 = 100 + 100 = 200，advance = 15
    assert_eq!(iv, vec![(200.0, 215.0)]);
}

#[test]
fn CTM缩放影响advance() {
    let (doc, res_id) = doc_with_font();
    let iv = walk(
        &doc,
        "q 2 0 0 2 0 0 cm BT /F1 10 Tf 100 500 Td (AB) Tj ET Q",
        Some(res_of(&doc, res_id)),
    );
    // x0 = 200，x1 = 200 + 15*2 = 230
    assert_eq!(iv, vec![(200.0, 230.0)]);
}

#[test]
fn TJ数组缩进计入() {
    // A=5, -500/1000*10=-5, B=10 → 10
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 100 500 Td [(A) -500 (B)] TJ ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(100.0, 110.0)]);
}

#[test]
fn Tw不额外加到非空格字形() {
    // 项目口径：str_advance 仅空格按 Tz/100·Tw 加宽，Tw 不逐字形累加
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 2 Tw 100 500 Td (AB) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(100.0, 115.0)]);
}

#[test]
fn Tc每字形累加() {
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 1 Tc 100 500 Td (AB) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(100.0, 117.0)]);
}

#[test]
fn Tz仅对空格生效() {
    // A: 5, 空格: 5+200/100*2 = 9, B: 10 → 24（Tz 系数 tz/100，Tw 仅经空格项生效）
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 2 Tw 200 Tz 100 500 Td (A B) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(100.0, 124.0)]);
}

#[test]
fn 双引号先应用TwTc再换行显示() {
    // " (aw, ac, s)：先设 tw/tc 再换行显示（x 保持行首）；A=6, 空格=8, B=11 → 25
    // 回归（忽略 aw/ac）→ 20 → (100,120)
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 100 500 Td 2 1 (A B) \" ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(100.0, 125.0)]);
}

#[test]
fn 单引号换行显示用当前TwTc() {
    // ' 取显示时刻的当前 tw/tc；回归（忽略 Tw/Tc）→ 20 → (100,120)
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 2 Tw 1 Tc 100 500 Td (A B) ' ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(100.0, 125.0)]);
}

#[test]
fn 星号换行x仍从行首起() {
    // T* 仅下移一行，x 保持行首；两行均 AB=15
    // 回归（T* 带 x 位移）→ 第二行始于 115 → (115,130)
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 12 TL 100 500 Td (AB) Tj T* (AB) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(100.0, 115.0), (100.0, 115.0)]);
}

#[test]
fn TD含x分量相对定位() {
    // TD(tx,ty)：行起点移动 (tx,ty) 并同时设 TL=-ty；x=100+10=110
    // 回归（TD 被忽略）→ (100,105)
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "BT /F1 10 Tf 100 500 Td 10 -12 TD (A) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(110.0, 115.0)]);
}

#[test]
fn BT外的文本操作被忽略() {
    // 第二个 BT 块验证 BT 重置 tlm：不继承第一块行矩阵（e 回到 200 而非叠加到 300）
    let (doc, res_id) = doc_with_font();
    let iv = walk(
        &doc,
        "BT /F1 10 Tf 100 500 Td (A) Tj ET 200 0 Td (B) Tj BT /F1 10 Tf 200 500 Td (B) Tj ET",
        Some(res_of(&doc, res_id)),
    );
    assert_eq!(iv, vec![(100.0, 105.0), (200.0, 210.0)]);
}

// ===================== 路径 =====================

#[test]
fn 路径线段范围() {
    let (doc, _) = doc_with_font();
    assert_eq!(walk(&doc, "0 0 m 100 0 l S", None), vec![(0.0, 100.0)]);
}

#[test]
fn 路径矩形re() {
    let (doc, _) = doc_with_font();
    assert_eq!(walk(&doc, "50 0 100 10 re f", None), vec![(50.0, 150.0)]);
}

#[test]
fn 路径随cm平移() {
    let (doc, _) = doc_with_font();
    let iv = walk(&doc, "q 1 0 0 1 100 0 cm 0 0 m 50 0 l S Q", None);
    assert_eq!(iv, vec![(100.0, 150.0)]);
}

#[test]
fn 路径随cm缩放() {
    let (doc, _) = doc_with_font();
    let iv = walk(&doc, "q 2 0 0 2 100 0 cm 0 0 m 10 0 l S Q", None);
    assert_eq!(iv, vec![(100.0, 120.0)]);
}

#[test]
fn W丢弃路径() {
    let (doc, _) = doc_with_font();
    let iv = walk(&doc, "0 0 m 100 0 l W 20 0 m 30 0 l f", None);
    assert_eq!(iv, vec![(20.0, 30.0)]);
}

#[test]
fn qQ恢复图形状态() {
    let (doc, _) = doc_with_font();
    let iv = walk(&doc, "q 1 0 0 1 100 0 cm 0 0 m 10 0 l S Q 20 0 m 30 0 l S", None);
    assert_eq!(iv, vec![(100.0, 110.0), (20.0, 30.0)]);
}

#[test]
fn m累积多子路径共享终结() {
    // Walk 口径：m 追加新子路径起点，绘制终结汇总全部子路径点集（与重写器多子路径累积一致）
    let (doc, _) = doc_with_font();
    let iv = walk(&doc, "0 0 m 10 0 l 20 0 m 30 0 l S", None);
    assert_eq!(iv, vec![(0.0, 30.0)]);
}

#[test]
fn 曲线控制点计入范围() {
    let (doc, _) = doc_with_font();
    let iv = walk(&doc, "5 0 m 10 20 20 30 30 0 c f", None);
    assert_eq!(iv, vec![(5.0, 30.0)]);
}

#[test]
fn v追加两点() {
    let (doc, _) = doc_with_font();
    let iv = walk(&doc, "0 0 m 10 20 30 0 v f", None);
    assert_eq!(iv, vec![(0.0, 30.0)]);
}

// ===================== Form / Image XObject =====================

#[test]
fn Form按Matrix定位() {
    let mut doc = Document::new();
    let f = form_xobject(&mut doc, b"0 0 100 10 re f", Some([1.0, 0.0, 0.0, 1.0, 500.0, 0.0]), Some([0.0, 0.0, 100.0, 10.0]), None);
    let res = page_resources(&[], &[(b"Fl", f)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let iv = walk(&doc, "/Fl Do", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(500.0, 600.0)]);
}

#[test]
fn Form按BBox裁剪() {
    let mut doc = Document::new();
    // 内容宽 200，BBox 只给 100 → 裁掉右半
    let f = form_xobject(&mut doc, b"0 0 200 10 re f", Some([1.0, 0.0, 0.0, 1.0, 500.0, 0.0]), Some([0.0, 0.0, 100.0, 10.0]), None);
    let res = page_resources(&[], &[(b"Fl", f)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let iv = walk(&doc, "/Fl Do", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(500.0, 600.0)]);
}

#[test]
fn Form无BBox不裁剪() {
    let mut doc = Document::new();
    let f = form_xobject(&mut doc, b"0 0 200 10 re f", Some([1.0, 0.0, 0.0, 1.0, 500.0, 0.0]), None, None);
    let res = page_resources(&[], &[(b"Fl", f)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let iv = walk(&doc, "/Fl Do", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(500.0, 700.0)]);
}

#[test]
fn Form内文本用自身字体() {
    let mut doc = Document::new();
    let mut fw = vec![0.0f32; 35];
    fw[0] = 500.0; fw[33] = 500.0; fw[34] = 1000.0;
    let ffont = type1_font(&mut doc, 32, &fw);
    let fres = page_resources(&[(b"F1", ffont)], &[]);
    let f = form_xobject(&mut doc, b"BT /F1 10 Tf 10 0 Td (AB) Tj ET", Some([1.0, 0.0, 0.0, 1.0, 500.0, 0.0]), None, Some(&fres));
    let pres = page_resources(&[], &[(b"Fl", f)]);
    let res_id = doc.add_object(Object::Dictionary(pres));
    let iv = walk(&doc, "/Fl Do", Some(res_of(&doc, res_id)));
    // x0 = 500 + 10 = 510，advance = 5 + 10 = 15
    assert_eq!(iv, vec![(510.0, 525.0)]);
}

#[test]
fn Form内qQ平衡的cm不外泄() {
    // Form 内 q/Q 包住的 cm(e=500) 在 Do 后复原；页面文本仍从 x=100 起
    // 回归（cm 外泄）→ 文本 x0=600 → (600,605)
    let mut doc = Document::new();
    let mut widths = vec![0.0f32; 35];
    widths[33] = 500.0; // 'A'
    let f = type1_font(&mut doc, 32, &widths);
    let fm = form_xobject(&mut doc, b"q 1 0 0 1 500 0 cm 0 0 10 10 re f Q", None, None, None);
    let res = page_resources(&[(b"F1", f)], &[(b"Fm", fm)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let iv = walk(&doc, "/Fm Do BT /F1 10 Tf 100 500 Td (A) Tj ET", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(500.0, 510.0), (100.0, 105.0)]);
}

#[test]
fn Image按单位正方形量测() {
    let mut doc = Document::new();
    let img = image_xobject(&mut doc, [100.0, 0.0, 0.0, 50.0, 600.0, 10.0]);
    let res = page_resources(&[], &[(b"Im", img)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let iv = walk(&doc, "/Im Do", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(600.0, 700.0)]);
}

#[test]
fn Image在ctm缩放下量测() {
    // 单位正方形四角 ×(Matrix·ctm)：Matrix=[2,0,0,2,100,0]，ctm=2× → x∈(200,204)
    // 回归（忽略 ctm）→ (100,102)；回归（组合顺序颠倒）→ (100,104)
    let mut doc = Document::new();
    let img = image_xobject(&mut doc, [2.0, 0.0, 0.0, 2.0, 100.0, 0.0]);
    let res = page_resources(&[], &[(b"Im", img)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let iv = walk(&doc, "q 2 0 0 2 0 0 cm /Im Do Q", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(200.0, 204.0)]);
}

#[test]
fn 同一Form绘制两次只计一次() {
    // seen_forms 固化口径：重复 Do 不重复量测（并集不变，但区间数减半）
    let mut doc = Document::new();
    let f = form_xobject(&mut doc, b"0 0 10 10 re f", Some([1.0, 0.0, 0.0, 1.0, 500.0, 0.0]), None, None);
    let res = page_resources(&[], &[(b"Fl", f)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let iv = walk(&doc, "/Fl Do /Fl Do", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(500.0, 510.0)]);
}

#[test]
fn 同一Form不同位置第二次漏测() {
    // 已知局限固化：第二次 Do（不同 cm 位置）被 seen_forms 阻断
    let mut doc = Document::new();
    let f = form_xobject(&mut doc, b"0 0 10 10 re f", Some([1.0, 0.0, 0.0, 1.0, 500.0, 0.0]), None, None);
    let res = page_resources(&[], &[(b"Fl", f)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let iv = walk(&doc, "q 1 0 0 1 200 0 cm /Fl Do Q /Fl Do", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(700.0, 710.0)]);
}

/// 构建 n 层 Form 链：F1 内 "/F2 Do" … Fn 内矩形（均无 Matrix/BBox），返回页面 resources 对象 id。
/// 每层 Form 自带 Resources 指向下一层（Do 的名字解析依赖当前层 Resources）
fn build_chain(doc: &mut Document, n: usize) -> ObjectId {
    let mut xobjs: Vec<(Vec<u8>, ObjectId)> = Vec::new();
    let mut prev: Option<(String, ObjectId)> = None;
    for k in (1..=n).rev() {
        let (content, resources) = match &prev {
            Some((p, pid)) => {
                let mut xo = Dictionary::new();
                xo.set(p.as_bytes().to_vec(), Object::Reference(*pid));
                let mut r = Dictionary::new();
                r.set("XObject", Object::Dictionary(xo));
                (format!("/{p} Do").into_bytes(), Some(r))
            }
            None => (b"0 0 10 10 re f".to_vec(), None),
        };
        let id = form_xobject(doc, &content, None, None, resources.as_ref());
        let name = format!("F{k}");
        xobjs.push((name.clone().into_bytes(), id));
        prev = Some((name, id));
    }
    let refs: Vec<(&[u8], ObjectId)> = xobjs.iter().map(|(nm, id)| (nm.as_slice(), *id)).collect();
    let res = page_resources(&[], &refs);
    doc.add_object(Object::Dictionary(res))
}

#[test]
fn Form链8层计入() {
    let mut doc = Document::new();
    let res_id = build_chain(&mut doc, 8);
    // walk_form 在 depth 0..7 放行，第 8 层矩形被量测
    let iv = walk(&doc, "/F1 Do", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(0.0, 10.0)]);
}

#[test]
fn Form链9层被阻断() {
    let mut doc = Document::new();
    let res_id = build_chain(&mut doc, 9);
    // depth 8 达到上限，最内层矩形漏测
    let iv = walk(&doc, "/F1 Do", Some(res_of(&doc, res_id)));
    assert!(iv.is_empty());
}

// ===================== 内联图像 / 损坏流 / 其他 =====================

#[test]
fn 内联图像被跳过且不计区间() {
    let (doc, _) = doc_with_font();
    let content = "0 0 10 10 re f BI << /Length 4 >> ID\nabcd EI 200 0 10 10 re f";
    let iv = walk(&doc, content, None);
    assert_eq!(iv, vec![(0.0, 10.0), (200.0, 210.0)]);
}

#[test]
fn 内联图像缺Length使遍历中止() {
    let (doc, _) = doc_with_font();
    let content = "0 0 10 10 re f BI << /W 4 >> ID\nabcd EI 200 0 10 10 re f";
    let iv = walk(&doc, content, None);
    assert_eq!(iv, vec![(0.0, 10.0)]);
}

#[test]
fn 损坏字符串使遍历中止() {
    let (doc, _) = doc_with_font();
    let content = "0 0 10 10 re f BT (abc Tj ET 200 0 10 10 re f";
    let iv = walk(&doc, content, None);
    assert_eq!(iv, vec![(0.0, 10.0)]);
}

#[test]
fn 未知操作被忽略() {
    let (doc, _) = doc_with_font();
    let iv = walk(&doc, "foo 0 0 10 10 re f", None);
    assert_eq!(iv, vec![(0.0, 10.0)]);
}

#[test]
fn Do未知名称被忽略() {
    let (doc, res_id) = doc_with_font();
    let iv = walk(&doc, "/Missing Do 0 0 10 10 re f", Some(res_of(&doc, res_id)));
    assert_eq!(iv, vec![(0.0, 10.0)]);
}

#[test]
fn 无Resources时Do不生效() {
    let mut doc = Document::new();
    let f = form_xobject(&mut doc, b"0 0 10 10 re f", None, None, None);
    let _ = f;
    let iv = walk(&doc, "/Fl Do 5 0 m 6 0 l S", None);
    assert_eq!(iv, vec![(5.0, 6.0)]);
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --test walk 2>&1 | tail -10`
Expected: `test result: ok. 35 passed; 0 failed`。

- [ ] **Step 3: Commit**

```bash
git add tests/walk.rs
git commit -m "test：Walk 遍历单元测试（文本定位/Tw/Tc/Tz、路径、Form BBox/深度/环、内联图像、损坏流）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 9: tests/rewrite.rs（格式保留式重写器）

**Files:**
- Create: `tests/rewrite.rs`

- [ ] **Step 1: 写入完整文件**

```rust
//! rewrite_page 格式保留式重写单元测试（分类、文本移位修正、Form/Image、回退、与 Walk 一致性）
#![allow(non_snake_case)]


mod common;

use common::{assert_ops, eo, form_xobject, image_xobject, page_resources, type1_font};
use lopdf::{Dictionary, Document, Object, ObjectId};
use pdf_crop_dual::{detect_gap, rewrite_page, Walk};

/// 含 F1 字体：FirstChar=32，Widths 覆盖码字 32..=66
/// （空格=500、'A'=500、'B'=1000、其余 0）→ @10pt 'A'=5pt, 'B'=10pt
fn doc_with_font() -> (Document, ObjectId) {
    let mut doc = Document::new();
    let mut widths = vec![0.0f32; 35];
    widths[0] = 500.0; // 空格（码字 32）
    widths[33] = 500.0; // 'A'（码字 65）
    widths[34] = 1000.0; // 'B'（码字 66）
    let f = type1_font(&mut doc, 32, &widths);
    let res = page_resources(&[(b"F1", f)], &[]);
    let res_id = doc.add_object(Object::Dictionary(res));
    (doc, res_id)
}

fn res_of(doc: &Document, id: ObjectId) -> &Dictionary {
    doc.get_object(id).unwrap().as_dict().unwrap()
}

// 以下多数用例使用 band [500, 600)（band_left=500, cut=100）

#[test]
fn 左侧原样右侧左移() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"BT /F1 10 Tf 100 500 Td (AB) Tj ET 610 0 10 10 re f", Some(res), 500.0, 100.0)
        .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Td", vec![100.0, 500.0]),
            ("Tj", vec![], vec![b"AB".to_vec()], vec![]),
            eo("ET", vec![]),
            eo("re", vec![510.0, 0.0, 10.0, 10.0]),
            eo("f", vec![]),
        ],
        "左原样右左移",
    );
    // TJ 数组：范围 [650, 663]（'A'5 + (-200/1000)·10 + 'B'10 = 13pt）全右 →
    // Tm 左移 100、数组原样重发（元素集 -200/A/B 被钉住）
    let out = rewrite_page(
        &doc,
        b"BT /F1 10 Tf 1 0 0 1 650 500 Tm [(A) -200 (B)] TJ ET",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, 550.0, 500.0]),
            ("TJ", vec![-200.0], vec![b"A".to_vec(), b"B".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "TJ 右侧左移",
    );
}

#[test]
fn 右侧Tm原点仅取CTM逆修正() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"BT /F1 10 Tf 1 0 0 1 650 500 Tm (AB) Tj ET", Some(res), 500.0, 100.0)
        .expect("可重写");
    // 发射 Tm 的 e = 650 - 100 = 550；内部状态仍按原位置校验
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, 550.0, 500.0]),
            ("Tj", vec![], vec![b"AB".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "右侧 Tm",
    );
    // 非单位 Tm 线性部分（2× 缩放）：修正仍只走 CTM 逆（ctm 单位 → de=-100）；
    // 若误用 ptm 逆（det=4）得 de=-50 → 发射 600，打挂
    let out = rewrite_page(
        &doc,
        b"BT /F1 10 Tf 2 0 0 2 650 500 Tm (A) Tj ET",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![2.0, 0.0, 0.0, 2.0, 550.0, 500.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "2x 缩放 Tm",
    );
}

#[test]
fn Td从左跨到右() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"BT /F1 10 Tf 1 0 0 1 100 500 Tm 520 0 Td (A) Tj ET", Some(res), 500.0, 100.0)
        .expect("可重写");
    // 新位置 620（右）、前位置 100（左）：δ = -cut → 发射 520 - 100 = 420
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, 100.0, 500.0]),
            eo("Td", vec![420.0, 0.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "Td 左→右",
    );
    // 2× 缩放行矩阵：Td 偏移经线性部分（e' = 100 + 450·2 = 1000，右）；
    // 修正走 ptm 逆：(-100·2/4, 0) = (-50, 0) → 发射 450 - 50 = 400；
    // 若丢缩放当设备空间，e' = 550 落带内 → None，expect 打挂
    let out = rewrite_page(
        &doc,
        b"BT /F1 10 Tf 2 0 0 2 100 500 Tm 450 0 Td (A) Tj ET",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![2.0, 0.0, 0.0, 2.0, 100.0, 500.0]),
            eo("Td", vec![400.0, 0.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "2x 缩放 Td 跨带",
    );
}

#[test]
fn Td从右跨到左() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"BT /F1 10 Tf 1 0 0 1 650 500 Tm -520 0 Td (A) Tj ET", Some(res), 500.0, 100.0)
        .expect("可重写");
    // Tm 发射 e=550；新位置 130（左）、前位置 650（右）：δ = +cut → 发射 -520 + 100 = -420
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, 550.0, 500.0]),
            eo("Td", vec![-420.0, 0.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "Td 右→左",
    );
}

#[test]
fn 文本跨带回退() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    // 无字体信息：'A'+'B' = 20 → [490, 510] 跨移除带
    assert!(rewrite_page(
        &doc,
        b"BT /F9 10 Tf 1 0 0 1 490 500 Tm (AB) Tj ET",
        Some(res),
        500.0,
        100.0
    )
    .is_none());
}

#[test]
fn 路径边界与跨带() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    // 左缘恰在 band_left+cut → 保守判跨带
    assert!(rewrite_page(&doc, b"600 0 10 10 re f", Some(res), 500.0, 100.0).is_none());
    // 横跨移除带
    assert!(rewrite_page(&doc, b"500 0 100 10 re f", Some(res), 500.0, 100.0).is_none());
    // 右侧可见区内 1pt
    let out = rewrite_page(&doc, b"601 0 10 10 re f", Some(res), 500.0, 100.0).expect("右 1pt");
    assert_ops(&out, &[eo("re", vec![501.0, 0.0, 10.0, 10.0]), eo("f", vec![])], "右 1pt");
    // 左侧区内 1pt
    let out = rewrite_page(&doc, b"489 0 10 10 re f", Some(res), 500.0, 100.0).expect("左 1pt");
    assert_ops(&out, &[eo("re", vec![489.0, 0.0, 10.0, 10.0]), eo("f", vec![])], "左 1pt");
}

#[test]
fn 多子路径独立分类() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"100 0 10 10 re 610 0 10 10 re f", Some(res), 500.0, 100.0)
        .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("re", vec![100.0, 0.0, 10.0, 10.0]),
            eo("re", vec![510.0, 0.0, 10.0, 10.0]),
            eo("f", vec![]),
        ],
        "混合子路径",
    );
    // 任一子路径跨带 → 整页回退
    assert!(rewrite_page(&doc, b"100 0 10 10 re 550 0 10 10 re f", Some(res), 500.0, 100.0).is_none());
}

#[test]
fn cm内路径按操作空间平移() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"q 1 0 0 1 620 0 cm 0 0 10 10 re f Q", Some(res), 500.0, 100.0)
        .expect("可重写");
    // 设备空间 (620,630) 在右；shift 反解到操作空间：0 - 100 = -100
    assert_ops(
        &out,
        &[
            eo("q", vec![]),
            eo("cm", vec![1.0, 0.0, 0.0, 1.0, 620.0, 0.0]),
            eo("re", vec![-100.0, 0.0, 10.0, 10.0]),
            eo("f", vec![]),
            eo("Q", vec![]),
        ],
        "cm 包裹路径",
    );
    // 曲线 c：三对坐标（两控制点 + 端点）各 -100，钉住索引表 (0,1),(2,3),(4,5)
    let out = rewrite_page(
        &doc,
        b"610 0 m 630 20 650 0 640 30 c 620 20 610 10 610 0 c f",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("m", vec![510.0, 0.0]),
            eo("c", vec![530.0, 20.0, 550.0, 0.0, 540.0, 30.0]),
            eo("c", vec![520.0, 20.0, 510.0, 10.0, 510.0, 0.0]),
            eo("f", vec![]),
        ],
        "c 曲线移位",
    );
    // 剪切 cm（b=1）：shift_vec = (-cut·d/det, cut·b/det) = (-100, 100)，
    // 钉住 y 分量（此前所有路径用例 ctm.b=0，y 分量恒 0 无法行使）
    let out = rewrite_page(
        &doc,
        b"q 1 1 0 1 0 0 cm 610 0 10 10 re f Q",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("q", vec![]),
            eo("cm", vec![1.0, 1.0, 0.0, 1.0, 0.0, 0.0]),
            eo("re", vec![510.0, 100.0, 10.0, 10.0]),
            eo("f", vec![]),
            eo("Q", vec![]),
        ],
        "剪切 cm 路径",
    );
}

#[test]
fn Form_Do分类() {
    let mut doc = Document::new();
    let fl = form_xobject(&mut doc, b"72 0 300 10 re f", None, None, None); // 墨迹 [72,372]
    let fr = form_xobject(&mut doc, b"72 0 300 10 re f", Some([1.0, 0.0, 0.0, 1.0, 504.0, 0.0]), None, None); // 墨迹 [576,876]
    let res = page_resources(&[], &[(b"FL", fl), (b"FR", fr)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let res = res_of(&doc, res_id);
    // band [424, 524)：FL 右缘 372 < 424 原样；FR 左缘 576 > 524 → q/cm 包裹左移
    let out = rewrite_page(&doc, b"/FL Do /FR Do", Some(res), 424.0, 100.0).expect("可重写");
    assert_ops(
        &out,
        &[
            ("Do", vec![], vec![], vec![b"FL".to_vec()]),
            eo("q", vec![]),
            eo("cm", vec![1.0, 0.0, 0.0, 1.0, -100.0, 0.0]),
            ("Do", vec![], vec![], vec![b"FR".to_vec()]),
            eo("Q", vec![]),
        ],
        "Form 分类",
    );
}

#[test]
fn Form跨带回退() {
    let mut doc = Document::new();
    let f = form_xobject(&mut doc, b"450 0 100 10 re f", None, None, None);
    let res = page_resources(&[], &[(b"FL", f)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let res = res_of(&doc, res_id);
    // 墨迹 [450,550] 跨 band [424,524)
    assert!(rewrite_page(&doc, b"/FL Do", Some(res), 424.0, 100.0).is_none());
}

#[test]
fn Form无墨迹原样通过() {
    let mut doc = Document::new();
    let f = form_xobject(&mut doc, b"q Q", None, None, None);
    let res = page_resources(&[], &[(b"FE", f)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"/FE Do", Some(res), 424.0, 100.0).expect("可重写");
    assert_ops(&out, &[("Do", vec![], vec![], vec![b"FE".to_vec()])], "空墨迹 Form");
}

#[test]
fn Image_Do分类() {
    let mut doc = Document::new();
    let il = image_xobject(&mut doc, [100.0, 0.0, 0.0, 50.0, 100.0, 10.0]); // [100,200]
    let ir = image_xobject(&mut doc, [100.0, 0.0, 0.0, 50.0, 620.0, 10.0]); // [620,720]
    let res = page_resources(&[], &[(b"IL", il), (b"IR", ir)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"/IL Do /IR Do", Some(res), 424.0, 100.0).expect("可重写");
    assert_ops(
        &out,
        &[
            ("Do", vec![], vec![], vec![b"IL".to_vec()]),
            eo("q", vec![]),
            eo("cm", vec![1.0, 0.0, 0.0, 1.0, -100.0, 0.0]),
            ("Do", vec![], vec![], vec![b"IR".to_vec()]),
            eo("Q", vec![]),
        ],
        "Image 分类",
    );
}

#[test]
fn cm定位右侧文本块合成Tm() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(
        &doc,
        b"q 1 0 0 1 650 0 cm BT /F1 10 Tf (AB) Tj ET Q",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    // 块内无 Tm/Td：合成 Tm 携带移位（e = 0 + (-cut) = -100，cm 坐标系）
    assert_ops(
        &out,
        &[
            eo("q", vec![]),
            eo("cm", vec![1.0, 0.0, 0.0, 1.0, 650.0, 0.0]),
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, -100.0, 0.0]),
            ("Tj", vec![], vec![b"AB".to_vec()], vec![]),
            eo("ET", vec![]),
            eo("Q", vec![]),
        ],
        "cm 右侧文本块",
    );
}

#[test]
fn cm定位左侧文本块不合成Tm() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(
        &doc,
        b"q 1 0 0 1 100 0 cm BT /F1 10 Tf (AB) Tj ET Q",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("q", vec![]),
            eo("cm", vec![1.0, 0.0, 0.0, 1.0, 100.0, 0.0]),
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            ("Tj", vec![], vec![b"AB".to_vec()], vec![]),
            eo("ET", vec![]),
            eo("Q", vec![]),
        ],
        "cm 左侧文本块",
    );
}

#[test]
fn T星号分解为Td() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(
        &doc,
        b"q 1 0 0 1 650 0 cm BT /F1 10 Tf 14 TL (A) Tj T* (B) Tj ET Q",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("q", vec![]),
            eo("cm", vec![1.0, 0.0, 0.0, 1.0, 650.0, 0.0]),
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("TL", vec![14.0]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, -100.0, 0.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("Td", vec![0.0, -14.0]),
            ("Tj", vec![], vec![b"B".to_vec()], vec![]),
            eo("ET", vec![]),
            eo("Q", vec![]),
        ],
        "T* 分解",
    );
}

#[test]
fn 单引号分解为Td加Tj() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(
        &doc,
        b"q 1 0 0 1 650 0 cm BT /F1 10 Tf (A) Tj (B) ' ET Q",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("q", vec![]),
            eo("cm", vec![1.0, 0.0, 0.0, 1.0, 650.0, 0.0]),
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, -100.0, 0.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("Td", vec![0.0, 0.0]),
            ("Tj", vec![], vec![b"B".to_vec()], vec![]),
            eo("ET", vec![]),
            eo("Q", vec![]),
        ],
        "' 分解",
    );
}

#[test]
fn 双引号重发TwTc且首行移承载移位() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(
        &doc,
        b"q 1 0 0 1 650 0 cm BT /F1 10 Tf 2 3 (AB) \" ET Q",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    // 块首行移无前序 Tm：δ = -cut - 0 → Td(-100, 0) 承载移位
    assert_ops(
        &out,
        &[
            eo("q", vec![]),
            eo("cm", vec![1.0, 0.0, 0.0, 1.0, 650.0, 0.0]),
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tw", vec![2.0]),
            eo("Tc", vec![3.0]),
            eo("Td", vec![-100.0, 0.0]),
            ("Tj", vec![], vec![b"AB".to_vec()], vec![]),
            eo("ET", vec![]),
            eo("Q", vec![]),
        ],
        "\" 分解",
    );
}

#[test]
fn 块内颜色状态操作原样保留() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"BT /F1 10 Tf 0.5 0.5 1 rg 100 500 Td (A) Tj ET", Some(res), 500.0, 100.0)
        .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("rg", vec![0.5, 0.5, 1.0]),
            eo("Td", vec![100.0, 500.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "块内状态操作",
    );
}

#[test]
fn 不支持语法回退() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    // 未知操作
    assert!(rewrite_page(&doc, b"foo 1", Some(res), 500.0, 100.0).is_none());
    // 内联图像
    assert!(rewrite_page(&doc, b"BI /Length 2 ID\nxx EI", Some(res), 500.0, 100.0).is_none());
    // BT 未闭合
    assert!(rewrite_page(&doc, b"BT /F1 10 Tf 100 500 Td (A) Tj", Some(res), 500.0, 100.0).is_none());
    // 文本块内 q/Q
    assert!(rewrite_page(&doc, b"BT /F1 10 Tf 100 500 Td (A) Tj q Q ET", Some(res), 500.0, 100.0).is_none());
    // 文本块内路径操作
    assert!(rewrite_page(&doc, b"BT /F1 10 Tf 100 500 Td (A) Tj 0 0 1 1 re ET", Some(res), 500.0, 100.0).is_none());
    // 退化矩阵（det=0）下的右侧内容
    assert!(rewrite_page(&doc, b"q 0 0 0 0 610 0 cm 0 0 10 10 re f Q", Some(res), 500.0, 100.0).is_none());
}

#[test]
fn Do未知XObject原样通过() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"/FMiss Do", Some(res), 500.0, 100.0).expect("可重写");
    assert_ops(&out, &[("Do", vec![], vec![], vec![b"FMiss".to_vec()])], "未知 XObject");
}

#[test]
fn Td同侧不变() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"BT /F1 10 Tf 1 0 0 1 650 500 Tm 10 0 Td (A) Tj ET", Some(res), 500.0, 100.0)
        .expect("可重写");
    // 新位置 660（右）、前位置 650（右）→ delta=0，Td 操作数不变
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, 550.0, 500.0]),
            eo("Td", vec![10.0, 0.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "Td 同侧",
    );
    // LL 格：左→左（100→120）delta=0，Td 原样，闭合 2×2 侧转移矩阵
    let out = rewrite_page(
        &doc,
        b"BT /F1 10 Tf 1 0 0 1 100 500 Tm 20 0 Td (A) Tj ET",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, 100.0, 500.0]),
            eo("Td", vec![20.0, 0.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "Td 左→左",
    );
}

#[test]
fn Td落带内回退() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    // 新位置 100+400=500 == band_left，不满足严格不等式 → None
    assert!(rewrite_page(&doc, b"BT /F1 10 Tf 1 0 0 1 100 500 Tm 400 0 Td (A) Tj ET", Some(res), 500.0, 100.0)
        .is_none());
}

#[test]
fn 文本左缘hi严格小于band_left() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    // 'A'=5pt → [490,495]，hi < 500 → 左侧原样
    let out = rewrite_page(&doc, b"BT /F1 10 Tf 1 0 0 1 490 500 Tm (A) Tj ET", Some(res), 500.0, 100.0)
        .expect("左临界通过");
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, 490.0, 500.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
        ],
        "左临界",
    );
    // 无字体信息：'AB' = 2em = 20pt → [480,500]，hi == band_left → 严格不等式不满足 → None
    assert!(rewrite_page(&doc, b"BT /F9 10 Tf 1 0 0 1 480 500 Tm (AB) Tj ET", Some(res), 500.0, 100.0)
        .is_none());
}

#[test]
fn h闭合不增点() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"610 300 m 620 300 l 620 310 l h f", Some(res), 500.0, 100.0)
        .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("m", vec![510.0, 300.0]),
            eo("l", vec![520.0, 300.0]),
            eo("l", vec![520.0, 310.0]),
            eo("h", vec![]),
            eo("f", vec![]),
        ],
        "h 不增点",
    );
}

#[test]
fn Wn裁剪子路径同样分类() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"100 0 10 10 re W n 610 0 10 10 re W n", Some(res), 500.0, 100.0)
        .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("re", vec![100.0, 0.0, 10.0, 10.0]),
            eo("W", vec![]),
            eo("n", vec![]),
            eo("re", vec![510.0, 0.0, 10.0, 10.0]),
            eo("W", vec![]),
            eo("n", vec![]),
        ],
        "W/n 分类",
    );
}

#[test]
fn ET必发且块后路径完整() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(
        &doc,
        b"BT /F1 10 Tf 1 0 0 1 650 500 Tm (A) Tj ET 610 300 m 620 300 l S",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, 550.0, 500.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
            eo("m", vec![510.0, 300.0]),
            eo("l", vec![520.0, 300.0]),
            eo("S", vec![]),
        ],
        "ET 必发 + 块后路径",
    );
}

#[test]
fn 块外文本操作原样通过() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(
        &doc,
        b"BT /F1 10 Tf 1 0 0 1 100 500 Tm (A) Tj ET (B) Tj 610 300 m 620 300 l S",
        Some(res),
        500.0,
        100.0,
    )
    .expect("可重写");
    assert_ops(
        &out,
        &[
            eo("BT", vec![]),
            ("Tf", vec![10.0], vec![], vec![b"F1".to_vec()]),
            eo("Tm", vec![1.0, 0.0, 0.0, 1.0, 100.0, 500.0]),
            ("Tj", vec![], vec![b"A".to_vec()], vec![]),
            eo("ET", vec![]),
            ("Tj", vec![], vec![b"B".to_vec()], vec![]),
            eo("m", vec![510.0, 300.0]),
            eo("l", vec![520.0, 300.0]),
            eo("S", vec![]),
        ],
        "块外原样",
    );
}

#[test]
fn sh与BDC回退() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    // sh（着色填充，未知操作）
    assert!(rewrite_page(&doc, b"/P0 sh", Some(res), 500.0, 100.0).is_none());
    // BDC 带名称操作数（不在允许列表）
    assert!(rewrite_page(&doc, b"/MC BDC", Some(res), 500.0, 100.0).is_none());
}

#[test]
fn ctm缩放下右路径操作空间偏移() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"q 2 0 0 2 0 0 cm 310 150 10 10 re f Q", Some(res), 500.0, 100.0)
        .expect("可重写");
    // 设备空间 (620,640) 在右；shift_vec = (-cut·d/det, cut·b/det) = (-100·2/4, 0) = (-50, 0)
    assert_ops(
        &out,
        &[
            eo("q", vec![]),
            eo("cm", vec![2.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
            eo("re", vec![260.0, 150.0, 10.0, 10.0]),
            eo("f", vec![]),
            eo("Q", vec![]),
        ],
        "ctm 缩放",
    );
}

#[test]
fn m前构造操作进preamble原样补发() {
    let (doc, res_id) = doc_with_font();
    let res = res_of(&doc, res_id);
    let out = rewrite_page(&doc, b"610 300 l 610 300 m 620 300 l f", Some(res), 500.0, 100.0)
        .expect("可重写");
    // 首个 m 之前的 l（无活动子路径）进 preamble，终结操作前原样补发；m 后子路径分类移位
    assert_ops(
        &out,
        &[
            eo("l", vec![610.0, 300.0]),
            eo("m", vec![510.0, 300.0]),
            eo("l", vec![520.0, 300.0]),
            eo("f", vec![]),
        ],
        "preamble",
    );
}

#[test]
fn 重写输出与Walk口径一致() {
    let mut doc = Document::new();
    let fl = form_xobject(&mut doc, b"72 0 300 10 re f", None, None, None);
    let fr = form_xobject(&mut doc, b"72 0 300 10 re f", Some([1.0, 0.0, 0.0, 1.0, 504.0, 0.0]), None, None);
    let mut widths = vec![0.0f32; 35];
    widths[0] = 500.0;
    widths[33] = 500.0;
    widths[34] = 1000.0;
    let f = type1_font(&mut doc, 32, &widths);
    let res = page_resources(&[(b"F1", f)], &[(b"FL", fl), (b"FR", fr)]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let res = res_of(&doc, res_id);
    let content = b"/FL Do /FR Do BT /F1 10 Tf 1 0 0 1 650 500 Tm (AB) Tj ET";

    // 第一遍：Walk 量测墨迹，detect_gap 定位空白带
    let mut w = Walk::new(&doc);
    w.walk(content, Some(res), 0);
    let (l, r) = detect_gap(&w.intervals, 0.0, 1008.0).expect("检测到空白带");
    let cut = 100.0;
    let band_left = (l + r) / 2.0 - cut / 2.0;

    let out = rewrite_page(&doc, content, Some(res), band_left, cut).expect("可重写");

    // 第二遍：输出内容的区间集合 == 左区间 ∪ (右区间 - cut)
    let bytes = out.encode().expect("编码");
    let mut w2 = Walk::new(&doc);
    w2.walk(&bytes, Some(res), 0);
    // 左 Form (72,372) 不动；右 Form (576,876) → (476,776)；文本 (650,665) → (550,565)
    let expected: Vec<(f32, f32)> = vec![(72.0, 372.0), (476.0, 776.0), (550.0, 565.0)];
    let mut got = w2.intervals;
    got.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    assert_eq!(got, expected);
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --test rewrite 2>&1 | tail -10`
Expected: `test result: ok. 31 passed; 0 failed`。

- [ ] **Step 3: Commit**

```bash
git add tests/rewrite.rs
git commit -m "test：重写器单元测试（左右分类、Tm/Td/T*/引号修正、Form/Image、回退、Walk 一致性）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 10: tests/main_logic.rs（迁入的 5 个纯函数 + 重写工具函数）

**Files:**
- Create: `tests/main_logic.rs`

- [ ] **Step 1: 写入完整文件**

```rust
//! 自 main.rs 迁入 lib 的纯逻辑函数与重写工具函数单元测试
#![allow(non_snake_case)]


mod common;

use common::{assert_ops, eo, page_tree};
use lopdf::content::Operation;
use lopdf::{dictionary, Dictionary, Document, Object, StringFormat};
use pdf_crop_dual::{
    add_to_real, build_crop_content, build_form_stream, compute_cut, register_form_xobject,
    shift_path_op, update_page_boxes, val_to_obj, Val,
};

// ===================== compute_cut =====================

#[test]
fn compute_cut_spec小于最小空白() {
    let gaps = vec![Some((400.0, 550.0)), Some((441.3, 564.2))]; // 150, 122.9
    let (cut, min) = compute_cut(&gaps, 80.0).unwrap();
    assert_eq!(cut, 80.0);
    assert!((min - 122.9).abs() < 1e-3);
}

#[test]
fn compute_cut_spec大于最小空白时收敛() {
    let gaps = vec![Some((400.0, 550.0)), Some((441.3, 564.2))];
    let (cut, min) = compute_cut(&gaps, 200.0).unwrap();
    assert!((cut - 122.9).abs() < 1e-3);
    assert!((min - 122.9).abs() < 1e-3);
}

#[test]
fn compute_cut混入无空白页() {
    // gap=None 的页不参与收敛
    let gaps = vec![None, Some((400.0, 490.0))]; // 90
    let (cut, min) = compute_cut(&gaps, 100.0).unwrap();
    assert!((cut - 90.0).abs() < 1e-3);
    assert!((min - 90.0).abs() < 1e-3);
}

#[test]
fn compute_cut全为无空白页() {
    // 全部 gap=None：min = +inf，不收敛
    let gaps: Vec<Option<(f32, f32)>> = vec![None, None];
    let (cut, min) = compute_cut(&gaps, 100.0).unwrap();
    assert_eq!(cut, 100.0);
    assert!(min.is_infinite());
}

#[test]
fn compute_cut_spec过小报错() {
    let gaps: Vec<Option<(f32, f32)>> = vec![None, None];
    let e = compute_cut(&gaps, 0.5).unwrap_err();
    assert!(e.contains("空白宽度必须大于 1 pt"), "{e}");
    // 边界：cut == 1.0 仍须 Err（钉住 <= 而非 <）
    assert!(compute_cut(&gaps, 1.0).is_err());
}

#[test]
fn compute_cut最小空白过窄报错() {
    // 检测到空白 (100, 100.5) → min=0.5 → cut=min(spec,0.5) <= 1 → Err
    let gaps = vec![Some((100.0, 100.5))];
    let e = compute_cut(&gaps, 5.0).unwrap_err();
    assert!(e.contains("空白宽度必须大于 1 pt"), "{e}");
}

// ===================== build_form_stream =====================

#[test]
fn build_form_stream字典与流内容() {
    let res = Object::Dictionary(dictionary! { "Font" => dictionary! {} });
    let s = build_form_stream(0.0, 10.0, 1008.0, 661.5, &res, b"BT ET".to_vec());
    assert_eq!(s.dict.get(b"Type").unwrap(), &Object::Name(b"XObject".to_vec()));
    assert_eq!(s.dict.get(b"Subtype").unwrap(), &Object::Name(b"Form".to_vec()));
    assert_eq!(s.dict.get(b"FormType").unwrap(), &Object::Integer(1));
    let bbox = s.dict.get(b"BBox").unwrap().as_array().unwrap();
    let xs: Vec<f32> = bbox.iter().filter_map(|o| o.as_f32().ok()).collect();
    assert_eq!(xs, vec![0.0, 10.0, 1008.0, 661.5]);
    let r = s.dict.get(b"Resources").unwrap().as_dict().unwrap();
    assert!(r.has(b"Font"));
    assert_eq!(&s.content, &b"BT ET"[..]);
}

// ===================== build_crop_content =====================

#[test]
fn build_crop_content操作序列与顺序不变量() {
    let c = build_crop_content(0.0, 0.0, 1008.0, 661.5, 452.8, 100.0, b"FormX1");
    assert_ops(
        &c,
        &[
            eo("q", vec![]),
            eo("re", vec![0.0, 0.0, 452.8, 661.5]),
            eo("W", vec![]),
            eo("n", vec![]),
            ("Do", vec![], vec![], vec![b"FormX1".to_vec()]),
            eo("Q", vec![]),
            eo("q", vec![]),
            // 右裁剪区从 band_left 起，宽 x2-cut-band_left = 1008-100-452.8
            eo("re", vec![452.8, 0.0, 455.2, 661.5]),
            eo("W", vec![]),
            eo("n", vec![]),
            eo("cm", vec![1.0, 0.0, 0.0, 1.0, -100.0, 0.0]),
            ("Do", vec![], vec![], vec![b"FormX1".to_vec()]),
            eo("Q", vec![]),
        ],
        "传统方案内容流",
    );
    // 顺序不变量：右半裁剪（W）必须在 cm 之前定义（历史 bug 回归）
    let ops: Vec<&str> = c.operations.iter().map(|o| o.operator.as_str()).collect();
    let w_idx = ops.iter().rposition(|o| *o == "W").unwrap();
    let cm_idx = ops.iter().position(|o| *o == "cm").unwrap();
    assert!(w_idx < cm_idx, "clip 必须在 cm 之前定义");
}

// ===================== register_form_xobject =====================

#[test]
fn register_form_xobject引用情形合并() {
    let mut doc = Document::new();
    let existing_xo_id = doc.add_object(Object::Dictionary(dictionary! { "Type" => "XObject" }));
    let mut xo = dictionary! {};
    xo.set("Im1", Object::Reference(existing_xo_id));
    let res_id = doc.add_object(Object::Dictionary(dictionary! { "XObject" => xo }));
    let form_id = doc.add_object(Object::Dictionary(dictionary! { "Type" => "XObject" }));
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => Object::Reference(res_id) },
        Dictionary::new(),
    );
    let page_dict = doc.get_object(page_id).unwrap().as_dict().unwrap().clone();
    register_form_xobject(&mut doc, &page_dict, page_id, b"FormX1", form_id);
    // 页面 Resources 仍指向原对象
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(page.get(b"Resources").unwrap(), &Object::Reference(res_id));
    // 原条目不丢、新条目并入
    let res = doc.get_object(res_id).unwrap().as_dict().unwrap();
    let xo = res.get(b"XObject").unwrap().as_dict().unwrap();
    assert_eq!(xo.get(b"Im1").unwrap(), &Object::Reference(existing_xo_id));
    assert_eq!(xo.get(b"FormX1").unwrap(), &Object::Reference(form_id));
}

#[test]
fn register_form_xobject内联字典提取() {
    let mut doc = Document::new();
    let form_id = doc.add_object(Object::Dictionary(dictionary! { "Type" => "XObject" }));
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => dictionary! { "Font" => dictionary! {} } },
        Dictionary::new(),
    );
    let page_dict = doc.get_object(page_id).unwrap().as_dict().unwrap().clone();
    register_form_xobject(&mut doc, &page_dict, page_id, b"FormX1", form_id);
    // 页面 Resources 变为独立对象引用
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    let res_id = match page.get(b"Resources").unwrap() {
        Object::Reference(id) => *id,
        other => panic!("期望 Resources 为引用，实际: {other:?}"),
    };
    let res = doc.get_object(res_id).unwrap().as_dict().unwrap();
    assert!(res.has(b"Font"), "原内联字典内容应保留");
    let xo = res.get(b"XObject").unwrap().as_dict().unwrap();
    assert_eq!(xo.get(b"FormX1").unwrap(), &Object::Reference(form_id));
}

#[test]
fn register_form_xobject缺失时新建() {
    let mut doc = Document::new();
    let form_id = doc.add_object(Object::Dictionary(dictionary! { "Type" => "XObject" }));
    let (page_id, _) = page_tree(&mut doc, dictionary! { "Type" => "Page" }, Dictionary::new());
    let page_dict = doc.get_object(page_id).unwrap().as_dict().unwrap().clone();
    register_form_xobject(&mut doc, &page_dict, page_id, b"FormX1", form_id);
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    let res_id = match page.get(b"Resources").unwrap() {
        Object::Reference(id) => *id,
        other => panic!("期望 Resources 为引用，实际: {other:?}"),
    };
    let res = doc.get_object(res_id).unwrap().as_dict().unwrap();
    let xo = res.get(b"XObject").unwrap().as_dict().unwrap();
    assert_eq!(xo.get(b"FormX1").unwrap(), &Object::Reference(form_id));
}

// ===================== update_page_boxes =====================

#[test]
fn update_page_boxes替换并删除多余框() {
    let mut doc = Document::new();
    let content_id = doc.add_object(Object::Dictionary(Dictionary::new()));
    let mb: Vec<Object> = vec![0.0.into(), 0.0.into(), 1008.0.into(), 661.5.into()];
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! {
            "Type" => "Page",
            "MediaBox" => mb.clone(),
            "CropBox" => mb.clone(),
            "TrimBox" => mb.clone(),
            "BleedBox" => mb.clone(),
            "ArtBox" => mb,
        },
        Dictionary::new(),
    );
    update_page_boxes(&mut doc, page_id, content_id, 0.0, 0.0, 1008.0, 661.5, 100.0);
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(page.get(b"Contents").unwrap(), &Object::Reference(content_id));
    let xs = |key: &[u8]| -> Vec<f32> {
        page.get(key).unwrap().as_array().unwrap().iter().filter_map(|o| o.as_f32().ok()).collect()
    };
    assert_eq!(xs(b"MediaBox"), vec![0.0, 0.0, 908.0, 661.5]);
    assert_eq!(xs(b"CropBox"), vec![0.0, 0.0, 908.0, 661.5]);
    assert!(!page.has(b"TrimBox"));
    assert!(!page.has(b"BleedBox"));
    assert!(!page.has(b"ArtBox"));
}

#[test]
fn update_page_boxes无CropBox不新增() {
    let mut doc = Document::new();
    let content_id = doc.add_object(Object::Dictionary(Dictionary::new()));
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "MediaBox" => vec![0.0.into(), 0.0.into(), 1008.0.into(), 661.5.into()] },
        Dictionary::new(),
    );
    update_page_boxes(&mut doc, page_id, content_id, 0.0, 0.0, 1008.0, 661.5, 100.0);
    let page = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(!page.has(b"CropBox"));
    let xs: Vec<f32> = page.get(b"MediaBox").unwrap().as_array().unwrap().iter().filter_map(|o| o.as_f32().ok()).collect();
    assert_eq!(xs, vec![0.0, 0.0, 908.0, 661.5]);
}

// ===================== 重写工具函数 =====================

#[test]
fn val_to_obj转换() {
    assert_eq!(val_to_obj(&Val::Num(1.5)), Object::Real(1.5));
    assert_eq!(val_to_obj(&Val::Name(b"F1".to_vec())), Object::Name(b"F1".to_vec()));
    match val_to_obj(&Val::Str(b"AB".to_vec())) {
        Object::String(s, f) => {
            assert_eq!(s, b"AB");
            assert!(matches!(f, StringFormat::Literal));
        }
        other => panic!("期望 String，实际: {other:?}"),
    }
    match val_to_obj(&Val::Arr(vec![Val::Num(1.0), Val::Str(b"x".to_vec())])) {
        Object::Array(items) => {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Object::Real(1.0));
            assert_eq!(items[1], Object::String(b"x".to_vec(), StringFormat::Literal));
        }
        other => panic!("期望 Array，实际: {other:?}"),
    }
}

#[test]
fn add_to_real行为() {
    let mut op = Operation::new(
        "Tm",
        vec![1.0.into(), 0.0.into(), 0.0.into(), 1.0.into(), 100.0.into(), 500.0.into()],
    );
    add_to_real(&mut op, 4, -100.0);
    assert_eq!(op.operands[4], Object::Real(0.0));
    add_to_real(&mut op, 4, 0.0); // delta=0 不动
    assert_eq!(op.operands[4], Object::Real(0.0));
    let mut op2 = Operation::new("Do", vec![Object::Name(b"F1".to_vec())]);
    add_to_real(&mut op2, 0, 5.0); // 非 Real 操作数被忽略
    assert_eq!(op2.operands[0], Object::Name(b"F1".to_vec()));
}

#[test]
fn shift_path_op坐标平移() {
    let w = (-100.0, 0.0);
    let mut m = Operation::new("m", vec![10.0.into(), 20.0.into()]);
    shift_path_op(&mut m, w);
    assert_eq!(m.operands[0], Object::Real(-90.0));
    assert_eq!(m.operands[1], Object::Real(20.0));

    let mut re = Operation::new("re", vec![10.0.into(), 20.0.into(), 30.0.into(), 40.0.into()]);
    shift_path_op(&mut re, w);
    assert_eq!(re.operands[0], Object::Real(-90.0)); // x 平移
    assert_eq!(re.operands[1], Object::Real(20.0));
    assert_eq!(re.operands[2], Object::Real(30.0)); // w 不平移
    assert_eq!(re.operands[3], Object::Real(40.0));

    let mut c = Operation::new(
        "c",
        vec![0.0.into(), 0.0.into(), 1.0.into(), 1.0.into(), 2.0.into(), 2.0.into()],
    );
    shift_path_op(&mut c, w);
    assert_eq!(c.operands[0], Object::Real(-100.0));
    assert_eq!(c.operands[2], Object::Real(-99.0));
    assert_eq!(c.operands[4], Object::Real(-98.0));

    // y 分量非零行使：l 的 (0,1) 对 + c 的 y 索引 (1,3,5)
    let mut l = Operation::new("l", vec![5.0.into(), 5.0.into()]);
    shift_path_op(&mut l, (-3.0, 7.0));
    assert_eq!(l.operands[0], Object::Real(2.0));
    assert_eq!(l.operands[1], Object::Real(12.0)); // 钉住 y 分量 *v += w.1
    let mut c2 = Operation::new(
        "c",
        vec![0.0.into(), 0.0.into(), 1.0.into(), 1.0.into(), 2.0.into(), 2.0.into()],
    );
    shift_path_op(&mut c2, (-3.0, 7.0));
    assert_eq!(c2.operands[1], Object::Real(7.0));
    assert_eq!(c2.operands[3], Object::Real(8.0));
    assert_eq!(c2.operands[5], Object::Real(9.0));

    let mut d = Operation::new("Do", vec![10.0.into()]);
    shift_path_op(&mut d, w); // 未知操作符不变
    assert_eq!(d.operands[0], Object::Real(10.0));
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --test main_logic 2>&1 | tail -8`
Expected: `test result: ok. 16 passed; 0 failed`。

- [ ] **Step 3: Commit**

```bash
git add tests/main_logic.rs
git commit -m "test：迁入纯函数单元测试（compute_cut 收敛/错误路径、Form 流、clip 顺序不变量、页面框）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 11: tests/e2e.rs —— 纯 lopdf 结构断言（测试 1-5）

**Files:**
- Create: `tests/e2e.rs`（Task 12 追加 gs 断言测试 6-10）

**背景**：`test.pdf` 在仓库根目录（74 页，1008×661.5pt）。本任务验证 CLI 端到端行为与输出 PDF 结构（不用 gs）；像素/文本层断言在 Task 12。

- [ ] **Step 1: 写入 helpers + 测试 1-5**

```rust
//! 集成测试：CLI 端到端行为与输出 PDF 结构断言（纯 lopdf，无 gs 依赖部分）

mod common;

use common::{run_tool, tmp_file};
use lopdf::{Document, Dictionary, Object, ObjectId};
use pdf_crop_dual::{detect_gap, get_mediabox, page_resources_dict, Walk};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// ===================== helpers =====================

/// 仓库根目录的 test.pdf（测试夹具）；缺失时给出清晰错误
fn test_pdf() -> &'static Path {
    static PATH: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("test.pdf");
        assert!(p.exists(), "缺少测试夹具 test.pdf（应位于仓库根目录）");
        p
    })
}

/// 对文档逐页执行空白检测（口径与 main.rs scan_page 一致），返回每页 gap
fn page_gaps(doc: &Document) -> Vec<Option<(f32, f32)>> {
    doc.get_pages()
        .iter()
        .map(|(_, page_id)| {
            let page_dict = match doc.get_object(*page_id) {
                Ok(Object::Dictionary(d)) => d,
                _ => return None,
            };
            let mb = get_mediabox(doc, page_dict, *page_id).ok()?;
            let (x1, _y1, x2, _y2) = (mb[0], mb[1], mb[2], mb[3]);
            let res = page_resources_dict(doc, page_dict, *page_id)?;
            let content = doc.get_page_content(*page_id);
            let mut w = Walk::new(doc);
            w.walk(&content, Some(res), 0);
            detect_gap(&w.intervals, x1, x2)
        })
        .collect()
}

/// 页面 /Resources/XObject 键集（缺失时为空集；Resources 与 XObject 均支持内联字典与引用）
fn xobject_keys(doc: &Document, page_id: ObjectId) -> BTreeSet<Vec<u8>> {
    let page = match doc.get_object(page_id) {
        Ok(Object::Dictionary(d)) => d,
        _ => return BTreeSet::new(),
    };
    let res: &Dictionary = match page.get(b"Resources") {
        Ok(Object::Dictionary(d)) => d,
        Ok(Object::Reference(id)) => match doc.get_object(*id) {
            Ok(Object::Dictionary(d)) => d,
            _ => return BTreeSet::new(),
        },
        _ => return BTreeSet::new(),
    };
    let xo: &Dictionary = match res.get(b"XObject") {
        Ok(Object::Dictionary(d)) => d,
        Ok(Object::Reference(id)) => match doc.get_object(*id) {
            Ok(Object::Dictionary(d)) => d,
            _ => return BTreeSet::new(),
        },
        _ => return BTreeSet::new(),
    };
    xo.iter().map(|(k, _)| k.clone()).collect()
}

/// 回退页新 Contents 断言：恰好 2 个 Do /FormXn、1 个 cm（e=-cut）、右半 W 在 cm 之前
fn assert_fallback_content(doc: &Document, page_id: ObjectId, form_name: &[u8], cut: f32) {
    let content = doc.get_page_content(page_id);
    use pdf_crop_dual::{Item, Tok, Val};
    let mut tk = Tok {
        data: &content,
        pos: 0,
    };
    let mut dos = 0usize;
    let mut cms = 0usize;
    let mut cm_e: Option<f32> = None;
    let mut items = 0usize;
    let mut last_w = 0usize;
    let mut cm_pos = 0usize;
    let mut operands: Vec<Val> = Vec::new();
    loop {
        match tk.next_item() {
            Some(Item::Val(v)) => operands.push(v),
            Some(Item::Op(op)) => {
                items += 1;
                if op == "BI" {
                    operands.clear();
                    assert!(tk.handle_inline_image(), "意外的内联图像");
                } else {
                    if op == "Do" {
                        dos += 1;
                        assert!(
                            matches!(operands.last(), Some(Val::Name(n)) if n == form_name),
                            "Do 名称应为 /{}，实际: {operands:?}",
                            String::from_utf8_lossy(form_name)
                        );
                    }
                    if op == "cm" {
                        cms += 1;
                        cm_pos = items;
                        cm_e = operands.get(4).and_then(|v| match v {
                            Val::Num(n) => Some(*n),
                            _ => None,
                        });
                    }
                    if op == "W" {
                        last_w = items;
                    }
                    operands.clear();
                }
            }
            None => break,
        }
    }
    assert_eq!(dos, 2, "应恰好 2 个 Do /FormXn");
    assert_eq!(cms, 1, "应恰好 1 个 cm");
    assert_eq!(cm_e, Some(-cut), "cm 的 e 分量应为 -cut");
    assert!(last_w > 0 && last_w < cm_pos, "右半裁剪 W 必须在 cm 之前");
}

// ===================== 测试 1-5 =====================

#[test]
fn cli_参数错误() {
    let s = test_pdf().to_str().unwrap();
    let out_path = tmp_file("err.pdf");
    let out = out_path.to_str().unwrap();
    // 无参
    let (code, _stdout, stderr) = run_tool(&[]);
    assert_eq!(code, 1);
    assert!(stderr.contains("用法"), "stderr: {stderr}");
    // 仅 2 参
    let (code, _stdout, stderr) = run_tool(&[s, out]);
    assert_eq!(code, 1);
    assert!(stderr.contains("用法"), "stderr: {stderr}");
    // 宽度非数字（parse 失败 → panic 退出，非零码）
    let (code, _stdout, stderr) = run_tool(&[s, out, "abc"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("空白宽度必须是数字"), "stderr: {stderr}");
    // 输入文件不存在（加载失败 → panic 退出，非零码）
    let (code, _stdout, stderr) = run_tool(&["/nonexistent/no-such-file.pdf", out, "100"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("无法加载 PDF"), "stderr: {stderr}");
}

#[test]
fn cli_过宽收敛且超小宽度拒绝() {
    let s = test_pdf().to_str().unwrap();
    // spec=0.5 → cut=0.5 <= 1 → exit 1
    let out = tmp_file("out05.pdf");
    let (code, _stdout, stderr) = run_tool(&[s, out.to_str().unwrap(), "0.5"]);
    assert_eq!(code, 1);
    assert!(stderr.contains("空白宽度必须大于 1 pt"), "stderr: {stderr}");
}

#[test]
fn e2e_正常裁剪输出框() {
    let input = test_pdf();
    let out = tmp_file("out100.pdf");
    let (code, stdout, stderr) = run_tool(&[input.to_str().unwrap(), out.to_str().unwrap(), "100"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    // spec=100 < 最小空白 122.9pt → 不收敛（无提示），cut=100 → 新宽 908
    assert!(!stdout.contains("提示"), "不应触发收敛提示: {stdout}");
    let doc = Document::load(&out).expect("加载输出 PDF");
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 74);
    for (num, &page_id) in &pages {
        let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
        let mb = get_mediabox(&doc, d, page_id).unwrap();
        assert_eq!(mb, vec![0.0, 0.0, 908.0, 661.5], "第 {num} 页 MediaBox");
        // CropBox 夹具每页均存在，update_page_boxes 的「已存在则更新」分支真实行使
        let cb = d.get(b"CropBox").unwrap().as_array().unwrap();
        let cb: Vec<f32> = cb.iter().map(|o| o.as_float().unwrap()).collect();
        assert_eq!(cb, vec![0.0, 0.0, 908.0, 661.5], "第 {num} 页 CropBox");
    }
}

#[test]
fn e2e_超宽收敛到最小空白() {
    let input = test_pdf();
    let out = tmp_file("out500.pdf");
    let (code, stdout, stderr) = run_tool(&[input.to_str().unwrap(), out.to_str().unwrap(), "500"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("提示"), "stdout 应含收敛提示:\n{stdout}");
    // 测试内独立计算原始文件最小空白
    let doc = Document::load(input).expect("加载 test.pdf");
    let gaps = page_gaps(&doc);
    let min_gap = gaps
        .iter()
        .flatten()
        .map(|(l, r)| r - l)
        .fold(f32::INFINITY, f32::min);
    assert!(min_gap.is_finite(), "test.pdf 应存在可检测空白");
    let out_doc = Document::load(&out).expect("加载输出 PDF");
    let pages = out_doc.get_pages();
    assert_eq!(pages.len(), 74);
    for (num, &page_id) in &pages {
        let d = out_doc.get_object(page_id).unwrap().as_dict().unwrap();
        let mb = get_mediabox(&out_doc, d, page_id).unwrap();
        assert!(
            (mb[2] - (1008.0 - min_gap)).abs() < 0.01,
            "第 {num} 页宽 {:.3}，应为 {:.3}",
            mb[2],
            1008.0 - min_gap
        );
    }
}

#[test]
fn e2e_格式保留结构() {
    let input = test_pdf();
    let out = tmp_file("out100b.pdf");
    let (code, _stdout, stderr) = run_tool(&[input.to_str().unwrap(), out.to_str().unwrap(), "100"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let in_doc = Document::load(input).expect("加载 test.pdf");
    let out_doc = Document::load(&out).expect("加载输出 PDF");
    let in_pages = in_doc.get_pages();
    let out_pages = out_doc.get_pages();
    assert_eq!(in_pages.len(), 74);
    assert_eq!(in_pages.len(), out_pages.len());
    let gaps = page_gaps(&in_doc);
    let mut n_gap = 0usize;
    let mut n_fb = 0usize;
    for (num, &in_id) in &in_pages {
        let out_id = out_pages[num];
        if gaps[(num - 1) as usize].is_some() {
            // gap 页：格式保留式重写 → 不注册新 XObject，键集不变
            n_gap += 1;
            assert_eq!(
                xobject_keys(&in_doc, in_id),
                xobject_keys(&out_doc, out_id),
                "第 {num} 页 XObject 键集变化（期望格式保留路径）"
            );
        } else {
            // 回退页：原键集 ∪ {FormXn}
            n_fb += 1;
            let in_keys = xobject_keys(&in_doc, in_id);
            let out_keys = xobject_keys(&out_doc, out_id);
            assert!(
                out_keys.len() == in_keys.len() + 1,
                "第 {num} 页: {} → {}",
                in_keys.len(),
                out_keys.len()
            );
            let extra: Vec<&Vec<u8>> = out_keys.difference(&in_keys).collect();
            assert_eq!(extra.len(), 1);
            assert!(extra[0].starts_with(b"FormX"), "意外新增条目: {:?}", extra[0]);
            assert_fallback_content(&out_doc, out_id, extra[0], 100.0);
        }
    }
    // test.pdf 基线：71 个 gap 页 + 3 个回退页（第 30/48/74 页）
    assert_eq!(n_gap, 71);
    assert_eq!(n_fb, 3);
    let fb: Vec<u32> = (1..=74).filter(|n| gaps[(n - 1) as usize].is_none()).collect();
    assert_eq!(fb, vec![30, 48, 74]);
}
```

- [ ] **Step 2: 运行测试**

Run: `cargo test --test e2e 2>&1 | tail -10`
Expected: `test result: ok. 5 passed; 0 failed`（每个测试完整跑一遍 74 页 CLI，合计约 10~20s）。

注意：`cli_参数错误` 中两个 `assert_ne!(code, 0)` 对应 `expect` panic 路径（退出码 101），不是 `exit(1)`——断言只要求非零。

- [ ] **Step 3: Commit**

```bash
git add tests/e2e.rs
git commit -m "test：e2e 集成测试（CLI 参数/收敛行为/输出框/格式保留结构断言）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 12: tests/e2e.rs —— gs 断言（测试 6-10）

**Files:**
- Modify: `tests/e2e.rs`（Task 11 已创建；本任务扩充 imports 并追加 5 个 gs 依赖测试）

**背景**：gs 72dpi 渲染 1px=1pt。核心断言「输出 == 原始页手工裁剪」：spec=100 时 cut 恰为 100pt 整数，空白带内无墨迹且两侧墨迹离 band_left ≥2pt，故逐像素严格成立。gs 缺失时 `gs_available()` 为 false，测试打印说明后直接 return（不失败）。

- [ ] **Step 1: 扩充 imports**

将 Task 11 的 imports 行：

```rust
use common::{run_tool, tmp_file};
use lopdf::{Document, Dictionary, Object, ObjectId};
```

替换为（`dictionary`/`Stream` 供回退 2× 合成文档使用）：

```rust
use common::{
    count_text_codepoints, gs_available, page_resources, page_tree, render_pgm_page,
    render_txt_file, render_txt_page, run_tool, tmp_file, type1_font,
};
use lopdf::{dictionary, Document, Dictionary, Object, ObjectId, Stream};
```

- [ ] **Step 2: 追加 gs 依赖测试 6-10**

在文件末尾追加：

```rust
// ===================== gs 依赖测试（gs 缺失时跳过） =====================

#[test]
fn e2e_文本层1倍() {
    if !gs_available() {
        eprintln!("跳过 e2e_文本层：系统未安装 gs（ghostscript）");
        return;
    }
    let input = test_pdf();
    let out = tmp_file("out100txt.pdf");
    let (code, _stdout, stderr) = run_tool(&[input.to_str().unwrap(), out.to_str().unwrap(), "100"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    // 总量 1×：每页内容在输出中恰好出现一次（gs 10.x txtwrite 无页分隔符，
    // 故按总量断言；空白回退页贡献 0 码点，其 2× 不改变总量）
    let o_cp = count_text_codepoints(&render_txt_file(input));
    let n_cp = count_text_codepoints(&render_txt_file(&out));
    assert_eq!(n_cp, o_cp, "输出文本层总量应 1×：{o_cp} → {n_cp}");
    // 代表 gap 页逐页定位（页 1 纯双栏、页 6 语法高亮+路径装饰、页 46 MP 标记）
    for page in [1u32, 6, 46] {
        let o_cp = count_text_codepoints(&render_txt_page(input, page));
        let n_cp = count_text_codepoints(&render_txt_page(&out, page));
        assert_eq!(n_cp, o_cp, "第 {page} 页（gap）文本层应 1×：{o_cp} → {n_cp}");
    }
}

/// 构造 1 页回退方案 PDF：内容横跨页面中线 → 无清晰空白 → 传统 clip 方案（文本层 2×）
fn span_fallback_pdf() -> PathBuf {
    let mut doc = Document::new();
    // A-D @10pt = 10pt/字符；"ABCD" 宽 40pt，置于 x∈[80,120]，横跨中线 100
    let f = type1_font(&mut doc, 65, &[1000.0; 4]);
    let res = page_resources(&[(b"F1", f)], &[]);
    let res_id = doc.add_object(Object::Dictionary(res));
    let content_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 80 50 Td (ABCD) Tj ET".to_vec(),
    )));
    let pd = dictionary! {
        "Type" => "Page",
        "MediaBox" => Object::Array(vec![
            Object::Real(0.0),
            Object::Real(0.0),
            Object::Real(200.0),
            Object::Real(100.0),
        ]),
        "Resources" => Object::Reference(res_id),
        "Contents" => Object::Reference(content_id),
    };
    let (_page_id, parent_id) = page_tree(&mut doc, pd, Dictionary::new());
    // lopdf 无 set_pages：页树经 trailer Root → catalog Pages 定位
    let catalog_id = doc.add_object(Object::Dictionary(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(parent_id),
    }));
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let p = tmp_file("span-fallback.pdf");
    doc.save(&p).expect("保存合成 PDF 失败");
    p
}

#[test]
fn e2e_回退页文本层2倍() {
    if !gs_available() {
        eprintln!("跳过 e2e_回退2倍：系统未安装 gs（ghostscript）");
        return;
    }
    let input = span_fallback_pdf();
    let out = tmp_file("span-out.pdf");
    let (code, _stdout, stderr) = run_tool(&[input.to_str().unwrap(), out.to_str().unwrap(), "20"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    // 回退方案整页绘制两次（左右 clip），文本层 2×
    let o_cp = count_text_codepoints(&render_txt_file(&input));
    let n_cp = count_text_codepoints(&render_txt_file(&out));
    assert_eq!(o_cp, 4, "合成页原始应 4 码点");
    assert_eq!(n_cp, 2 * o_cp, "回退页文本层应 2×：{o_cp} → {n_cp}");
}

#[test]
fn e2e_像素手工裁剪全等() {
    if !gs_available() {
        eprintln!("跳过 e2e_像素手工裁剪：系统未安装 gs（ghostscript）");
        return;
    }
    let input = test_pdf();
    let out = tmp_file("out100px.pdf");
    let (code, _stdout, stderr) = run_tool(&[input.to_str().unwrap(), out.to_str().unwrap(), "100"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let gaps = page_gaps(&Document::load(input).expect("加载 test.pdf"));
    const CUT: usize = 100;
    // 代表页：1（纯双栏）、6/9（语法高亮+路径装饰+标记内容）、46（MP）、30/48/74（空白回退页）
    for page in [1u32, 6, 9, 46, 30, 48, 74] {
        let i = (page - 1) as usize;
        // 移除带位置：与 main.rs rebuild_page 同公式
        let band_left = if let Some((l, r)) = gaps[i] {
            (l + r) / 2.0 - CUT as f32 / 2.0
        } else {
            (1008.0 - CUT as f32) / 2.0
        };
        let left_cols = band_left.ceil() as usize;
        let o = render_pgm_page(input, page);
        let n = render_pgm_page(&out, page);
        assert_eq!(n.width, 1008 - CUT, "第 {page} 页输出宽");
        assert_eq!(n, o.manual_crop(left_cols, CUT), "第 {page} 页应逐像素等于原始页手工裁剪");
    }
}

#[test]
fn e2e_墨迹范围左移() {
    if !gs_available() {
        eprintln!("跳过 e2e_墨迹范围：系统未安装 gs（ghostscript）");
        return;
    }
    let input = test_pdf();
    let out = tmp_file("out100dx.pdf");
    let (code, _stdout, stderr) = run_tool(&[input.to_str().unwrap(), out.to_str().unwrap(), "100"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    for page in [1u32, 6, 9, 46, 30, 48, 74] {
        let o = render_pgm_page(input, page);
        let n = render_pgm_page(&out, page);
        assert_eq!(n.width, 908, "第 {page} 页输出宽");
        match (o.dark_cols(), n.dark_cols()) {
            (Some((omn, omx)), Some((nmin, nmax))) => {
                assert_eq!(omn, nmin, "第 {page} 页左缘应不变");
                assert_eq!(omx, nmax + 100, "第 {page} 页右缘应左移 100px");
            }
            (None, None) => {} // 无墨迹页（空白回退页）
            (a, b) => panic!("第 {page} 页墨迹缺失: 原始 {a:?} 输出 {b:?}"),
        }
    }
}

#[test]
#[ignore = "全 74 页逐像素深检较慢（约 2~5 分钟），手动运行: cargo test --test e2e -- --ignored"]
fn e2e_全部页像素全等() {
    if !gs_available() {
        eprintln!("跳过 e2e_全部页像素全等：系统未安装 gs（ghostscript）");
        return;
    }
    let input = test_pdf();
    let out = tmp_file("out100all.pdf");
    let (code, _stdout, stderr) = run_tool(&[input.to_str().unwrap(), out.to_str().unwrap(), "100"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let gaps = page_gaps(&Document::load(input).expect("加载 test.pdf"));
    const CUT: usize = 100;
    for page in 1..=74u32 {
        let i = (page - 1) as usize;
        let band_left = if let Some((l, r)) = gaps[i] {
            (l + r) / 2.0 - CUT as f32 / 2.0
        } else {
            (1008.0 - CUT as f32) / 2.0
        };
        let left_cols = band_left.ceil() as usize;
        let o = render_pgm_page(input, page);
        let n = render_pgm_page(&out, page);
        assert_eq!(n, o.manual_crop(left_cols, CUT), "第 {page} 页像素应全等于手工裁剪");
    }
}
```

**若 `e2e_像素手工裁剪全等` 出现系统性 1px 偏移**（个别页在 band 边界列差 1px）：按设计文档 §6.7 降级——将该断言改为「暗像素级全等（dark_cols 一致）+ 总差异像素占比 < 0.01%」，并在设计文档 §6.7 记录实测降级原因。其余测试不变。

- [ ] **Step 3: 运行测试**

Run: `cargo test --test e2e 2>&1 | tail -12`
Expected: `test result: ok. 9 passed; 0 failed; 1 ignored`（e2e 文件共 10 个测试：Task 11 的 5 个 + Task 12 的 5 个，其中 1 个 `#[ignore]`；gs 测试首次运行约 1~3 分钟）。

- [ ] **Step 4: Commit**

```bash
git add tests/e2e.rs
git commit -m "test：e2e gs 断言（文本层 1×/2×、像素手工裁剪全等、墨迹范围左移、全页深检 ignored）

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

---

### Task 13: 全量验证 + 文档更新 + 收尾

**Files:**
- Modify: `CLAUDE.md`
- Commit: `docs/superpowers/plans/2026-09-12-test-suite.md`（计划文档本身）

- [ ] **Step 1: 全量测试**

Run: `cargo test 2>&1 | grep -E "^test result|running" | tail -20`
Expected：所有 test binary 均 `test result: ok. N passed; 0 failed`（e2e 为 `9 passed; 1 ignored`，其文件共定义 10 个测试），合计 157 个测试（12+16+15+9+7+41+31+16+10），总耗时约 1~3 分钟。任何失败：先修测试（断言口径错）或修 src（真 bug），再重跑。

- [ ] **Step 2: 深检（全 74 页逐像素）**

Run: `cargo test -- --ignored 2>&1 | tail -6`
Expected：`test result: ok. 1 passed`（`e2e_全部页像素全等`），约 2~5 分钟。

- [ ] **Step 3: 更新 CLAUDE.md 第一处**

将：

```markdown
- 没有测试框架，修改后的验证方法见下节
```

替换为：

```markdown
- 测试套件见「测试与验证」节（tests/ 目录，`cargo test` 一键运行）
```

- [ ] **Step 4: 更新 CLAUDE.md 第二节**

将下面这段旧文本（`## 输出验证方法（像素级 + 文本层）` 整节，从标题到「系统只有 gs，没有 pdftoppm」一条）：

````markdown
## 输出验证方法（像素级 + 文本层）

项目无单元测试。验证流程：

```bash
# 72dpi 渲染使 1px=1pt；pgmraw 输出 P5 原始格式，纯 Python 可直接解析（系统无 PIL/numpy）
# 注意 PGM 头含 # 注释行，解析要逐 token 跳过
gs -dNOPAUSE -dBATCH -sDEVICE=pgmraw -r72 -sOutputFile=/tmp/x-%d.pgm <file.pdf>
```

- **像素比对**：逐页比较暗像素（灰度 < 200）的 min/max——左栏范围应不变，右栏应整体左移 cut、左右缘不丢 1pt 以上。验证「重写方案渲染 == 传统方案渲染」时，新旧两个二进制的输出须**逐像素完全相同**（空白带内无墨迹、两侧墨迹离 band_left ≥2pt 无抗锯齿交互，任何 1px 差异即 bug）
- **文本层 1× 检查**（验证内容未重复、格式保留有效）：
  ```bash
  gs -dNOPAUSE -dBATCH -sDEVICE=txtwrite -sOutputFile=/tmp/x.txt <file.pdf>
  ```
  纯 Python 过滤 `[ \t\r\n\x00-\x1f]` 后数 UTF-8 码点，输出总数应等于原始 PDF（恰好 1×；传统方案的输出是 2×）。逐页比对（`gs -dFirstPage=N -dLastPage=N`）可定位问题页
- 系统只有 gs，没有 pdftoppm
````

替换为：

````markdown
## 测试与验证

测试全部位于 `tests/` 目录（src/ 内不含测试代码；测试所需的 lib 私有项已加 `pub`，main.rs 的 5 个纯函数已迁入 lib）：

```bash
cargo test                  # 全量：单元测试 + 集成测试（约 1~3 分钟）
cargo test -- --ignored     # 追加 e2e 全 74 页逐像素深检（约 2~5 分钟）
```

- 单元测试：`geometry.rs`（矩阵/裁剪）、`lexer.rs`（词法器）、`fonts.rs`（字体宽度与 advance）、`object_access.rs`（MediaBox/Resources 解析）、`detect_gap.rs`（空白判定）、`walk.rs`（Walk 墨迹区间口径）、`rewrite.rs`（重写器分类/移位/回退）、`main_logic.rs`（收敛算法/传统方案内容流/页面框）
- 集成测试 `e2e.rs`：CLI 参数与收敛行为、输出页框、格式保留结构（gap 页不新增 XObject、回退页 2 个 Do + 裁剪在 cm 之前）、gs 文本层 1×/2×、gs 像素级「输出 == 原始页手工裁剪」全等；gs 缺失时 gs 依赖项自动跳过
- 手工验证（调试用）：`gs -dNOPAUSE -dBATCH -sDEVICE=pgmraw -r72 -sOutputFile=/tmp/x-%d.pgm <file.pdf>`（72dpi 使 1px=1pt；PGM P5 头含 # 注释行需逐 token 跳过）；文本层用 `-sDEVICE=txtwrite`，过滤 `[ \t\r\n\x00-\x1f]` 后数码点。系统只有 gs，没有 pdftoppm
````

- [ ] **Step 5: 最终提交**（计划文档已在计划定稿时提交，此处仅提交 CLAUDE.md）

```bash
git add CLAUDE.md
git commit -m "docs：CLAUDE.md 更新为测试套件说明

Co-Authored-By: Claude Code <noreply@anthropic.com>"
```

- [ ] **Step 6: 收尾确认**

Run: `git log --oneline -12`
Expected：Task 0 基线无需提交（/tmp 产物）；自 Task 1 起每任务一个提交，共 12 个左右提交，工作区干净（`git status` 无未跟踪文件）。

