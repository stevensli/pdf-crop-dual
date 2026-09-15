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
