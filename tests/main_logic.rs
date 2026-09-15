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

    // v/y 坐标位：仅 (0,1)(2,3) 两对平移
    let mut v = Operation::new(
        "v",
        vec![10.0.into(), 20.0.into(), 30.0.into(), 40.0.into()],
    );
    shift_path_op(&mut v, (-100.0, 5.0));
    assert_eq!(v.operands[0], Object::Real(-90.0));
    assert_eq!(v.operands[1], Object::Real(25.0));
    assert_eq!(v.operands[2], Object::Real(-70.0));
    assert_eq!(v.operands[3], Object::Real(45.0));
    let mut y = Operation::new("y", vec![1.0.into(), 2.0.into(), 3.0.into(), 4.0.into()]);
    shift_path_op(&mut y, (-10.0, 3.0));
    assert_eq!(y.operands[0], Object::Real(-9.0));
    assert_eq!(y.operands[1], Object::Real(5.0));
    assert_eq!(y.operands[2], Object::Real(-7.0));
    assert_eq!(y.operands[3], Object::Real(7.0));

    let mut d = Operation::new("Do", vec![10.0.into()]);
    shift_path_op(&mut d, w); // 未知操作符不变
    assert_eq!(d.operands[0], Object::Real(10.0));
}
