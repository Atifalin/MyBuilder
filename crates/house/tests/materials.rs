use mb_house::materials::{SLOTS, TEX, texture_png, texture_rgba};

fn px(img: &[u8], x: usize, y: usize) -> i32 {
    img[(y * TEX + x) * 4] as i32
}

#[test]
fn textures_are_deterministic_and_golden() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/gates/house");
    std::fs::create_dir_all(&dir).unwrap();
    for s in SLOTS {
        let png = texture_png(s);
        assert_eq!(png, texture_png(s));
        assert_eq!(&png[1..4], b"PNG");
        std::fs::write(dir.join(format!("tex_{}.png", s.name())), &png).unwrap();
        mb_seed::golden::check_hash(
            env!("CARGO_MANIFEST_DIR"),
            &format!("texture_{}", s.name()),
            &texture_rgba(s),
        );
    }
}

#[test]
fn textures_tile_seamlessly_and_stay_light() {
    for s in SLOTS {
        let img = texture_rgba(s);
        let (mut seam, mut inner) = (0i64, 0i64);
        for i in 0..TEX {
            seam += (px(&img, TEX - 1, i) - px(&img, 0, i)).abs() as i64;
            seam += (px(&img, i, TEX - 1) - px(&img, i, 0)).abs() as i64;
            for j in 0..TEX - 1 {
                inner += (px(&img, j, i) - px(&img, j + 1, i)).abs() as i64;
                inner += (px(&img, i, j) - px(&img, i, j + 1)).abs() as i64;
            }
        }
        let seam_mean = seam as f64 / (2 * TEX) as f64;
        let inner_mean = inner as f64 / (2 * TEX * (TEX - 1)) as f64;
        assert!(
            seam_mean <= 1.5 * inner_mean + 0.5,
            "{}: seam {seam_mean:.2} vs inner {inner_mean:.2}",
            s.name()
        );
        let mean =
            (0..TEX * TEX).map(|i| img[i * 4] as f64).sum::<f64>() / (TEX * TEX) as f64 / 255.0;
        assert!((0.8..=0.97).contains(&mean), "{}: mean {mean:.3}", s.name());
        println!(
            "{}: mean {mean:.3} seam {seam_mean:.2} inner {inner_mean:.2}",
            s.name()
        );
    }
}
