//! get_mediabox / get_resources / page_resources_dict 单元测试
#![allow(non_snake_case)]


mod common;

use common::page_tree;
use lopdf::{dictionary, Document, Dictionary, Object};
use pdf_crop_dual::{get_mediabox, get_resources, page_resources_dict};

fn real_vec(xs: &[f32]) -> Object {
    Object::Array(xs.iter().map(|x| Object::Real(*x)).collect())
}

fn int_vec(xs: &[i32]) -> Object {
    Object::Array(xs.iter().map(|x| Object::Integer(*x as i64)).collect())
}

#[test]
fn mediabox页内Real() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "MediaBox" => real_vec(&[0.0, 0.0, 1008.0, 661.5]) },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(
        get_mediabox(&doc, d, page_id).unwrap(),
        vec![0.0, 0.0, 1008.0, 661.5]
    );
}

#[test]
fn mediabox页内Integer兼容() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "MediaBox" => int_vec(&[0, 0, 1008, 661]) },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(get_mediabox(&doc, d, page_id).unwrap(), vec![0.0, 0.0, 1008.0, 661.0]);
}

#[test]
fn mediabox从Parent继承() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page" },
        dictionary! { "MediaBox" => real_vec(&[10.0, 20.0, 300.0, 400.0]) },
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(
        get_mediabox(&doc, d, page_id).unwrap(),
        vec![10.0, 20.0, 300.0, 400.0]
    );
}

#[test]
fn mediabox缺失报错() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(&mut doc, dictionary! { "Type" => "Page" }, Dictionary::new());
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(get_mediabox(&doc, d, page_id).is_err());
}

#[test]
fn mediabox含非数字报错() {
    let mut doc = Document::new();
    let arr = Object::Array(vec![
        Object::Integer(0),
        Object::Integer(0),
        Object::Integer(100),
        Object::Name(b"Bad".to_vec()),
    ]);
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "MediaBox" => arr },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    let e = get_mediabox(&doc, d, page_id).unwrap_err();
    assert!(e.to_string().contains("非数字值"), "实际: {e}");
}

#[test]
fn resources页内内联() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => dictionary! { "Font" => dictionary! {} } },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(get_resources(&doc, d, page_id).unwrap().as_dict().unwrap().has(b"Font"));
}

#[test]
fn resources经引用() {
    let mut doc = Document::new();
    let res_id = doc.add_object(Object::Dictionary(dictionary! { "Font" => dictionary! {} }));
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => Object::Reference(res_id) },
        Dictionary::new(),
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(get_resources(&doc, d, page_id).unwrap().as_dict().unwrap().has(b"Font"));
}

#[test]
fn resources从Parent继承() {
    let mut doc = Document::new();
    let (page_id, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page" },
        dictionary! { "Resources" => dictionary! { "Font" => dictionary! {} } },
    );
    let d = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(get_resources(&doc, d, page_id).unwrap().as_dict().unwrap().has(b"Font"));
}

#[test]
fn page_resources_dict三种情形() {
    let mut doc = Document::new();
    // 内联字典
    let (p1, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => dictionary! { "Font" => dictionary! {} } },
        Dictionary::new(),
    );
    let d1 = doc.get_object(p1).unwrap().as_dict().unwrap();
    assert!(page_resources_dict(&doc, d1, p1).unwrap().has(b"Font"));
    // 间接引用
    let res_id = doc.add_object(Object::Dictionary(dictionary! { "Font" => dictionary! {} }));
    let (p2, _) = page_tree(
        &mut doc,
        dictionary! { "Type" => "Page", "Resources" => Object::Reference(res_id) },
        Dictionary::new(),
    );
    let d2 = doc.get_object(p2).unwrap().as_dict().unwrap();
    assert!(page_resources_dict(&doc, d2, p2).unwrap().has(b"Font"));
    // 缺失
    let (p3, _) = page_tree(&mut doc, dictionary! { "Type" => "Page" }, Dictionary::new());
    let d3 = doc.get_object(p3).unwrap().as_dict().unwrap();
    assert!(page_resources_dict(&doc, d3, p3).is_none());
}
