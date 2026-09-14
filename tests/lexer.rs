//! Tok 内容流词法器单元测试
#![allow(non_snake_case)]


mod common;

use pdf_crop_dual::{Item, Parsed, Tok, Val};

fn items(data: &str) -> Vec<Item> {
    let mut tk = Tok {
        data: data.as_bytes(),
        pos: 0,
    };
    let mut out = Vec::new();
    while let Some(it) = tk.next_item() {
        out.push(it);
    }
    out
}

fn parse(data: &str) -> Parsed {
    let mut tk = Tok {
        data: data.as_bytes(),
        pos: 0,
    };
    tk.parse_val()
}

fn val(it: &Item) -> &Val {
    match it {
        Item::Val(v) => v,
        Item::Op(_) => panic!("期望 Val，实际是 Op"),
    }
}

fn is_num(v: &Val, x: f32) -> bool {
    matches!(v, Val::Num(n) if (n - x).abs() < 1e-6)
}

fn is_name(v: &Val, n: &[u8]) -> bool {
    matches!(v, Val::Name(x) if x == n)
}

fn is_str(v: &Val, s: &[u8]) -> bool {
    matches!(v, Val::Str(x) if x == s)
}

fn is_op(it: &Item, v: &str) -> bool {
    matches!(it, Item::Op(o) if o == v)
}

#[test]
fn 数字形式() {
    let v = items("123 -4.5 +7 .25 3.");
    assert_eq!(v.len(), 5);
    for (i, x) in v.iter().zip([123.0, -4.5, 7.0, 0.25, 3.0]) {
        assert!(is_num(val(i), x), "数字解析错误: {:?} != {x}", val(i));
    }
}

#[test]
fn 孤立符号是错误() {
    assert!(matches!(parse("-"), Parsed::Err));
    assert!(items("-x").is_empty());
    assert!(items("").is_empty());
    assert!(items("  \n\t ").is_empty());
}

#[test]
fn 名称与十六进制转义() {
    let v = items("/F1 /Co#6Cor");
    assert!(is_name(val(&v[0]), b"F1"));
    assert!(is_name(val(&v[1]), b"Color")); // C o #6C('l') o r
    // % 是注释起始，终止名称
    let v = items("/A%B");
    assert_eq!(v.len(), 1);
    assert!(is_name(val(&v[0]), b"A"));
}

#[test]
fn 字面量串转义() {
    let r = |input: &str, expect: &[u8]| {
        assert!(
            matches!(parse(input), Parsed::Val(Val::Str(s)) if s == expect),
            "输入 {input:?} 未按预期解析"
        );
    };
    r(r"(a\nb)", b"a\nb");
    r(r"(\(\)\\)", b"()\\");
    r(r"(\101)", b"A"); // 八进制
    r(r"(\12)", &[10]); // 八进制 12 = 十进制 10
    r(r"(\8)", b"8"); // 非法八进制退化为原字符
}

#[test]
fn 字面量串行续() {
    assert!(matches!(parse("(a\\\nb)"), Parsed::Val(Val::Str(s)) if s == b"ab"));
    assert!(matches!(parse("(a\\\r\nb)"), Parsed::Val(Val::Str(s)) if s == b"ab"));
}

#[test]
fn 字面量串嵌套括号() {
    assert!(matches!(parse("(a(b)c)"), Parsed::Val(Val::Str(s)) if s == b"a(b)c"));
}

#[test]
fn 字面量串未闭合是错误() {
    assert!(matches!(parse("(abc"), Parsed::Err));
}

#[test]
fn 十六进制串() {
    assert!(matches!(parse("<4849>"), Parsed::Val(Val::Str(s)) if s == b"HI"));
    assert!(matches!(parse("<48 49>"), Parsed::Val(Val::Str(s)) if s == b"HI"));
    // 奇数位补 0：4 → "40" → 0x40
    assert!(matches!(parse("<4>"), Parsed::Val(Val::Str(s)) if s == &[0x40]));
    assert!(matches!(parse("<zz>"), Parsed::Err));
}

#[test]
fn 数组嵌套() {
    let v = items("[1 2.5 (s) /n [3]]");
    assert_eq!(v.len(), 1);
    assert!(matches!(&v[0], Item::Val(Val::Arr(a)) if a.len() == 5
        && is_num(&a[0], 1.0) && is_num(&a[1], 2.5)
        && is_str(&a[2], b"s") && is_name(&a[3], b"n")
        && matches!(&a[4], Val::Arr(b) if b.len() == 1 && is_num(&b[0], 3.0))));
}

#[test]
fn 数组内裸词与字典被丢弃() {
    let v = items("[BT 1 << /A 2 >> 2]");
    assert!(matches!(&v[0], Item::Val(Val::Arr(a)) if a.len() == 2
        && is_num(&a[0], 1.0) && is_num(&a[1], 2.0)));
}

#[test]
fn 字典整体跳过且位置正确() {
    assert!(matches!(
        parse("<< /A 1 /B [1 2] /C (x) /D 5 0 R >>"),
        Parsed::DictSkipped
    ));
    assert!(matches!(parse("<< >>"), Parsed::DictSkipped));
    // 字典后的 token 位置正确
    let v = items("<< /D 5 0 R >> BT");
    assert_eq!(v.len(), 1);
    assert!(is_op(&v[0], "BT"));
}

#[test]
fn 注释() {
    let v = items("% c\n1");
    assert_eq!(v.len(), 1);
    assert!(is_num(val(&v[0]), 1.0));
    // 文件尾无换行注释
    assert_eq!(items("1 % trailing comment").len(), 1);
}

#[test]
fn 混合流顺序() {
    let v = items("1.5 -2 /F1 (abc) [1 2 3] BT");
    assert_eq!(v.len(), 6);
    assert!(is_num(val(&v[0]), 1.5));
    assert!(is_num(val(&v[1]), -2.0));
    assert!(is_name(val(&v[2]), b"F1"));
    assert!(is_str(val(&v[3]), b"abc"));
    assert!(matches!(&v[4], Item::Val(Val::Arr(a)) if a.len() == 3));
    assert!(is_op(&v[5], "BT"));
}

#[test]
fn 内联图像BI_ID_EI() {
    let data = b"BI << /Width 1 /Length 4 >> ID\nwxyz\nEI S";
    let mut tk = Tok { data, pos: 0 };
    assert!(matches!(tk.next_item(), Some(Item::Op(ref o)) if o == "BI"));
    assert!(tk.handle_inline_image());
    assert!(matches!(tk.next_item(), Some(Item::Op(ref o)) if o == "S"));
}

#[test]
fn 内联图像CRLF变体() {
    let data = b"BI << /Length 4 >> ID\r\nwxyz\r\nEI";
    let mut tk = Tok { data, pos: 0 };
    assert!(matches!(tk.next_item(), Some(Item::Op(ref o)) if o == "BI"));
    assert!(tk.handle_inline_image());
    assert!(tk.next_item().is_none());
}

#[test]
fn 内联图像损坏() {
    // 缺 EI
    let data = b"BI << /Length 4 >> ID\nwxyz\nXX";
    let mut tk = Tok { data, pos: 0 };
    let _ = tk.next_item();
    assert!(!tk.handle_inline_image());
    // Length 越界
    let data = b"BI << /Length 100 >> ID\nwxyz EI";
    let mut tk = Tok { data, pos: 0 };
    let _ = tk.next_item();
    assert!(!tk.handle_inline_image());
}
