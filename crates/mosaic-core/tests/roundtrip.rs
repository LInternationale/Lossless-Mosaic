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

#[test]
fn grid_v2_roundtrip_all_sizes_and_more_than_256_cells() {
    use mosaic_core::{protect_grid, GridSelection};
    let src = fixture(257, -129, 24);
    for size in [8, 16, 32, 64] {
        let selection = GridSelection::from_regions(
            257,
            129,
            size,
            &[Rect {
                x: 0,
                y: 0,
                width: 257,
                height: 129,
            }],
        )
        .unwrap();
        for mode in [Mode::Alpha, Mode::Sidecar] {
            let p = protect_grid(&src, "grid", mode, &selection).unwrap();
            let info = inspect(&p.bitmap, p.sidecar.as_deref()).unwrap();
            assert_eq!(info.selection, Some(selection.clone()));
            assert_eq!(info.source_bpp, 24);
            assert_eq!(
                restore(&p.bitmap, p.sidecar.as_deref(), "grid").unwrap(),
                src
            );
        }
    }
}
#[test]
fn grid_repaint_and_erase_rebuild_the_target_without_double_xor() {
    use mosaic_core::{protect_grid, GridSelection};
    let src = fixture(128, -96, 32);
    let first = GridSelection::from_regions(
        128,
        96,
        16,
        &[
            Rect {
                x: 0,
                y: 0,
                width: 64,
                height: 64,
            },
            Rect {
                x: 16,
                y: 16,
                width: 48,
                height: 48,
            },
        ],
    )
    .unwrap();
    assert_eq!(first.rects(128, 96).unwrap().len(), 16);
    let p = protect_grid(&src, "grid", Mode::Sidecar, &first).unwrap();
    let mut next = first.clone();
    next.bits[0] &= !1;
    next.bits[2] |= 1 << 7;
    let recovered = restore(&p.bitmap, p.sidecar.as_deref(), "grid").unwrap();
    let revised = protect_grid(&recovered, "grid", Mode::Sidecar, &next).unwrap();
    assert_eq!(
        inspect(&revised.bitmap, revised.sidecar.as_deref())
            .unwrap()
            .selection,
        Some(next)
    );
    assert_eq!(&revised.bitmap[70..70 + 16 * 4], &src[70..70 + 16 * 4]);
    assert_eq!(
        restore(&revised.bitmap, revised.sidecar.as_deref(), "grid").unwrap(),
        src
    );
    let cleared = protect_grid(
        &src,
        "grid",
        Mode::Sidecar,
        &GridSelection::empty(128, 96, 16).unwrap(),
    )
    .unwrap();
    assert_eq!(cleared.bitmap, src);
    assert!(cleared.sidecar.is_none());
    assert!(restore(&p.bitmap, p.sidecar.as_deref(), "wrong").is_err());
}
#[test]
fn variable_grid_blocks_are_metadata_free_and_can_be_partially_erased() {
    use mosaic_core::{transform_grid, GridSelection};
    let src = fixture(67, -39, 32);
    for size in [8, 16, 32, 64] {
        let old = GridSelection::from_regions(
            67,
            39,
            size,
            &[Rect {
                x: 0,
                y: 0,
                width: 67,
                height: 39,
            }],
        )
        .unwrap();
        let encrypted = transform_grid(&src, "pw", &old).unwrap();
        assert_eq!(encrypted.len(), src.len());
        assert_eq!(transform_grid(&encrypted, "pw", &old).unwrap(), src);
        let erased = GridSelection::from_regions(
            67,
            39,
            size,
            &[Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            }],
        )
        .unwrap();
        let mut target = old.clone();
        for (a, b) in target.bits.iter_mut().zip(&erased.bits) {
            *a &= !*b;
        }
        let partial = transform_grid(&encrypted, "pw", &erased).unwrap();
        assert_eq!(partial, transform_grid(&src, "pw", &target).unwrap());
    }
}
#[test]
fn grid_rejects_invalid_bits_and_tampered_selection() {
    use mosaic_core::{protect_grid, GridSelection};
    assert!(GridSelection::empty(u32::MAX, u32::MAX, 8).is_err());
    assert!(GridSelection {
        block_size: 7,
        bits: vec![1]
    }
    .validate(32, 32)
    .is_err());
    assert!(GridSelection {
        block_size: 32,
        bits: vec![0xff]
    }
    .validate(32, 32)
    .is_err());
    assert!(GridSelection {
        block_size: 8,
        bits: vec![]
    }
    .validate(32, 32)
    .is_err());
    let src = fixture(128, -96, 24);
    let s = GridSelection::from_regions(128, 96, 16, &[R]).unwrap();
    let p = protect_grid(&src, "pw", Mode::Sidecar, &s).unwrap();
    let mut side = p.sidecar.unwrap();
    let text = String::from_utf8(side[..side.len() - 16].to_vec()).unwrap();
    let offset = text.find("\"block_size\":16").unwrap() + 13;
    side[offset] = b'3';
    assert!(restore(&p.bitmap, Some(&side), "pw").is_err());
}

#[test]
fn pixel_masks_encrypt_only_selected_pixels_in_alpha_and_sidecar() {
    use mosaic_core::{protect_grid, GridSelection};
    let legacy_source = fixture(129, -67, 24);
    let legacy = protect(&legacy_source, "legacy", Mode::Sidecar, &[R]).unwrap();
    let legacy_mask = inspect(&legacy.bitmap, legacy.sidecar.as_deref())
        .unwrap()
        .selection
        .unwrap();
    assert_eq!(legacy_mask.block_size, 1);
    assert_eq!(legacy_mask.rects(129, 67).unwrap(), vec![R]);
    for bpp in [24, 32] {
        let src = fixture(129, -67, bpp);
        let selection = GridSelection::from_regions(
            129,
            67,
            1,
            &[
                Rect {
                    x: 17,
                    y: 19,
                    width: 1,
                    height: 1,
                },
                Rect {
                    x: 35,
                    y: 21,
                    width: 7,
                    height: 5,
                },
            ],
        )
        .unwrap();
        for mode in [Mode::Alpha, Mode::Sidecar] {
            let p = protect_grid(&src, "pixels", mode, &selection).unwrap();
            let before = Bitmap::parse(src.clone()).unwrap().rgba();
            let after = Bitmap::parse(p.bitmap.clone()).unwrap().rgba();
            let mut changed = false;
            for y in 0..67 {
                for x in 0..129 {
                    let i = (y * 129 + x) as usize;
                    let selected = selection.bits[i / 8] & (1 << (i % 8)) != 0;
                    if selected {
                        changed |= before[i * 4..i * 4 + 3] != after[i * 4..i * 4 + 3];
                    } else {
                        assert_eq!(
                            &before[i * 4..i * 4 + 3],
                            &after[i * 4..i * 4 + 3],
                            "unselected pixel ({x},{y}) changed"
                        );
                    }
                }
            }
            assert!(changed);
            let restored = restore(&p.bitmap, p.sidecar.as_deref(), "pixels").unwrap();
            if mode == Mode::Alpha && bpp == 32 {
                assert_eq!(Bitmap::parse(restored).unwrap().rgba(), before);
            } else {
                assert_eq!(restored, src);
            }
            assert_eq!(
                inspect(&p.bitmap, p.sidecar.as_deref()).unwrap().selection,
                Some(selection.clone())
            );
        }
    }
    assert!(mosaic_core::transform_grid(
        &fixture(32, 32, 24),
        "pw",
        &GridSelection::empty(32, 32, 1).unwrap()
    )
    .is_err());
}
