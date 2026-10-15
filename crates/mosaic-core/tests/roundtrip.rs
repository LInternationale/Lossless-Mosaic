use mosaic_core::{inspect, protect, restore, transform_blocks, Bitmap, Mode, Rect};
fn fixture(w: u32, h: i32, bpp: u16) -> Vec<u8> {
    let stride = (w as usize * bpp as usize).div_ceil(32) * 4;
    let size = 70 + stride * h.unsigned_abs() as usize + 9;
    let mut b = vec![0xa7; size];
    b[..2].copy_from_slice(b"BM");
    b[2..6].copy_from_slice(&(size as u32).to_le_bytes());
    b[10..14].copy_from_slice(&70u32.to_le_bytes());
    b[14..18].copy_from_slice(&40u32.to_le_bytes());
    b[18..22].copy_from_slice(&w.to_le_bytes());
    b[22..26].copy_from_slice(&h.to_le_bytes());
    b[26..28].copy_from_slice(&1u16.to_le_bytes());
    b[28..30].copy_from_slice(&bpp.to_le_bytes());
    b[30..34].fill(0);
    for (i, v) in b[70..size - 9].iter_mut().enumerate() {
        *v = ((i * 13 + i / 7) % 256) as u8;
    }
    b
}
const R: Rect = Rect {
    x: 5,
    y: 3,
    width: 21,
    height: 17,
};
#[test]
fn sidecar_exact_both_orientations_and_depths() {
    for bpp in [24, 32] {
        for h in [39, -39] {
            let original = fixture(67, h, bpp);
            let p = protect(&original, "测试🔒 password", Mode::Sidecar, &[R]).unwrap();
            assert_ne!(p.bitmap, original);
            assert_eq!(p.bitmap.len(), original.len());
            assert_eq!(
                restore(&p.bitmap, p.sidecar.as_deref(), "测试🔒 password").unwrap(),
                original
            );
        }
    }
}
#[test]
fn alpha_24_exact_headers_padding_and_tail() {
    for h in [81, -81] {
        let original = fixture(101, h, 24);
        let p = protect(&original, "alpha", Mode::Alpha, &[R]).unwrap();
        assert!(p.sidecar.is_none());
        assert_eq!(inspect(&p.bitmap, None).unwrap().mode, Some(Mode::Alpha));
        assert_eq!(restore(&p.bitmap, None, "alpha").unwrap(), original);
    }
}
#[test]
fn alpha_32_preserves_rgb_but_discards_original_alpha() {
    let original = fixture(101, -81, 32);
    let p = protect(&original, "alpha", Mode::Alpha, &[R]).unwrap();
    let result = restore(&p.bitmap, None, "alpha").unwrap();
    assert_ne!(result, original);
    assert_eq!(
        Bitmap::parse(result.clone()).unwrap().rgba(),
        Bitmap::parse(original.clone()).unwrap().rgba()
    );
    for y in 0..81usize {
        for x in 0..101usize {
            assert_eq!(result[70 + y * 404 + x * 4 + 3], 255);
        }
    }
    assert_eq!(&result[..70], &original[..70]);
    assert_eq!(&result[result.len() - 9..], &original[original.len() - 9..]);
}
#[test]
fn wrong_password_and_tampering_rejected() {
    let p = protect(&fixture(80, 80, 32), "right", Mode::Sidecar, &[R]).unwrap();
    assert!(restore(&p.bitmap, p.sidecar.as_deref(), "wrong").is_err());
    let mut changed = p.bitmap.clone();
    changed[100] ^= 1;
    assert!(restore(&changed, p.sidecar.as_deref(), "right").is_err());
    let mut side = p.sidecar.unwrap();
    side[40] ^= 1;
    assert!(restore(&p.bitmap, Some(&side), "right").is_err());
}
#[test]
fn sidecar_encrypts_alpha_and_leaves_outside_untouched() {
    let src = fixture(67, -39, 32);
    let p = protect(&src, "pass", Mode::Sidecar, &[R]).unwrap();
    let mut alpha_changed = false;
    for y in 0..39usize {
        for x in 0..67usize {
            let i = 70 + y * 268 + x * 4;
            let inside = (5..26).contains(&x) && (3..20).contains(&y);
            if inside {
                alpha_changed |= src[i + 3] != p.bitmap[i + 3];
            } else {
                assert_eq!(&src[i..i + 4], &p.bitmap[i..i + 4]);
            }
        }
    }
    assert!(alpha_changed);
}
#[test]
fn blocks_no_added_bytes_and_exact_manual_recovery() {
    for bpp in [24, 32] {
        for h in [65, -65] {
            let src = fixture(67, h, bpp);
            let rs = [
                R,
                Rect {
                    x: 66,
                    y: 64,
                    width: 1,
                    height: 1,
                },
            ];
            let enc = transform_blocks(&src, "short", &rs).unwrap();
            assert_eq!(enc.len(), src.len());
            assert_eq!(&enc[..70], &src[..70]);
            assert_eq!(&enc[enc.len() - 9..], &src[src.len() - 9..]);
            assert_eq!(transform_blocks(&enc, "short", &rs).unwrap(), src);
            assert_eq!(inspect(&enc, None).unwrap().mode, None);
            assert_ne!(transform_blocks(&enc, "wrong", &rs).unwrap(), src);
        }
    }
}
#[test]
fn block_heuristic_suggests_ciphertext() {
    let mut src = fixture(96, -64, 32);
    src[70..70 + 96 * 64 * 4].fill(100);
    let rs = [Rect {
        x: 0,
        y: 0,
        width: 32,
        height: 32,
    }];
    let p = protect(&src, "pw", Mode::Entropy, &rs).unwrap();
    assert!(p.sidecar.is_none());
    assert_eq!(p.bitmap.len(), src.len());
    assert_eq!(inspect(&p.bitmap, None).unwrap().suggested_blocks, rs);
}
#[test]
fn invalid_regions_and_small_alpha_rejected() {
    let src = fixture(16, 16, 24);
    assert!(protect(
        &src,
        "x",
        Mode::Alpha,
        &[Rect {
            x: 0,
            y: 0,
            width: 2,
            height: 2
        }]
    )
    .is_err());
    assert!(protect(&src, "x", Mode::Sidecar, &[R]).is_err());
    assert!(protect(&src, "x", Mode::Sidecar, &[]).is_err());
    assert!(transform_blocks(
        &src,
        "",
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1
        }]
    )
    .is_err());
    let src = fixture(80, 80, 24);
    assert!(protect(&src, "x", Mode::Sidecar, &[R, R]).is_err());
}
#[test]
fn malformed_bmps_do_not_panic() {
    for n in 0..200 {
        let mut b = vec![0xff; n];
        if n >= 2 {
            b[..2].copy_from_slice(b"BM");
        }
        assert!(Bitmap::parse(b).is_err());
    }
    let src = fixture(80, 80, 24);
    let mut invalid_height = src.clone();
    invalid_height[22..26].copy_from_slice(&i32::MIN.to_le_bytes());
    assert!(Bitmap::parse(invalid_height).is_err());
    for offset in [10, 14, 18, 26, 28, 30] {
        let mut b = src.clone();
        b[offset..offset + 4].fill(0xff);
        assert!(Bitmap::parse(b).is_err());
    }
}
#[test]
fn fresh_salts_make_distinct_authenticated_outputs() {
    let src = fixture(80, 80, 24);
    let a = protect(&src, "pw", Mode::Sidecar, &[R]).unwrap();
    let b = protect(&src, "pw", Mode::Sidecar, &[R]).unwrap();
    assert_ne!(a.bitmap, b.bitmap);
    assert!(restore(&a.bitmap, b.sidecar.as_deref(), "pw").is_err());
}
