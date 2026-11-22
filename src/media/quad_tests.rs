//! Tests of `quad.rs`: a page in perspective on a darker table, a projection screen with a person in
//! front, the whole picture as a page, and pictures without an outline.

use super::*;
use crate::media::test_pictures::{fill, inside, plain, scene};
use crate::media::RgbBuf;

/// A bright page with dark lines at `corners` on a ground of `ground`.
fn page_on(w: u32, h: u32, corners: [(f32, f32); 4], ground: [u8; 3]) -> RgbBuf {
    let mut b = plain(w, h, ground);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if inside(&corners, fx, fy) {
                let line = (y / 6) % 3 == 0 && (x / 9) % 5 != 0;
                let c = if line { [40, 40, 40] } else { [235, 232, 225] };
                let i = (y as usize * w as usize + x as usize) * 3;
                b.rgb[i..i + 3].copy_from_slice(&c);
            }
        }
    }
    b
}

fn close(a: (f32, f32), b: (f32, f32), tol: f32) -> bool {
    (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol
}

#[test]
fn a_page_photographed_at_an_angle_is_found() {
    let corners = [(210.0, 90.0), (820.0, 140.0), (760.0, 700.0), (150.0, 620.0)];
    let b = page_on(1000, 760, corners, [90, 70, 50]);
    let q = find_quad(&b.picture()).expect("page found");
    for (got, want) in q.corners.iter().zip(corners) {
        assert!(close(*got, want, 8.0), "{:?} vs {:?}", q.corners, corners);
    }
    assert!(!q.covers(1000, 760));
}

#[test]
fn a_screen_with_a_person_in_front_is_still_found() {
    let corners = [(300.0, 60.0), (980.0, 90.0), (960.0, 470.0), (320.0, 440.0)];
    let mut b = page_on(1280, 720, corners, [70, 60, 55]);
    // A speaker standing in front of the lower left part of the screen.
    fill(&mut b, (360, 300, 440, 720), [30, 30, 60]);
    let q = find_quad(&b.picture()).expect("screen found");
    for (got, want) in q.corners.iter().zip(corners) {
        assert!(close(*got, want, 10.0), "{:?} vs {:?}", q.corners, corners);
    }
}

#[test]
fn a_page_filling_the_picture_is_never_a_smaller_outline() {
    let corners = [(4.0, 3.0), (636.0, 2.0), (637.0, 478.0), (2.0, 477.0)];
    let mut b = page_on(640, 480, corners, [20, 20, 20]);
    fill(&mut b, (0, 0, 640, 1), [20, 20, 20]);
    // Either no outline (the caller reads the whole picture) or one covering it.
    if let Some(q) = find_quad(&b.picture()) {
        assert!(q.covers(640, 480), "{q:?}");
    }
}

#[test]
fn no_outline_in_noise_in_a_round_spot_or_in_one_colour() {
    assert_eq!(find_quad(&scene(640, 360, 1, 0).picture()), None);
    let mut spot = plain(640, 360, [30, 30, 30]);
    for y in 0..360u32 {
        for x in 0..640u32 {
            let (dx, dy) = (x as f32 - 320.0, y as f32 - 180.0);
            if dx.hypot(dy) < 150.0 {
                fill(&mut spot, (x, y, x + 1, y + 1), [240, 240, 240]);
            }
        }
    }
    assert_eq!(find_quad(&spot.picture()), None);
    assert_eq!(find_quad(&plain(64, 48, [200, 200, 200]).picture()), None);
}

#[test]
fn corners_in_any_order_are_sorted_clockwise_from_top_left() {
    let q = Quad::from_points([(10.0, 90.0), (90.0, 10.0), (12.0, 8.0), (95.0, 88.0)]);
    assert_eq!(q.corners, [(12.0, 8.0), (90.0, 10.0), (95.0, 88.0), (10.0, 90.0)]);
    let s = Quad::from_shares([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], 200, 100);
    assert_eq!(s.area(), 20_000.0);
    assert!(s.covers(200, 100));
    // A slide with a speaker inset cutting off its bottom right corner: three corners are the picture's.
    let cut = Quad { corners: [(1.0, 1.0), (199.0, 2.0), (150.0, 60.0), (2.0, 99.0)] };
    assert!(cut.area() < COVERS_SHARE * 20_000.0 && cut.covers(200, 100));
    // A screen inside the picture.
    let inner = Quad { corners: [(20.0, 10.0), (180.0, 12.0), (178.0, 90.0), (22.0, 88.0)] };
    assert!(!inner.covers(200, 100));
    assert!(inner.plausible(200, 100));
    // A receipt running off the picture: its extended sides cross outside.
    let off = Quad { corners: [(50.0, 2.0), (150.0, 2.0), (160.0, 99.0), (-15.0, 92.0)] };
    assert!(!off.plausible(200, 100));
    assert!(inner.contains((100.0, 50.0)) && !inner.contains((10.0, 50.0)));
}

