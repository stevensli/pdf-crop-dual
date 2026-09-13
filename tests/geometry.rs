//! Mat 矩阵、x_extents、clip_intervals_to_bbox 单元测试

mod common;

use pdf_crop_dual::{clip_intervals_to_bbox, x_extents, Mat};

#[test]
fn 单位矩阵保持坐标() {
    assert_eq!(Mat::I.x_of(3.0, 4.0), 3.0);
}

#[test]
fn 平移矩阵移动点() {
    let m = Mat::translate(10.0, 5.0);
    assert_eq!(m.x_of(3.0, 4.0), 13.0);
}

#[test]
fn mul先应用m1再应用m2() {
    let scale2 = Mat::of(2.0, 0.0, 0.0, 2.0, 0.0, 0.0);
    // 先平移 10 再缩放 2：x' = 2(x+10) = 2x + 20
    let m = Mat::mul(Mat::translate(10.0, 0.0), scale2);
    assert_eq!(m.x_of(1.0, 0.0), 22.0);
    // 先缩放 2 再平移 10：x' = 2x + 10
    let m = Mat::mul(scale2, Mat::translate(10.0, 0.0));
    assert_eq!(m.x_of(1.0, 0.0), 12.0);
}

#[test]
fn mul平移先于旋转() {
    let r90 = Mat::of(0.0, -1.0, 1.0, 0.0, 0.0, 0.0);
    let m = Mat::mul(Mat::translate(10.0, 30.0), r90);
    // 先平移：(x+10, y+30)；再按该矩阵映射 (u, v) → (v, -u)：x' = y + 30
    // e' 中的 m1.f*m2.c 项（30·1）在此首次非零，漏写即失败
    assert_eq!(m.x_of(0.0, 0.0), 30.0);
    assert_eq!(m.x_of(1.0, 2.0), 32.0);
}

#[test]
fn 平移合成() {
    let m = Mat::mul(Mat::translate(1.0, 2.0), Mat::translate(3.0, 4.0));
    assert_eq!(m.x_of(0.0, 0.0), 4.0);
}

#[test]
fn 旋转矩阵交换轴() {
    // 90° 旋转（该矩阵实现映射 (x, y) → (y, -x)），故 x' = y
    let m = Mat::of(0.0, -1.0, 1.0, 0.0, 0.0, 0.0);
    assert_eq!(m.x_of(3.0, 7.0), 7.0);
    assert_eq!(m.x_of(-2.0, 5.0), 5.0);
}

#[test]
fn x_extents缩放与平移() {
    let m = Mat::of(2.0, 0.0, 0.0, 2.0, 5.0, 0.0);
    let (lo, hi) = x_extents(m, &[(1.0, 0.0), (3.0, 0.0), (2.0, 5.0)]);
    assert_eq!(lo, 7.0); // 2*1 + 5
    assert_eq!(hi, 11.0); // 2*3 + 5
}

#[test]
fn x_extents负缩放min_max正确互换() {
    let m = Mat::of(-1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    let (lo, hi) = x_extents(m, &[(1.0, 0.0), (3.0, 0.0), (2.0, 0.0)]);
    assert_eq!(lo, -3.0);
    assert_eq!(hi, -1.0);
}

#[test]
fn clip钳位跨边区间并剔除区间外者() {
    let mut iv = vec![(5.0, 15.0), (100.0, 200.0), (20.0, 30.0)];
    clip_intervals_to_bbox(&mut iv, 0, 0.0, 10.0);
    assert_eq!(iv, vec![(5.0, 10.0)]);
}

#[test]
fn clip左缘钳位与左外侧剔除() {
    // 左缘钳位（a < bx0）、左侧完全在外剔除、框内原样通过、双缘同时钳位
    let mut iv = vec![(-5.0, 3.0), (-20.0, -5.0), (2.0, 8.0), (-5.0, 100.0)];
    clip_intervals_to_bbox(&mut iv, 0, 0.0, 10.0);
    assert_eq!(iv, vec![(0.0, 3.0), (2.0, 8.0), (0.0, 10.0)]);
}

#[test]
fn clip只处理mark之后的区间() {
    // 前缀区间刻意选用裁剪下会变化的形态：忽略 mark（全部裁剪）的实现会失败
    let mut iv = vec![(5.0, 15.0), (5.0, 15.0)];
    clip_intervals_to_bbox(&mut iv, 1, 0.0, 10.0);
    assert_eq!(iv, vec![(5.0, 15.0), (5.0, 10.0)]);
}

#[test]
fn clip退化区间被丢弃() {
    let mut iv = vec![(10.0, 10.0), (12.0, 8.0)];
    clip_intervals_to_bbox(&mut iv, 0, 0.0, 10.0);
    assert!(iv.is_empty());
}
