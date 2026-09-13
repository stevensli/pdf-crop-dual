use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use std::env;

use pdf_crop_dual::{
    build_crop_content, build_form_stream, compute_cut, detect_gap, get_mediabox, get_resources,
    page_resources_dict, register_form_xobject, rewrite_page, update_page_boxes, Walk,
};

fn main() {
    let (input_path, output_path, gap_width) = parse_args();

    let mut doc = Document::load(&input_path).expect("无法加载 PDF");
    let pages = doc.get_pages();

    println!("共 {} 页，准备裁剪中间空白（指定宽度 = {} pt）", pages.len(), gap_width);

    // ---------- 第一遍：扫描每页内容，检测真实中间空白 ----------
    let mut plans: Vec<PagePlan> = Vec::new();
    for (page_num, &page_id) in pages.iter() {
        if let Some(plan) = scan_page(&doc, *page_num, page_id) {
            plans.push(plan);
        }
    }

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

    // ---------- 第二遍：重建每页内容流 ----------
    for plan in &plans {
        rebuild_page(&mut doc, plan, cut);
    }

    // 压缩并保存
    doc.compress();
    doc.save(&output_path).expect("保存 PDF 失败");

    println!("完成！输出文件: {}", output_path);
}

/// 解析命令行参数；数量或格式错误时打印用法并退出
fn parse_args() -> (String, String, f32) {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("用法: {} <输入.pdf> <输出.pdf> <中间空白宽度>", args[0]);
        eprintln!("  中间空白宽度: 要裁剪掉的中间空白区域的宽度（PDF 点单位，如 80）");
        eprintln!("  程序会自动检测每页真实空白的位置；指定宽度超过实际空白时自动收敛为实际值");
        eprintln!("  示例: {} input-dual.pdf output.pdf 80", args[0]);
        std::process::exit(1);
    }
    let gap_width: f32 = args[3].parse().expect("空白宽度必须是数字");
    (args[1].clone(), args[2].clone(), gap_width)
}

/// 单页裁剪计划（第一遍扫描产物）
struct PagePlan {
    page_num: u32,
    page_id: ObjectId,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    gap: Option<(f32, f32)>,
}

/// 第一遍扫描单页；页对象/MediaBox 失败时返回 None，
/// 缺 Resources 时返回 gap=None 的正常计划（不打错误消息）
fn scan_page(doc: &Document, page_num: u32, page_id: ObjectId) -> Option<PagePlan> {
    let page_obj = doc.get_object(page_id).expect("获取页面对象失败");
    let page_dict = match page_obj {
        Object::Dictionary(d) => d,
        _ => {
            eprintln!("跳过第 {} 页：页面对象不是字典", page_num);
            return None;
        }
    };
    let mediabox = match get_mediabox(doc, page_dict, page_id) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("跳过第 {} 页：无法获取 MediaBox: {}", page_num, e);
            return None;
        }
    };
    let (x1, y1, x2, y2) = (mediabox[0], mediabox[1], mediabox[2], mediabox[3]);

    let gap = match page_resources_dict(doc, page_dict, page_id) {
        Some(res) => {
            let content = doc.get_page_content(page_id);
            let mut walker = Walk::new(doc);
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
        None => None,
    };

    Some(PagePlan {
        page_num,
        page_id,
        x1,
        y1,
        x2,
        y2,
        gap,
    })
}

/// 第二遍重建单页内容流；页面宽度过小或获取失败时跳过
fn rebuild_page(doc: &mut Document, plan: &PagePlan, cut: f32) {
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

    if total_width <= cut {
        eprintln!(
            "  跳过：页面宽度 ({:.1}) 小于等于移除宽度 ({:.1})",
            total_width, cut
        );
        return;
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
        _ => return,
    };

    // 获取原始页面内容流（已解码合并）
    let original_content = doc.get_page_content(page_id);

    // 获取 Resources（字体、图片等）
    let resources = match get_resources(doc, &page_dict, page_id) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  跳过：无法获取 Resources: {}", e);
            return;
        }
    };

    // 检测到空白时优先直接重写内容流：左侧原样、右侧物理左移 cut，内容只存在
    // 一份且保留原有 Form 结构（格式保留式裁剪）；不可重写时回退传统方案
    let rewritten = if gap.is_some() {
        let res_dict = page_resources_dict(doc, &page_dict, page_id);
        rewrite_page(doc, &original_content, res_dict, band_left, cut)
    } else {
        None
    };

    let new_content = match rewritten {
        Some(content) => content,
        None => {
            // 创建 Form XObject（将原页面内容封装进去）并注册进页面 Resources
            let form_name = format!("FormX{}", page_num);
            let form_name_bytes = form_name.into_bytes();
            let form_stream = build_form_stream(x1, y1, x2, y2, &resources, original_content);
            let form_id = doc.add_object(form_stream);
            register_form_xobject(doc, &page_dict, page_id, &form_name_bytes, form_id);

            // 构建新的内容流
            build_crop_content(x1, y1, x2, y2, band_left, cut, &form_name_bytes)
        }
    };
    let new_content_stream = Stream::new(dictionary! {}, new_content.encode().unwrap());
    let new_content_id = doc.add_object(new_content_stream);

    // 更新页面字典：替换 Contents、MediaBox、CropBox
    update_page_boxes(doc, page_id, new_content_id, x1, y1, x2, y2, cut);
}
