//! detect_gap 中间空白检测单元测试

mod common;

use pdf_crop_dual::detect_gap;

const X1: f32 = 0.0;
const X2: f32 = 1008.0; // 中线 504

#[test]
fn 正常双栏() {
    // 左最大右缘 400 → +2；右最小左缘 560 → -2
    assert_eq!(
        detect_gap(&[(72.0, 400.0), (560.0, 900.0)], X1, X2),
        Some((402.0, 558.0))
    );
}

#[test]
fn 跨中线无空白() {
    assert_eq!(detect_gap(&[(400.0, 600.0)], X1, X2), None);
}

#[test]
fn 余量后不足10pt无空白() {
    // l = 500+2 = 502, r = 510-2 = 508 → 宽 6 < 10
    assert_eq!(detect_gap(&[(0.0, 500.0), (510.0, 800.0)], X1, X2), None);
}

#[test]
fn 余量后恰好10pt保留() {
    // l = 496+2 = 498, r = 510-2 = 508 → 宽 10，不 < 10
    assert_eq!(
        detect_gap(&[(0.0, 496.0), (510.0, 800.0)], X1, X2),
        Some((498.0, 508.0))
    );
}

#[test]
fn 单边内容与空列表无空白() {
    assert_eq!(detect_gap(&[(0.0, 400.0)], X1, X2), None);
    assert_eq!(detect_gap(&[(600.0, 900.0)], X1, X2), None);
    assert_eq!(detect_gap(&[], X1, X2), None);
}

#[test]
fn 贴中线边界归属() {
    // b == mid 归左侧、a == mid 归右侧（严格不等式才判跨线）
    // l = 504+2 = 506, r = 504-2 = 502 → 负宽 < 10 → None
    assert_eq!(detect_gap(&[(400.0, 504.0), (504.0, 600.0)], X1, X2), None);
}

#[test]
fn 多区间取极值且与顺序无关() {
    let v = detect_gap(
        &[(560.0, 900.0), (72.0, 400.0), (100.0, 380.0), (700.0, 950.0)],
        X1,
        X2,
    )
    .unwrap();
    assert_eq!(v, (402.0, 558.0));
}
