//! 集成测试：CLI 端到端行为与输出 PDF 结构断言（纯 lopdf，无 gs 依赖部分）

mod common;

use common::{
    count_text_codepoints, gs_available, page_resources, page_tree, render_pgm_page,
    render_txt_file, render_txt_page, run_tool, tmp_file, type1_font,
};
use lopdf::{dictionary, Document, Dictionary, Object, ObjectId, Stream};
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
