//! 字体宽度解析与文本 advance 单元测试
#![allow(non_snake_case)]


mod common;

use common::{cid_font, page_resources, type0_font, type1_font};
use lopdf::{dictionary, Document, Object};
use pdf_crop_dual::{
    build_font_info, glyph_width, parse_w_entry, resolve_font_id, str_advance, tj_advance,
    FontInfo, Val,
};
use std::collections::HashMap;

fn w_pair(first: f32, w: f32) -> Object {
    Object::Array(vec![first.into(), w.into()])
}

fn w_range(first: f32, last: f32, w: f32) -> Object {
    Object::Array(vec![first.into(), last.into(), w.into()])
}

fn w_pair_array(first: f32, ws: &[f32]) -> Object {
    let inner = Object::Array(ws.iter().map(|w| Object::Real(*w)).collect());
    Object::Array(vec![Object::Real(first), inner])
}

#[test]
fn str_advance无字体按1em() {
    assert_eq!(str_advance(None, b"AB", 12.0, 0.0, 0.0, 100.0), 24.0);
}

#[test]
fn str_advance_simple字体() {
    let f = FontInfo::Simple {
        first: 32,
        widths: vec![500.0, 1000.0],
    };
    // 空格(32)→500, '!'(33)→1000；@tfs=10 → 5 + 10
    assert_eq!(str_advance(Some(&f), b" !", 10.0, 0.0, 0.0, 100.0), 15.0);
    // 码字超出 Widths 范围 → 1em 回退
    assert_eq!(str_advance(Some(&f), b"C", 10.0, 0.0, 0.0, 100.0), 10.0);
}

#[test]
fn glyph_width_simple越界回退1em() {
    let f = FontInfo::Simple {
        first: 32,
        widths: vec![500.0],
    };
    assert_eq!(glyph_width(&f, 32), 500.0);
    assert_eq!(glyph_width(&f, 33), 1000.0);
    assert_eq!(glyph_width(&f, 31), 1000.0);
}

#[test]
fn glyph_width_cid缺省用DW() {
    let mut widths = HashMap::new();
    widths.insert(0x0041u16, 600.0);
    let f = FontInfo::Cid {
        widths,
        dw: 800.0,
    };
    assert_eq!(glyph_width(&f, 0x0041), 600.0);
    assert_eq!(glyph_width(&f, 0x0042), 800.0);
}

#[test]
fn str_advance_cid按2字节解码() {
    let mut widths = HashMap::new();
    widths.insert(0x0041u16, 600.0);
    let f = FontInfo::Cid {
        widths,
        dw: 800.0,
    };
    assert_eq!(
        str_advance(Some(&f), b"\x00\x41\x00\x42", 10.0, 0.0, 0.0, 100.0),
        14.0
    );
    // 尾部奇数字节被忽略
    assert_eq!(
        str_advance(Some(&f), b"\x00\x41\x00\x42\x00", 10.0, 0.0, 0.0, 100.0),
        14.0
    );
}

#[test]
fn str_advance_Tc逐字形累加() {
    assert_eq!(str_advance(None, b"AB", 10.0, 0.0, 0.5, 100.0), 21.0);
}

#[test]
fn str_advance_Tz仅对空格生效() {
    // 空格码 32：附加 tz/100 * tw
    assert_eq!(str_advance(None, b"A B", 10.0, 2.0, 0.0, 100.0), 32.0);
    assert_eq!(str_advance(None, b"A B", 10.0, 2.0, 0.0, 50.0), 31.0);
}

#[test]
fn tj_advance含缩进项() {
    let items = vec![Val::Str(b"AB".to_vec()), Val::Num(-200.0), Val::Str(b"C".to_vec())];
    // 20 - 2 + 10
    assert_eq!(tj_advance(None, &items, 10.0, 0.0, 0.0, 100.0), 28.0);
}

#[test]
fn parse_w_entry三种形式() {
    let mut m = HashMap::new();
    parse_w_entry(&[5.0.into(), 600.0.into()], &mut m);
    assert_eq!(m.get(&5u16), Some(&600.0));

    let mut m = HashMap::new();
    parse_w_entry(&[5.0.into(), 8.0.into(), 500.0.into()], &mut m);
    assert_eq!(m.len(), 4);
    for c in 5..=8 {
        assert_eq!(m.get(&c), Some(&500.0));
    }

    // first > last：不插入
    let mut m = HashMap::new();
    parse_w_entry(&[9.0.into(), 5.0.into(), 500.0.into()], &mut m);
    assert!(m.is_empty());

    let mut m = HashMap::new();
    let entry = w_pair_array(10.0, &[100.0, 200.0, 300.0]);
    parse_w_entry(entry.as_array().unwrap(), &mut m);
    assert_eq!(m.get(&10u16), Some(&100.0));
    assert_eq!(m.get(&11u16), Some(&200.0));
    assert_eq!(m.get(&12u16), Some(&300.0));
}

#[test]
fn parse_w_entry_u16环绕() {
    let mut m = HashMap::new();
    let entry = w_pair_array(65535.0, &[100.0, 200.0]);
    parse_w_entry(entry.as_array().unwrap(), &mut m);
    assert_eq!(m.get(&65535u16), Some(&100.0));
    assert_eq!(m.get(&0u16), Some(&200.0));
}

#[test]
fn build_font_info_type1() {
    let mut doc = Document::new();
    let id = type1_font(&mut doc, 32, &[500.0, 1000.0]);
    let d = doc.get_object(id).unwrap().as_dict().unwrap().clone();
    match build_font_info(&doc, &d) {
        FontInfo::Simple { first, widths } => {
            assert_eq!(first, 32);
            assert_eq!(widths, vec![500.0, 1000.0]);
        }
        FontInfo::Cid { .. } => panic!("期望 Simple"),
        FontInfo::Unknown => panic!("期望 Simple"),
    }
}

#[test]
fn build_font_info缺Widths为Unknown() {
    let mut doc = Document::new();
    let dict = dictionary! { "Subtype" => "Type1" };
    let id = doc.add_object(Object::Dictionary(dict));
    let d = doc.get_object(id).unwrap().as_dict().unwrap().clone();
    match build_font_info(&doc, &d) {
        FontInfo::Unknown => {}
        _ => panic!("期望 Unknown"),
    }
}

#[test]
fn build_font_info_type0_cid() {
    let mut doc = Document::new();
    let w = vec![
        w_range(5.0, 8.0, 500.0),
        w_pair_array(10.0, &[100.0, 200.0]),
        w_pair(9.0, 600.0),
    ];
    let cid = cid_font(&mut doc, 700.0, w);
    let id = type0_font(&mut doc, cid);
    let d = doc.get_object(id).unwrap().as_dict().unwrap().clone();
    match build_font_info(&doc, &d) {
        FontInfo::Cid { widths, dw } => {
            assert_eq!(dw, 700.0);
            for c in 5..=8 {
                assert_eq!(widths.get(&c), Some(&500.0));
            }
            assert_eq!(widths.get(&10u16), Some(&100.0));
            assert_eq!(widths.get(&11u16), Some(&200.0));
            assert_eq!(widths.get(&9u16), Some(&600.0));
            assert_eq!(widths.len(), 7); // 4（5..=8）+ 2（10,11）+ 1（9）
        }
        _ => panic!("期望 Cid"),
    }
}

#[test]
fn build_font_info缺DescendantFonts() {
    let mut doc = Document::new();
    let dict = dictionary! { "Subtype" => "Type0" };
    let id = doc.add_object(Object::Dictionary(dict));
    let d = doc.get_object(id).unwrap().as_dict().unwrap().clone();
    match build_font_info(&doc, &d) {
        FontInfo::Cid { widths, dw } => {
            assert!(widths.is_empty());
            assert_eq!(dw, 1000.0);
        }
        _ => panic!("期望 Cid"),
    }
}

#[test]
fn resolve_font_id缓存与失败路径() {
    let mut doc = Document::new();
    let f = type1_font(&mut doc, 32, &[500.0]);
    let res = page_resources(&[(b"F1", f)], &[]);
    let mut fonts = HashMap::new();
    assert_eq!(resolve_font_id(&doc, Some(&res), b"F1", &mut fonts), Some(f));
    assert_eq!(fonts.len(), 1);
    // 二次调用命中缓存，不重复解析
    assert_eq!(resolve_font_id(&doc, Some(&res), b"F1", &mut fonts), Some(f));
    assert_eq!(fonts.len(), 1);
    // 名称不存在
    assert_eq!(resolve_font_id(&doc, Some(&res), b"Nope", &mut fonts), None);
    // res 为 None
    assert_eq!(resolve_font_id(&doc, None, b"F1", &mut fonts), None);
    // 条目非间接引用
    let mut f = dictionary! {};
    f.set(b"Subtype", Object::Name(b"Type1".to_vec()));
    let mut fonts_sub = dictionary! {};
    fonts_sub.set(b"F1", Object::Dictionary(f));
    let mut res2 = dictionary! {};
    res2.set(b"Font", Object::Dictionary(fonts_sub));
    assert_eq!(resolve_font_id(&doc, Some(&res2), b"F1", &mut fonts), None);
}
