use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Dictionary, Object, ObjectId, Stream};
use std::env;

use pdf_crop_dual::{
    detect_gap, get_mediabox, get_resources, page_resources_dict, rewrite_page, Walk,
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
    let cut = compute_cut(&plans, gap_width);

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

/// 收敛实际移除宽度：不超过所有页最小空白宽度；cut<=1 时报错退出
fn compute_cut(plans: &[PagePlan], gap_width: f32) -> f32 {
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
    cut
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

/// 构建封装原页面内容的 Form XObject 流（BBox = 原页面框，Resources 从页复制）
fn build_form_stream(
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
fn register_form_xobject(
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
fn build_crop_content(
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
fn update_page_boxes(
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
