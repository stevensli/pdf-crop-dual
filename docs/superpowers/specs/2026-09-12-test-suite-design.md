# pdf-crop-dual 测试套件设计

日期：2026-09-12
状态：已批准（用户确认设计 OK）

## 1. 背景与目标

pdf-crop-dual 目前没有任何测试（CLAUDE.md 明确「没有测试框架，修改后的验证方法」仅靠 gs 手工渲染比对）。本设计为项目建立**完备的单元测试与集成测试**，全部测试代码放在独立的 `tests/` 目录中。

- **单元测试**：直接覆盖 `lib.rs` 各内部模块（矩阵几何、内容流词法器、字体宽度、`Walk` 遍历、`rewrite_page` 重写器、对象访问、`detect_gap`）以及从 `main.rs` 迁入 lib 的纯逻辑函数。
- **集成测试（e2e）**：运行真实二进制 + 真实 `test.pdf`（74 页），用 lopdf 做结构断言，用 `gs` 做像素级/文本层断言（沿用 CLAUDE.md 的验证口径）。

约束：

1. 测试代码全部位于 `tests/` 目录，`src/` 内不放任何测试代码。
2. 零新增依赖：只用现有 lopdf 0.45 + std。临时文件用 `std::env::temp_dir()` + 进程 ID + 原子计数器命名。
3. `src/` 的改动仅限**加 `pub` 可见性**与**迁移 5 个纯函数**，不改变任何行为逻辑。

## 2. 环境基线（设计时实测）

- test.pdf：74 页，页面 1008×661.5pt；每页两个 504pt Form XObject（左栏英文、右栏中文）。
- 最小中间空白：**122.9 pt（第 1 页，区间 [441.3, 564.3]）**；其余 gap 页 130.5~140.0 pt。
- 第 30/48/74 页：无墨迹（空白页）→ 未检测到空白 → 回退传统 clip 方案。
- 指定 100 → 输出宽 908；指定 500 → 收敛为 122.9（stdout 出现「提示」行）；指定 0.5 → exit 1，stderr「错误：空白宽度必须大于 1 pt，且页面需存在足够的中间空白」。
- 工具链：Rust 1.98.1（edition 2024）、gs 10.02.1（/usr/bin/gs；注意某些 shell 下 `which gs` 失败，探测须用 `Command` 直接尝试执行）。

## 3. src/ 的改动

### 3.1 lib.rs 加 `pub`（仅测试直接引用的项）

| 项 | 说明 |
|---|---|
| `Mat`（含 `I`/`of`/`translate`/`mul`/`x_of`） | 3x2 矩阵，行向量约定 |
| `x_extents` | 点集经矩阵投影的 x 范围 |
| `clip_intervals_to_bbox` | 区间按 BBox 钳位 |
| `FontInfo`（Simple/Cid/Unknown 各字段） | 字体宽度信息 |
| `glyph_width`、`parse_w_entry` | 单码字宽、CID W 条目解析 |
| `build_font_info`、`build_cid_font_info`、`descendant_cid_font`、`resolve_font_id` | 字体解析链 |
| `str_advance`、`tj_advance` | 文本 advance 共用口径 |
| `Val`、`Item`、`Parsed`、`Tok`（含 `data`/`pos` 字段及全部解析方法） | 内容流词法器 |
| `val_to_obj`、`shift_path_op`、`add_to_real` | 重写器小工具 |

保持私有：`onum`、`obj_dict`、`sub_dict`、`page_inherit`、`find_xobject`、`nums_at`（经公共入口间接覆盖）、`Rewriter` 结构体与 `Walk` 的私有字段。

### 3.2 main.rs → lib.rs 迁移（5 个函数，仅 `compute_cut` 改签名）

| 函数 | 变化 |
|---|---|
| `compute_cut` | 签名改为 `pub fn compute_cut(gaps: &[Option<(f32, f32)>], gap_width: f32) -> Result<(f32, f32), String>`：返回 `(实际 cut, 最小检测空白宽; 无 gap 页时为 f32::INFINITY)`；`cut <= 1.0` 时返回 `Err(原错误消息文本)`，不在 lib 内 `process::exit`。「提示」行的打印移到 main（依据返回值判断）。main 侧调用改为收集 `plans.iter().map(|p| p.gap)` 传入 |
| `build_form_stream` | 原样移动，加 `pub` |
| `register_form_xobject` | 原样移动，加 `pub` |
| `build_crop_content` | 原样移动，加 `pub`（使「裁剪矩形必须在 cm 之前」这一历史 bug 不变量可直接单测） |
| `update_page_boxes` | 原样移动，加 `pub` |

main.rs 保留：`PagePlan`、`scan_page`、`rebuild_page`、`parse_args`、`main`（CLI 编排，由 e2e 覆盖）。

迁移后 main.rs 对 lib 的导入增加上述 5 个函数；行为逐字节不变（除 `compute_cut` 的 exit 改由 main 执行，输出文案保持一致）。

## 4. tests/ 文件布局

```
tests/
  common/mod.rs    # 共享助手（子目录，不会被 cargo 当作独立测试目标）
  geometry.rs      # Mat / x_extents / clip_intervals_to_bbox
  lexer.rs         # Tok 词法器
  fonts.rs         # FontInfo / W 解析 / advance 口径
  object_access.rs # get_mediabox / get_resources / page_resources_dict
  detect_gap.rs    # 空白检测
  walk.rs          # Walk 内容流遍历
  rewrite.rs       # rewrite_page 格式保留重写
  main_logic.rs    # 迁移后的 5 个纯逻辑函数
  e2e.rs           # CLI + test.pdf 端到端
```

### common/mod.rs 助手

- **合成文档构造器**（全部内存构造，`Document::new()` + `add_object`）：
  - `type1_font(doc, first_char, widths) -> ObjectId`
  - `cid_font(doc, dw, w_entries) -> ObjectId`（DescendantFonts → CIDFont，W 数组三形式）
  - `type0_font(doc, cid_font_id) -> ObjectId`
  - `form_xobject(doc, content: &[u8], matrix: Option<[f32;6]>, bbox: Option<[f32;4]>, resources: Option<&Dictionary>) -> ObjectId`
  - `image_xobject(doc, matrix) -> ObjectId`
  - `page_resources(doc, fonts: &[(&[u8], ObjectId)], xobjects: &[(&[u8], ObjectId)]) -> Dictionary`（内联字典，测试时直接借用传给 Walk/rewrite_page）
  - 页树构造：`page_with_parent(doc, ...)` 用于 MediaBox/Resources 继承测试
- **二进制运行器**：`run_tool(args) -> (i32, String stdout, String stderr)`，二进制路径 `env!("CARGO_BIN_EXE_pdf-crop-dual")`。
- **gs 助手**：`gs_available() -> bool`（`Command::new("gs").arg("--version")` 探测，不用 `which`）；`render_pgm(pdf, page) -> Pgm`（72dpi pgmraw，`-dFirstPage/-dLastPage` 单页）；`render_txt(pdf) -> String`（txtwrite 整文件）。
- **PGM**：`Pgm { width, height, maxval, bytes }`；解析 P5 头（逐 token：先 "P5"，再跳过 `#` 注释行至行尾，收 width/height/maxval 三个整数，其余为像素数据）；`dark_cols(&Pgm) -> Option<(min, max)>`（灰度 < 200）；`manual_crop(orig, band_left_px: isize, cut_px: isize) -> Pgm`（左段 `[0, band_left)` 原样 + 右段 `[band_left+cut, ..)` 左移）。
- **文本层**：`count_text_codepoints(txt) -> usize`：过滤 `[ \t\r\n\x00-\x1f]` 后 UTF-8 码点数；`split_pages(txt) -> Vec<String>`（按 `\f` 分页）。
- **临时文件**：`tmp_file(name) -> PathBuf`（`temp_dir/pdf-crop-dual-test-{pid}-{原子计数}/{name}`）。
- **Content 断言助手**：`ops_of(&Content) -> Vec<(String, Vec<f32>)>`（操作符 + 数值操作数，Name/String 另存）；`assert_ops(actual, expected)`（f32 容差 1e-3）。

## 5. 单元测试覆盖清单

### geometry.rs
- `Mat::I` 恒等；`translate` 平移点。
- **`mul` 顺序约定**：`mul(translate(10,0), scale(2))` 对 x=1 给 22（先移后缩）；`mul(scale(2), translate(10))` 给 12（先缩后移）——固化「p·m1·m2，先 m1 后 m2」文档约定。
- 平移合成 `mul(translate(1,2), translate(3,4))` → e=4, f=6。
- 旋转矩阵 `of(0,-1,1,0,0,0)`：`x_of(x,y) == y`。
- `x_extents`：缩放/负缩放（min/max 正确互换）下点集 x 范围。
- `clip_intervals_to_bbox`：跨边钳位、完全在外剔除、全空结果；mark 之前的区间不受影响。

### lexer.rs
- 数字：`123`、`-4.5`、`+7`、`.25`、`3.`；孤立符号（`-` 后无数字）→ `Parsed::Err`；EOF/纯空白 → `next_item` 返回 `None`。
- 混合流顺序：`1.5 -2 /F1 (abc) [1 2 3] BT` 逐项断言。
- 名称：`/F1`；十六进制转义 `/Co#6Cor` → `b"Cor"`；`%` 终止名称。
- 字面量串：转义 `\n \r \t \b \f \\ \( \)`；八进制 `\101`→`b"A"`、`\12`→`10u8`、`\8`→`b"8"`；行续 `\<LF>` 与 `\<CRLF>` 消耗不产字节；嵌套括号 `(a(b)c)`；未闭合 → `Err`。
- 十六进制串：`<4849>`→`b"HI"`；允许内部空白；奇数位补 0（`<4>`→`[4]`）；非法十六进制字符 → `Err`。
- 数组：嵌套 `[1 2.5 (s) /n [3]]`；数组内裸词/字典被丢弃（`[BT 1]` → `[Num(1)]`）。
- 字典跳过：`<< /A 1 /B [1 2] /C (x) /D 5 0 R >>` → `DictSkipped` 且后续 token 位置正确；空字典 `<< >>`。
- 注释：`% c\n1` → `Num(1)`；文件尾无换行注释。
- 内联图像：`BI /Width 1 /Length 4 ID\nwxyz\nEI S`：首个 item 为 `Op("BI")`，`handle_inline_image()` 返回 true，随后 `next_item` 为 `Op("S")`；`ID` 后 `<CRLF>` 变体；缺 `EI` / Length 越界 → false。

### fonts.rs
- `str_advance` 无字体：每字节 1em（`len * tfs`）。
- Simple 字体：`first=32, widths=[500; '!'=1000]`，`"A!"`@tfs=10 → 15.0；码字超出 Widths 范围 → 1000 回退。
- CID 字体：2 字节解码（`b"\x00\x41\x00\x42"`），widths 命中/未命中（用 DW）；奇数字节串丢弃末字节。
- `Tc` 逐字形累加；`Tz` 仅对码字 32（空格）加 `tz/100 * tw`。
- `tj_advance`：`[Str, Num(-200), Str]` 缩进项 = `-0.2 * tfs`。
- `parse_w_entry`：`[first w]`；`[first last w]`（first>last 不插入）；`[first [w1 w2 ...]]`（含 u16 wrapping 边界 65535）。
- `build_font_info`：Type1（FirstChar+Widths）→ Simple；缺 Widths → Unknown；Type0/Type0C → Cid。
- `build_cid_font_info`：DW 覆盖默认 1000；W 三形式合并；缺 DescendantFonts → 空 Cid（dw=1000）。
- `resolve_font_id`：成功并缓存（二次调用不重复解析）；res 为 None / 名称不存在 / 条目非间接引用 → None。

### object_access.rs
- `get_mediabox`：页内 Real 值；页内 Integer 值（`as_float` 兼容）；经 Parent 继承；缺失 → `Err`（ObjectNotFound）；数组含非数字 → `Err`。
- `get_resources`：页内内联字典；经 Reference 到独立字典；经 Parent 继承。
- `page_resources_dict`：内联字典返回借用；Reference 解析；缺失 → None。

### detect_gap.rs
- 正常双栏：左 `(72,400)`、右 `(560,900)`，页宽 1008 → `Some((402, 558))`（两侧各 +2/−2 余量）。
- 有内容横跨中线 → None。
- 余量后宽度 < 10 → None（如左 500 右 510 → l=502, r=508）。
- 仅左侧 / 仅右侧 / 空区间列表 → None。
- 贴中线边界：`b == mid` 归左、`a == mid` 归右（`a < mid && b > mid` 才判跨线）。
- 多区间取左最大右缘、右最小左缘；输入乱序（右区间在前）结果不变。

### walk.rs（内存合成文档）
- 文本：Tm+Tj 区间 `[e, e+adv]`（已知字体宽）；Tf 名称在 res 中不存在 → 1em 回退。
- Td 文本空间缩放：Tm 线性部分 (2,2) 后 Td(1,0) → e 增加 2；叠加 cm 缩放下设备坐标正确。
- TD 同时设 TL；T* 下移一行（x 不变）；`'` 换行显示；`"` 先应用 Tw/Tc 再换行显示。
- TJ 区间宽度 = `tj_advance`（含负缩进）。
- q/Q：`q cm(+100) Q` 后位置恢复。
- cm 作用于路径（re 缩放后区间）与文本。
- 路径构造：re/m/l/c（三控制点全计入）/v/y；各终结操作（S/f/B/…）产生区间；W/W*/n 丢弃；空路径终结无区间。
- Do Form：Matrix 平移定位；BBox 裁剪（超出部分剔除）；无 BBox 不裁剪；空 Form 无区间；Form 内 q/Q 图形状态不外泄（Form 返回后 ctm 复原）。
- Do Form 深度上限：10+ 层嵌套，第 8 层以内计入、超出者不计。
- **已知局限固化**：同一 Form 在同一次 Walk 中绘制两次（不同 cm 位置），`seen_forms` 不清除 → 第二次不计入（test.pdf 不受影响，每页两个不同 Form；此测试仅作行为文档）。
- Do Image：单位正方形 ×（Matrix·ctm）四角 x 范围；ctm 缩放下正确。
- Do 未知名 / resources 为 None → 无区间、不崩溃。
- 内联图像：合法 BI..EI 之后继续处理（后续 re f 产生区间）；损坏的 BI（缺 EI）→ 按现状**中止整个 walk**（后续操作不再处理，固化现状）。
- 文本对象外（BT 之前 / ET 之后）的 Tm/Tj 等 → 忽略。

### rewrite.rs（内存合成文档，band=[500,600) 即 band_left=500, cut=100 为基准参数）
- **左原样**：左侧文本块/路径/Do 的操作与操作数逐字不变。
- **文本块 Tm 逆修正**：右侧 Tm e=600 → 发射 e=500（ctm=I）；Tm 线性部分不变；左侧 Tm 无修正。
- **无 Tm 合成**：`q cm(1,0,0,1,600,0) BT Tf Tj ET Q` → 块内 Tj 前合成 `Tm(1 0 0 1 500 0)`；左侧同形块不合成。
- **Td 同侧**：右 Tm 后 Td(10,0) 操作数不变（delta=0）。
- **Td 跨左→右**：Tm(100) 后 Td(520,0)（新 e=620 右侧）→ 发射 Td(420,0)。
- **Td 跨右→左**：Tm(600) 后 Td(-520,0)（新 e=80 左侧）→ 发射 Td(-420,0)（基于已修正基 500）。
- **Td 落带内** → None（如 Tm(100)+Td(400,0) → e=500 不满足严格不等式）。
- **T* 分解**：TL 12 + T* → `Td(0, -12)`（同侧）。
- **" 分解**：`2 3 (Hi) "` → `Tw 2`、`Tc 3`、`Td(0,-tl+修正)`、`Tj (Hi)` 四操作顺序。
- `'` 分解：`Td(0,-tl)` + `Tj`。
- **范围校验**：Tm(450) Tj 宽度 200 跨带 → None；hi < band_left 临界通过。
- **路径**：右侧 m/l S 坐标加 (-cut,0)；左不变；跨带 → None；ctm 缩放 (2,2) 下右路径操作空间偏移 (-cut/2, 0)；re 右侧平移；多子路径混合（左子路径原样+右子路径移位，共享终结操作只发一次）；`h` 不增点；m 之前的构造操作进 preamble，首个终结操作前原样补发。
- **W/W\*/n 终结**：裁剪子路径同样分类（右侧裁剪矩形移位）——固化现状。
- **Do Form**：全右 → 包 `q cm(1 0 0 1 -cut 0) Do Q`，Do 操作数保留；全左原样；跨带 → None；空墨迹 Form 原样不包。
- **Do Image**：右侧 → 同包裹；未知名 → 原样。
- **块内透传**：`g G rg RG k K sc SC scn SCN cs CS gs w J j M d ri i Tr Ts MP BMC EMC` 原样入块且顺序保留。
- **ET 必发（历史 bug 回归）**：任意成功重写的块以 ET 收尾；右文本块 + 块后路径 S 均完整出现在输出（漏 ET 时 gs 会丢后续路径墨迹）。
- **回退触发清单**（各返回 None）：`BI`（内联图像）；未知操作 `sh`；`/MC BDC`（带字典操作数）；文本块内 `q`；`BT` 未闭合（run 末尾 in_text）。
- **退化矩阵**：`0 0 0 0 700 0 cm` + 右路径 → None（shift_vec det≈0）。
- **块外文本操作**：BT 之外的 Tm/Tj/Tf 原样通过（Tf 更新状态）。
- **工具函数**：`val_to_obj`（Num→Real、Name、Str→String(Literal)、Arr 嵌套）；`shift_path_op`（m/l/re/c/v/y 各坐标位平移，未知操作符不变）；`add_to_real`（加 delta；delta=0 不动；非 Real 操作数不动）。
- **Walk↔重写器口径一致性（核心不变量）**：构造左 Form+右 Form+直接文本的双栏内容 → Walk+detect_gap 得带 → rewrite_page 成功 → 将输出 `Content::encode()` 重新过 Walk → 输出坐标系下的区间集合 == 原始左侧区间 ∪（原始右侧区间 − cut）（原移除带位置在输出中已被左移后的内容占据，不能以「带内无新区间」断言）。

### main_logic.rs
- `compute_cut`：
  - spec < 最小 gap → `Ok((spec, min_gap))`；
  - spec > 最小 gap → `Ok((min_gap, min_gap))`（收敛）；
  - 混入 None（无 gap 页）→ 仅对 Some 求最小；
  - 全 None → `Ok((spec, f32::INFINITY))`；
  - spec ≤ 1 → `Err`（消息含「空白宽度必须大于 1 pt」）；
  - 最小 gap < 1（如 (100,100.5)）→ `Err`。
- `build_form_stream`：字典 Type=XObject、Subtype=Form、FormType=1、BBox 四值、Resources 为传入对象的深拷贝；流内容 == 传入字节。
- `build_crop_content`：**操作序列精确断言**（13 个操作）：
  - 左半：`q, re(x1, y1, band_left−x1, h), W, n, Do(name), Q`；
  - 右半：`q, re(band_left, y1, x2−cut−band_left, h), W, n, cm(1 0 0 1 −cut 0), Do(name), Q`；
  - **顺序不变量**：右半 `W` 的索引 < `cm` 的索引（历史 bug：clip 必须在 cm 前定义）；右裁剪区从 band_left 起、宽 `x2−cut−band_left`。
- `register_form_xobject` 三情形：
  - 页无 Resources → 新建 Resources 字典对象，页引用之，含 `XObject/form_name → form_id`；
  - 页 Resources 为内联字典 → 提取为独立对象并更新页引用，XObject 含新条目；
  - 页 Resources 为引用且已有 XObject → 新条目并入、原有条目不丢。
- `update_page_boxes`：Contents 替换为 new_content_id；MediaBox 宽 `x2−cut`（x1/y1/y2 不变）；CropBox 存在则同步、不存在则不新增；TrimBox/BleedBox/ArtBox 一律删除。

## 6. 集成测试（e2e.rs）

前置：`test.pdf` 位于 `env!("CARGO_MANIFEST_DIR")/test.pdf`（缺失则测试报清晰错误）；输出写到临时目录。

### A. 纯 lopdf 断言（无 gs 依赖，全 74 页）

1. **cli_arg_errors**：无参/2 参 → exit 1 且 stderr 含「用法」；宽度非数字（`abc`）→ 非零退出、stderr 含「空白宽度必须是数字」；输入文件不存在 → 非零退出、stderr 含「无法加载 PDF」。
2. **cli_tiny_width_rejected**：宽度 0.5 → exit 1、stderr 含「空白宽度必须大于 1 pt」。
3. **normal_crop_boxes**（spec=100）：exit 0；输出可被 lopdf 加载；74 页；每页 MediaBox 统一 `[0, 0, 908, 661.5]`（高度不变，宽 = 1008−100）。
4. **converges_to_min_gap**（spec=500）：exit 0；stdout 含「提示」；最小空白由**测试内独立计算**（对原始 test.pdf 逐页 Walk+detect_gap 求最小），断言输出宽 == `1008 − min_gap`（容差 0.01）。
5. **format_preserving_structure**（spec=100）：
   - gap 页（lib 判定 gap=Some，含 1/6/9/46 等 71 页）：输出页 `/Resources/XObject` 的键集合与原始页相同（未注册 FormXn → 格式保留路径）；
   - 30/48/74 页：键集合 = 原键 ∪ `{FormXn}`；新 Contents 解码后（用 lib 词法器 `Tok` 解析）恰含 2 个 `Do /FormXn`、1 个 `cm`（e=−cut）、右半 `W` 在 `cm` 之前。

### B. gs 断言（`gs_available()` 为 false 时 `eprintln!` 说明并直接返回，不失败）

6. **text_layer_single_x**（spec=100）：`gs txtwrite` 各渲染原/输出一次（各 1 次调用），按 `\f` 分页后逐页过滤空白数码点：
   - gap 页：输出码点数 == 原始（1×，格式保留核心指标）；
   - 30/48/74：输出 == 2×原始（空白页即 0==0）。
7. **pixel_manual_crop**（spec=100）：`pgmraw -r72` 渲染代表页集合 {1, 6, 9, 46}（普通/代码块+颜色+`i`/装饰路径/标记内容）与回退页 {30, 48, 74}：
   - 输出 PGM 宽 == 908 px；
   - 移除带位置按 `rebuild_page` 同公式复算：gap 页 `c=(l+r)/2, band_left=c−cut/2`（l,r 来自 Walk+detect_gap）；无 gap 页 `band_left = x1 + (1008−cut)/2`；
   - **逐像素断言**：`输出像素 == manual_crop(原始像素)`（左段 `[0, floor(band_left)]` 原样 + 右段 `[band_left+cut, ..)` 左移；spec=100 为整数像素平移，band 内 ≥2pt 无墨迹保证等价严格成立）；
   - 若实测出现系统性 1px 差异，降级为「暗像素级全等 + 总差异像素 < 0.01%」并回写本设计文档记录。
8. **dark_extent**（spec=100）：同代表页集合，暗像素（<200）min/max 列：左栏范围与原始一致，右栏整体左移 cut，左右缘不丢。
9. **`#[ignore] pixel_all_pages`**：全 74 页逐像素手工裁剪比对（手动深检用，`cargo test -- --ignored`）。

## 7. 已知局限与风险处理

| 项 | 处理 |
|---|---|
| `Walk.seen_forms` 从不清除：同一 Form 一页绘制两次时第二次墨迹不计入 | walk.rs 固化现状测试并注释说明；test.pdf 不受影响 |
| 词法器/重写器测试固化现状，若暴露与 CLAUDE.md 不变量冲突的真 bug | 先停下报告用户，不擅自改 src 行为 |
| gs 可能不可用（部分 shell `which gs` 失败） | `gs_available()` 用 `Command` 直接探测；gs 类测试优雅跳过 |
| 像素全等假设（gs 跨页尺寸渲染一致性） | 主断言「输出==手工裁剪」；备降方案见 §6.7 |
| 无新依赖 | 临时文件用 temp_dir + pid + `AtomicUsize` 计数 |

## 8. 验证方式

- `cargo test`：单元测试全量 + e2e（预计 30~60s）。
- `cargo test -- --ignored`：全 74 页像素深检。
- 迁移正确性：实施第一步先用**迁移前**的构建对 test.pdf（spec=100）跑一遍留底（临时目录），迁移+重建后再跑一遍，两份输出用 `gs` 72dpi 逐页像素全等比对，确认行为逐字节不变。
