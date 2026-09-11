# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目简介

pdf-crop-dual 是 Rust 命令行工具：裁剪「双栏」PDF（左栏原文、右栏译文）中间的空白带，将右栏左移使两栏相接，输出宽度变窄的新 PDF。检测到空白的页面采用「格式保留式」重写：直接改写页面级内容流，左侧墨迹原样、右侧墨迹物理左移 cut，原有 Form XObject 结构不动、内容只存在一份（文本层不翻倍，PDF 编辑工具中段落不被拆块）；不可重写时按页回退传统方案（整页封装新 Form + clip 绘制两次）。代码分两部分：`src/lib.rs`（PDF 内容分析：对象访问、矩阵几何、字体宽度、内容流词法器与 `Walk` 遍历、`detect_gap`、`rewrite_page` 重写器）与 `src/main.rs`（CLI 与两遍算法、页面重建），唯一依赖 lopdf 0.45。代码注释和输出信息使用中文，修改时保持一致。

## 构建与运行

```bash
cargo build
cargo run -- <输入.pdf> <输出.pdf> <中间空白宽度(pt)>
```

- `test.pdf` 是测试文件：74 页，页面 1008×661.5pt，每页由两个 504pt 的 Form XObject 组成（左栏英文、右栏中文，右栏经 `Matrix [1 0 0 1 504 0]` 定位）；第 30/48/74 页为空白页（无空白检测→传统方案）；左栏内容约 [72, 432]，右栏约 [566/576, 935–944]，真实空白带约 123–140pt
- 部分页的**页面级**内容流不止两个 `Do`，还含直接绘制的内容（重写器必须全部支持，见「第二遍」节）：语法高亮代码文本块（`Tf/Td/Tm/Tj` 混用 + 块内 `rg`/`g` 颜色操作、第 6/9 页块内还有 `i` 平坦度）、小路径装饰（圆形/数字字形，`m/l/c/h/f/S` + 嵌套 `q cm … cm … Q`）、`/TouchUp_TextEdit MP` 标记内容（第 6/9/46 页）
- 没有测试框架，修改后的验证方法见下节

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

## 架构（src/main.rs 约 380 行 + src/lib.rs 约 2100 行）

`main.rs` 的 `main()` 编排两遍算法：`parse_args` → 逐页 `scan_page`（第一遍）→ `compute_cut` → 逐页 `rebuild_page`（第二遍）→ 压缩保存。内容分析机制（`Walk`、词法器、字体宽度、`detect_gap`、重写器、对象访问助手）在 `lib.rs`，main 仅导入 `Walk`/`detect_gap`/`get_mediabox`/`get_resources`/`page_resources_dict`/`rewrite_page`。

**第一遍：空白检测（`scan_page`）**
- 每页取合并内容流（`doc.get_page_content`）与 Resources，`Walk` 遍历内容流收集墨迹 x 区间
- `Walk` 跟踪：q/Q 图形状态栈、CTM（`cm` 与 Form Matrix）、文本状态（BT/ET、Tm/Td/TD/T*、TL/Tf/Tw/Tc/Tz）、路径坐标（m/l/c/v/y，绘制操作 S/f/… 时汇总为区间，W/n 丢弃）、`Do`（Form XObject 递归进入并应用 BBox 裁剪；Image XObject 按单位正方形）。操作分派在 `Walk::exec`，按类别分为 `exec_state`/`exec_path`/`exec_text`（文本再按定位/显示分派到 `exec_text_move`/`exec_text_show`）；Form 递归在 `walk_form`
- 文本宽度计算：Type1 用 `FirstChar`+`Widths`；CID(Type0) 用 DescendantFonts 下 CIDFont 的 `W` 数组+`DW`（支持 `[first w]`、`[first last w]`、`[first [w1...]]` 三种形式）；无宽度信息时回退 1em（过估是安全方向）。advance 计算（`str_advance`/`tj_advance`）与字体解析（`resolve_font_id`）是 `Walk` 与重写器共用的自由函数（口径单点实现），两侧只保留同名薄封装
- `detect_gap`：页面中线左侧区间的最大右缘 = 空白左界，右侧区间的最小左缘 = 空白右界；有内容横跨中线或空隙 < 10pt 判为无清晰空白；两侧各留 2pt 安全余量
- 实际移除宽度 `cut = min(用户指定宽度, 所有页最小空白宽)`，保证所有输出页宽度一致

**第二遍：内容重建（`rebuild_page`，双路径）**

*路径 A（优先，`gap` 为 `Some` 时先尝试）：`rewrite_page` 格式保留式重写*
- 逐操作按**设备空间 x 范围**分类：右缘 < band_left → 原样；左缘 > band_left + cut → 整体左移 cut；跨越移除带 → 整页回退路径 B
- **Do（Form）**：临时 `Walk` 子遍测其真实墨迹范围（`w.ctm = form矩阵·ctm`，文本状态继承，口径与空白检测一致；空墨迹原样通过），全右则包 `q cm(1,0,0,1,wx,wy) Do Q`，**Form 内部流不动**（格式保留的关键）；Image 按单位正方形四角·(Matrix·ctm)
- **文本块（BT..ET）**：缓冲整块，逐显示操作（Tj/TJ/'/"）校验范围不跨带；`Tm` 的 e/f 加 CTM 逆修正（文本原点 (e,f) 不受 Tm 线性部分影响）；`Td/TD` 的 δ 加 `(s_t - s_p)` 文本空间修正（偏移经行矩阵线性部分缩放）；`T*`/`'`/`"` 无/少操作数，**分解改写**为 `Td(δx, -TL+δy)`（`"` 先补发 `Tw`/`Tc`）；无 Tm/Td 的右侧块由 `ensure_block_position` 合成 Tm 携带移位
- **路径**：按子路径累积（`m`/`re` 起新子路径），终结操作（S/f/…/W/n）时逐子路径分类，右侧子路径的构造坐标加操作空间平移 `(-cut·ctm.d/det, cut·ctm.b/det)`；`h` 不增点
- 文本块内允许颜色/线条外观/平坦度/渲染模式/标记内容操作（`g/rg/i/Tr/Ts/MP/…`）原样入缓冲；**ET 必须发射**（历史 bug：漏发 ET 使 gs 把后续路径操作当作文本对象内操作而丢弃绘制）
- 回退触发：内联图像 BI、未知操作（含带字典操作数的 BDC/DP/sh，词法器无法重发字典）、块内 cm/q/Q/BT、任何范围跨带、退化矩阵、BT 未闭合
- 重写成功时不创建/注册任何 Form XObject，直接以重写结果为新 `Contents`

*路径 B（回退）：传统 clip 方案*
- `build_form_stream` 将原页内容流封装为新 Form XObject（BBox = 原页面框，Resources 从页复制），`register_form_xobject` 注册进页面 `/Resources /XObject`
- `build_crop_content` 构建新内容流：左半 `clip [x1, band_left]` + Do；右半 `clip [band_left, x2-cut]` + `cm(-cut)` + Do
- 输出内容存在两份（clip 隐藏一半），文本层 2×——仅作回退，检测到空白的页面应尽量走路径 A

两条路径共用 `update_page_boxes`：更新页面 `Contents`/`MediaBox`/`CropBox`（新宽 `x2-cut`），删除 `TrimBox`/`BleedBox`/`ArtBox`，最后 `doc.compress()` + `save`。移除带位置：检测到空白时居中于检测到的空白带；未检测到时退回页面对称（以页面中心为心）。

## 关键不变量（历史 bug 根因，改动时务必保持）

- **裁剪矩形必须在 cm 之前定义**（路径 B）：clip 区域以执行 W 时的坐标系为准，若先写 cm 平移，裁剪区会随内容左移 cut，导致右栏右缘每页被切掉 cut 宽度（本工具最初的 bug）
- **空白比指定值窄时必须收敛**（`cut = min(...)`），否则移除带右缘伸入右栏切掉内容
- `Mat` 用行向量约定 `p·M`；`mul(m1, m2)` 表示先应用 m1 再应用 m2；`cm` → `ctm = mul(m, ctm)`；文本→设备坐标 `ptm = mul(tlm, ctm)`
- **页面内容只允许存在一份**：检测到空白的页面输出不得把原内容封装 Form 再绘制两次（clip 方案的 2× 文本层会让 PDF 编辑工具把段落拆成独立文本块——本工具要解决的核心症状）
- **重写器分类口径必须与 `Walk` 一致**：Form 墨迹范围用 `Walk` 子遍量测、文本 advance 用同一套字体宽度逻辑，否则分类与 `detect_gap` 的「带内无墨迹」保证脱节
- **Td 偏移在文本空间**（经行矩阵线性部分缩放，`Walk` 与重写器同口径）；而 **Tm 的 (e,f) 原点不受 Tm 线性部分影响**，其移位修正只取 CTM 逆 `(s·ctm.d/det, -s·ctm.b/det)`，Td 的修正才用 ptm 逆
- **ET 必须发射**：漏发 ET 后 gs 会丢弃后续路径绘制（实测丢墨迹）

## lopdf 0.45 API 陷阱

- `Object::as_f32()` 只接受 Real；兼容 Integer 用 `as_float()`（MediaBox 等常为 Integer）
- `Error::ObjectNotFound` 是元组变体，需传 ObjectId：`Error::ObjectNotFound(page_id)`
- Stream 对象是 `Object::Stream(Stream)`，`as_dict()` 对它失败；取字典用 `obj_dict()` 辅助函数（Stream 有公有字段 `dict`）——Form XObject 全是 Stream，忘记这点会导致内容扫描在第一个 Do 处静默中止
- lopdf 不提供内容流解析器（只有编码器），`Tok`/`Val`/`Item`/`Parsed` 词法器与 `Walk` 遍历均为本项目自写
- `Document::get_pages()` 返回 `BTreeMap<u32, ObjectId>`（页码 → 对象 ID）
- `Document::get_page_content()` 直接返回 `Vec<u8>`（0.44 起不再是 `Result`）；某内容流解压失败时会静默回退为原始压缩字节，拿到的不一定是解码结果
- `Dictionary::get()` 返回 `Result<&Object>`，模式匹配时除 `Ok(Dictionary)`/`Ok(Reference)` 外还有其他 Ok 变体，需要通配臂
- test.pdf 是 PDF 1.7 并使用压缩对象流（ObjStm）：无法用原始字节搜索对象，只能经 lopdf 的 `get_object` 访问；调试对象结构可临时在 `examples/` 下写 example 运行
