# pdf-crop-dual

裁剪「双栏 PDF」中间空白的命令行工具 / A command-line tool that crops the middle gap out of two-column PDFs.

[中文](#中文) · [English](#english)

---

## 中文

### 背景

[pdfmathtranslate-next](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next) 等翻译工具输出的中英文对照 PDF 默认是左右双栏布局：左栏英文原文、右栏中文译文，两栏之间隔着一条过宽的空白带，页面显得稀疏，窄屏阅读时尤为明显。这个工具把这条空白带收窄，让两栏相接、页面变窄。

```
裁剪前:  [英文原文]      ····  宽空白带  ····      [中文译文]
裁剪后:  [英文原文][中文译文]          （页面宽度 = 原宽 − 移除宽度）
```

### 特性

- **自动检测每页真实空白位置**：解析 PDF 内容流（递归进入 Form XObject、按字体度量计算文本推进宽度、收集路径坐标），而不是按页面宽度盲目裁剪
- **格式保留式裁剪（主路径）**：对检测到空白的页面直接重写页面级内容流——左栏墨迹原样、右栏墨迹物理左移，原有 Form XObject 结构不动，内容在输出中**只存在一份**：文本层不翻倍，PDF 编辑工具中段落文本不会被拆成独立文本块
- **移除宽度安全收敛**：实际移除宽度 `cut = min(指定宽度, 所有页最小空白宽)`，指定值过大时自动收敛，不会切进栏目内容
- **逐页回退**：无法安全重写的页面自动回退传统 clip 方案（整页封装进新 Form，左右两次裁剪绘制），渲染结果正确，但内容变为两份（见「局限」）
- 纯 Rust 实现，唯一依赖 [lopdf](https://crates.io/crates/lopdf)

### 构建

需要 Rust 工具链（Rust 1.85+，edition 2024）：

```bash
git clone https://github.com/stevensli/pdf-crop-dual.git
cd pdf-crop-dual
cargo build --release
```

生成的二进制位于 `target/release/pdf-crop-dual`。

### 用法

```
pdf-crop-dual <输入.pdf> <输出.pdf> <中间空白宽度>
```

| 参数 | 说明 |
| ---- | ---- |
| `输入.pdf` | 要处理的双栏 PDF（不会被修改） |
| `输出.pdf` | 结果写入位置 |
| 中间空白宽度 | 要移除的空白带宽度，PDF 点单位（1 pt = 1/72 英寸）。程序会自动检测每页真实空白，指定值超过实际空白时自动收敛为实际值 |

示例：

```bash
./pdf-crop-dual book-dual.pdf book-narrow.pdf 80
```

运行输出示例（74 页双栏 PDF，页宽 1008 pt，实际空白约 123–140 pt；程序消息为中文）：

```
共 74 页，准备裁剪中间空白（指定宽度 = 80 pt）
第 1 页：检测到中间空白 [441.3, 564.3]（宽 122.9 pt）
第 2 页：检测到中间空白 [436.1, 574.0]（宽 137.9 pt）
第 30 页：未检测到明显空白，按页面对称处理
...
提示：最窄页面的空白仅 122.9 pt，移除宽度由 200.0 调整为 122.9 pt
  原宽: 1008.0, 新宽: 885.1, 移除区域: [441.3, 564.3]
  原宽: 1008.0, 新宽: 885.1, 移除区域: [443.6, 566.5]
...
完成！输出文件: book-narrow.pdf
```

### 工作原理

两遍算法：

1. **第一遍：空白检测**。遍历每页内容流（递归进入 Form XObject 并应用其 BBox 裁剪，按字体度量计算文本宽度），收集整页墨迹在 x 轴上的区间；以页面中线为界，左侧墨迹的最大右缘与右侧墨迹的最小左缘即空白带的左右边界。内容横跨中线或空隙不足 10 pt 的页面判为无清晰空白。最后把所有页的检测结果收敛为统一的移除宽度 `cut`，保证所有输出页宽度一致。
2. **第二遍：内容重建**。对每页内容流中的每个操作，按其在设备空间的 x 范围分类：
   - 完全位于移除带左侧 → 原样保留；
   - 完全位于移除带右侧 → 整体左移 `cut`（文本块重写定位操作符，路径平移构造坐标，Form 包一层平移矩阵——**Form 内部流不动**）；
   - 跨越移除带 → 该页回退传统方案。

   传统方案：将原页内容封装为新的 Form XObject，新内容流用两个裁剪矩形分别绘制左、右两半，右半平移 `cut`。重建后把页面 `MediaBox`/`CropBox` 的宽度减 `cut`。

### 局限

- **未检测到清晰空白的页面（内容横跨中线、空隙 < 10 pt 或纯空白页）按页面对称处理：移除带居中于页面中心。对内容真实横跨页面中线的页面（如整宽表格、章节标题页），这会切掉内容，运行后请检查输出。**
- 回退传统方案的页面，内容存在两份，文本层翻倍（文本提取会返回 2× 内容）；格式保留式重写的主路径无此问题
- 仅处理左右双栏布局，不对其他版式做任何调整

### 代码结构

```
src/main.rs   命令行与两遍算法编排（逐页扫描、宽度收敛、页面重建）
src/lib.rs    PDF 内容分析：对象访问、矩阵几何、字体推进宽度、
              内容流词法器、Walk 遍历、空白检测、格式保留式重写器
```

### 许可证

本项目采用 MIT 和 BSD 2-Clause 双许可发布，两种任选其一，详见 [LICENSE](LICENSE)。

---

## English

### Background

Bilingual PDFs produced by translation tools such as [pdfmathtranslate-next](https://github.com/PDFMathTranslate-next/PDFMathTranslate-next) default to a left–right two-column layout: the original English text in the left column, the Chinese translation in the right one. The columns are separated by an overly wide blank band, so pages look sparse and are hard to read on narrow screens. This tool narrows that band so the two columns meet and the page becomes slimmer.

```
Before:  [English original]     ····  wide blank band  ····     [Chinese translation]
After:   [English original][Chinese translation]   (page width = original width − removed width)
```

### Features

- **Per-page gap detection**: parses the PDF content streams (recursing into Form XObjects, computing text advance widths from font metrics, collecting path coordinates) instead of blindly cropping at a fixed page width
- **Format-preserving crop (primary path)**: for pages where a gap is detected, the page-level content stream is rewritten in place — ink from the left column stays put, ink from the right column is physically shifted left, and the original Form XObject structure is preserved. The content exists **exactly once** in the output: the text layer is not doubled, and PDF editing tools do not split paragraphs into separate text blocks
- **Safe width convergence**: the actual removed width is `cut = min(requested width, smallest gap over all pages)`, so an overly large request can never cut into column content
- **Per-page fallback**: pages that cannot be rewritten safely fall back to the traditional clip-based approach (wrap the whole page in a new Form XObject and draw the two halves with clipping). Rendering stays correct, but the page content then exists in two copies (see Limitations)
- Pure Rust, with a single dependency: [lopdf](https://crates.io/crates/lopdf)

### Building

Requires a Rust toolchain (Rust 1.85+, edition 2024):

```bash
git clone https://github.com/stevensli/pdf-crop-dual.git
cd pdf-crop-dual
cargo build --release
```

The binary ends up at `target/release/pdf-crop-dual`.

### Usage

```
pdf-crop-dual <input.pdf> <output.pdf> <gap width>
```

| Argument | Description |
| ---- | ---- |
| `input.pdf` | the two-column PDF to process (never modified) |
| `output.pdf` | where the result is written |
| gap width | width of the middle band to remove, in PDF points (1 pt = 1/72 inch). The real gap on each page is detected automatically; if the requested width exceeds the real gap, it is clamped down to the actual value |

Example:

```bash
./pdf-crop-dual book-dual.pdf book-narrow.pdf 80
```

Sample output (a 74-page two-column PDF, 1008 pt page width, real gaps ≈ 123–140 pt; program messages are in Chinese):

```
共 74 页，准备裁剪中间空白（指定宽度 = 80 pt）
第 1 页：检测到中间空白 [441.3, 564.3]（宽 122.9 pt）
第 2 页：检测到中间空白 [436.1, 574.0]（宽 137.9 pt）
第 30 页：未检测到明显空白，按页面对称处理
...
提示：最窄页面的空白仅 122.9 pt，移除宽度由 200.0 调整为 122.9 pt
  原宽: 1008.0, 新宽: 885.1, 移除区域: [441.3, 564.3]
  原宽: 1008.0, 新宽: 885.1, 移除区域: [443.6, 566.5]
...
完成！输出文件: book-narrow.pdf
```

### How it works

Two-pass algorithm:

1. **Pass 1 — gap detection.** Every page's content stream is walked (recursing into Form XObjects with their BBox clipping, measuring text widths from font metrics) to collect the page's ink spans on the x axis. Using the page center as the dividing line, the rightmost ink on the left and the leftmost ink on the right bound the gap. A page has no clear gap if content crosses the center line or the gap is narrower than 10 pt. All detected gaps are then converged to a single `cut` shared by every page, so all output pages have the same width.
2. **Pass 2 — content rebuild.** Every operation in a page's content stream is classified by its device-space x range:
   - entirely left of the removed band → kept as-is;
   - entirely right of the band → shifted left by `cut` (text blocks get rewritten positioning operators, paths get translated construction coordinates, Forms are wrapped in a translating matrix — **Form internals are untouched**);
   - crossing the band → the page falls back to the traditional approach.

   The traditional approach wraps the original page content in a new Form XObject and draws it twice with two clipping rectangles, the right half translated by `cut`. Afterwards the page's `MediaBox`/`CropBox` is narrowed by `cut`.

### Limitations

- **Pages with no clearly detected gap (content crossing the page center, gap < 10 pt, or blank pages) are handled page-symmetrically: the removed band is placed at the page center. Pages whose content genuinely crosses the center (full-width tables, chapter title pages) will be cut — check the output after running.**
- Pages handled by the traditional fallback keep two copies of the page content; the text layer is doubled (text extraction yields 2× the content). The format-preserving rewrite path, the primary one, has no such issue
- Only horizontal two-column layouts are handled; other layouts are not adjusted

### Project layout

```
src/main.rs   CLI and the two-pass algorithm orchestration (page scan, width convergence, page rebuild)
src/lib.rs    PDF content analysis: object access, matrix geometry, font advance widths,
              content-stream lexer, Walk traversal, gap detection, format-preserving rewriter
```

### License

The project is dual-licensed under the MIT License and the BSD 2-Clause License; you may choose either. See [LICENSE](LICENSE) for details.
