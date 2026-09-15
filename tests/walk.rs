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

#[test]
fn y追加两点() {
    // y 臂（与 v 同索引 (0,1)(2,3)）：m(0,0) + y(10,100,20,200) → x∈(0,20)
    // 回归（y 臂缺失）→ (0,0)；回归（索引行列互换 (0,2)(1,3)）→ (0,100)
    let (doc, _) = doc_with_font();
    let iv = walk(&doc, "0 0 m 10 100 20 200 y f", None);
    assert_eq!(iv, vec![(0.0, 20.0)]);
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
