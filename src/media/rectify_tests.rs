//! Tests of `rectify.rs`: the map hits the corners, a page drawn in perspective comes out upright with its
//! lines straight, sizes follow the longer edges and are never enlarged.

use super::*;
use crate::media::test_pictures::plain;

#[test]
fn the_map_takes_each_corner_to_its_partner() {
    let from = [(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)];
    let to = [(10.0, 20.0), (130.0, 5.0), (120.0, 90.0), (15.0, 70.0)];
    let h = homography(from, to).unwrap();
    for (f, t) in from.iter().zip(to) {
        let (x, y) = apply(&h, f64::from(f.0), f64::from(f.1));
        assert!((x - f64::from(t.0)).abs() < 1e-6 && (y - f64::from(t.1)).abs() < 1e-6);
    }
    assert!(homography(from, [(0.0, 0.0), (1.0, 1.0), (2.0, 2.0), (3.0, 3.0)]).is_none());
}

#[test]
fn a_page_in_perspective_comes_out_upright() {
    // Draw a 400 × 200 page with a dark band in its top quarter through a known map into a 600 × 400 picture.
    let rect = [(0.0, 0.0), (400.0, 0.0), (400.0, 200.0), (0.0, 200.0)];
    let corners = [(120.0, 60.0), (500.0, 30.0), (540.0, 330.0), (90.0, 300.0)];
    let to_page = homography(corners, rect).unwrap();
    let mut b = plain(600, 400, [0, 0, 0]);
    for y in 0..400u32 {
        for x in 0..600u32 {
            let (u, v) = apply(&to_page, f64::from(x) + 0.5, f64::from(y) + 0.5);
            if (0.0..400.0).contains(&u) && (0.0..200.0).contains(&v) {
                let c = if v < 50.0 { 20 } else { 230 };
                let i = (y as usize * 600 + x as usize) * 3;
                b.rgb[i..i + 3].copy_from_slice(&[c, c, c]);
            }
        }
    }
    let q = Quad { corners };
    let out = rectify(&b.picture(), &q, None).unwrap();
    let (w, h) = (out.width, out.height);
    assert_eq!((w, h), output_size(&q, None));
    let p = out.picture();
    // The band is straight: the same rows dark across the whole width, the rest light.
    for x in [w / 10, w / 2, w * 9 / 10] {
        assert!(p.px(x, h / 10)[0] < 80, "x {x} top");
        assert!(p.px(x, h * 6 / 10)[0] > 180, "x {x} middle");
        assert!(p.px(x, h * 9 / 10)[0] > 180, "x {x} bottom");
    }
}

#[test]
fn size_follows_the_longer_edges_and_is_never_enlarged() {
    let q = Quad { corners: [(0.0, 0.0), (300.0, 0.0), (280.0, 100.0), (20.0, 120.0)] };
    assert_eq!(output_size(&q, None), (300, 122));
    assert_eq!(output_size(&q, Some(150)), (150, 61));
    assert_eq!(output_size(&q, Some(1280)), (300, 122));
}
