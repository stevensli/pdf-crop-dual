# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目简介

pdf-crop-dual 是 Rust 命令行工具：裁剪「双栏」PDF（左栏原文、右栏译文）中间的空白带，将右栏左移使两栏相接，输出宽度变窄的新 PDF。全部代码在 `src/main.rs`（单文件二进制，唯一依赖 lopdf 0.36）。代码注释和输出信息使用中文，修改时保持一致。

## 构建与运行

```bash
cargo build
cargo run -- <输入.pdf> <输出.pdf> <中间空白宽度(pt)>
```

- `test.pdf` 是测试文件：74 页，页面 1008×661.5pt，每页由两个 504pt 的 Form XObject 组成（左栏英文、右栏中文，右栏经 `Matrix [1 0 0 1 504 0]` 定位）；第 30/48/74 页为空白页；左栏内容约 [72, 432]，右栏约 [566/576, 935–944]，真实空白带约 123–140pt
- 没有测试框架，修改后的验证方法见下节

## 输出验证方法（像素级比对）

项目无单元测试。验证「输出无内容丢失」的流程：

```bash
# 72dpi 渲染使 1px=1pt；pgmraw 输出 P5 原始格式，纯 Python 可直接解析（系统无 PIL/numpy）
gs -dNOPAUSE -dBATCH -sDEVICE=pgmraw -r72 -sOutputFile=/tmp/x-%d.pgm <file.pdf>
```

逐页比较暗像素列（灰度 < 200）的 min/max：左栏范围应不变，右栏应整体左移 cut、左右缘不丢 1pt 以上。系统只有 gs，没有 pdftoppm。

## 架构（src/main.rs，约 1500 行）

`main()` 实现两遍算法：

**第一遍：空白检测**
- 每页取合并内容流（`doc.get_page_content`）与 Resources，`Walk` 遍历内容流收集墨迹 x 区间
- `Walk` 跟踪：q/Q 图形状态栈、CTM（`cm` 与 Form Matrix）、文本状态（BT/ET、Tm/Td/TD/T*、TL/Tf/Tw/Tc/Tz）、路径坐标（m/l/c/v/y，绘制操作 S/f/… 时汇总为区间，W/n 丢弃）、`Do`（Form XObject 递归进入并应用 BBox 裁剪；Image XObject 按单位正方形）
- 文本宽度计算：Type1 用 `FirstChar`+`Widths`；CID(Type0) 用 DescendantFonts 下 CIDFont 的 `W` 数组+`DW`（支持 `[first w]`、`[first last w]`、`[first [w1...]]` 三种形式）；无宽度信息时回退 1em（过估是安全方向）
- `detect_gap`：页面中线左侧区间的最大右缘 = 空白左界，右侧区间的最小左缘 = 空白右界；有内容横跨中线或空隙 < 10pt 判为无清晰空白；两侧各留 2pt 安全余量
- 实际移除宽度 `cut = min(用户指定宽度, 所有页最小空白宽)`，保证所有输出页宽度一致

**第二遍：内容重建**
- 原页内容流封装为新 Form XObject（BBox = 原页面框，Resources 从页复制），注册进页面 `/Resources /XObject`
- 新内容流：左半 `clip [x1, band_left]` + Do；右半 `clip [band_left, x2-cut]` + `cm(-cut)` + Do
- 更新页面 `Contents`/`MediaBox`/`CropBox`（新宽 `x2-cut`），删除 `TrimBox`/`BleedBox`/`ArtBox`，最后 `doc.compress()` + `save`
- 移除带位置：检测到空白时居中于检测到的空白带；未检测到时退回页面对称（以页面中心为心）

## 关键不变量（历史 bug 根因，改动时务必保持）

- **裁剪矩形必须在 cm 之前定义**：clip 区域以执行 W 时的坐标系为准，若先写 cm 平移，裁剪区会随内容左移 cut，导致右栏右缘每页被切掉 cut 宽度（本工具最初的 bug）
- **空白比指定值窄时必须收敛**（`cut = min(...)`），否则移除带右缘伸入右栏切掉内容
- `Mat` 用行向量约定 `p·M`；`mul(m1, m2)` 表示先应用 m1 再应用 m2；`cm` → `ctm = mul(m, ctm)`；文本→设备坐标 `ptm = mul(tlm, ctm)`

## lopdf 0.36 API 陷阱

- `Object::as_f32()` 只接受 Real；兼容 Integer 用 `as_float()`（MediaBox 等常为 Integer）
- `Error::ObjectNotFound` 是元组变体，需传 ObjectId：`Error::ObjectNotFound(page_id)`
- Stream 对象是 `Object::Stream(Stream)`，`as_dict()` 对它失败；取字典用 `obj_dict()` 辅助函数（Stream 有公有字段 `dict`）——Form XObject 全是 Stream，忘记这点会导致内容扫描在第一个 Do 处静默中止
- lopdf 不提供内容流解析器（只有编码器），`Tok`/`Val`/`Item`/`Parsed` 词法器与 `Walk` 遍历均为本项目自写
- `Document::get_pages()` 返回 `BTreeMap<u32, ObjectId>`（页码 → 对象 ID）
- `Dictionary::get()` 返回 `Result<&Object>`，模式匹配时除 `Ok(Dictionary)`/`Ok(Reference)` 外还有其他 Ok 变体，需要通配臂
- test.pdf 是 PDF 1.7 并使用压缩对象流（ObjStm）：无法用原始字节搜索对象，只能经 lopdf 的 `get_object` 访问；调试对象结构可临时在 `examples/` 下写 example 运行
