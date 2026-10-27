//! Standard LDraw MPD interchange. Native persistence and physical metadata stay
//! in Ergo files; MPD carries visual geometry, colors and metric-correct poses.
use crate::{WorldExport, assembly::Model};
use anyhow::{Context, Result, ensure};
use glam::DVec3;
use std::{collections::BTreeSet, fmt::Write};

fn comment(value: &str) -> String {
    value.replace(['\r', '\n'], " ")
}
fn color(code: u32) -> String {
    if code & 0x0200_0000 != 0 {
        format!("0x2{:06X}", code & 0xffffff)
    } else {
        code.to_string()
    }
}

pub fn mpd(scene: &WorldExport) -> Result<String> {
    let mut bricks = scene.bricks.clone();
    let bounds = bricks.iter().map(|b| b.bounds).reduce(crate::Aabb::union);
    let origin = bounds.map_or(DVec3::ZERO, |b| {
        DVec3::new((b.min.x + b.max.x) / 2., 0., (b.min.z + b.max.z) / 2.)
    });
    for brick in &mut bricks {
        brick.translated(-origin);
    }
    let model = Model::from_bricks(&bricks);
    let refs = model.references()?;
    let mut text = String::new();
    writeln!(
        text,
        "0 FILE construction.ldr\r\n0 {}\r\n0 !LDRAW_ORG Model\r\n0 // Exported from Ergo; horizontal origin centered, original ground at Y=0.\r",
        comment(&scene.name)
    )?;
    let mut used = BTreeSet::new();
    for (reference, brick) in refs.iter().zip(&bricks) {
        ensure!(
            !reference.name.contains(['\r', '\n']),
            "Invalid part filename"
        );
        let code = if !matches!(brick.color, 16 | 24) && crate::color_rgb(brick.color) == brick.rgb
        {
            brick.color
        } else {
            0x0200_0000 | brick.rgb
        };
        let a = reference.transform.to_cols_array_2d();
        let p = reference.position;
        writeln!(
            text,
            "1 {} {} {} {} {} {} {} {} {} {} {} {} {} {}\r",
            color(code),
            p.x,
            p.y,
            p.z,
            a[0][0],
            a[1][0],
            a[2][0],
            a[0][1],
            a[1][1],
            a[2][1],
            a[0][2],
            a[1][2],
            a[2][2],
            reference.name
        )?;
        used.insert(reference.name.clone());
    }
    // Embed each part once, including custom/imported geometry. A recipient
    // does not need Ergo's private cache or a separately installed part library.
    let conversion = crate::ldraw::conversion();
    let inverse = conversion.inverse();
    for part in used {
        let mesh = scene
            .meshes
            .get(&part)
            .context("Missing export part geometry")?;
        ensure!(mesh.vertices.len() % 27 == 0, "Invalid triangle geometry");
        writeln!(
            text,
            "0 FILE {part}\r\n0 Name: {part}\r\n0 !LDRAW_ORG Unofficial_Part\r\n0 // Ergo resolved geometry: {}\r\n0 BFC CERTIFY CCW\r",
            comment(&mesh.quality)
        )?;
        for credit in &mesh.credits {
            writeln!(
                text,
                "0 // Source: {} | Author: {} | License: {}\r",
                comment(&credit.source_url),
                comment(&credit.author),
                comment(&credit.license)
            )?;
        }
        for triangle in mesh.vertices.chunks_exact(27) {
            ensure!(
                triangle.iter().all(|v| v.is_finite()),
                "Nonfinite triangle geometry"
            );
            let vertex = |offset| {
                inverse
                    * DVec3::new(
                        triangle[offset] as f64,
                        triangle[offset + 1] as f64,
                        triangle[offset + 2] as f64,
                    )
            };
            let mut p = [vertex(0), vertex(9), vertex(18)];
            let normal = conversion.transpose()
                * DVec3::new(triangle[3] as f64, triangle[4] as f64, triangle[5] as f64);
            if (p[1] - p[0]).cross(p[2] - p[0]).dot(normal) < 0. {
                p.swap(1, 2);
            }
            let code = if triangle[6] < 0. {
                16
            } else {
                let rgb = (0..3).fold(0u32, |rgb, i| {
                    (rgb << 8) | ((triangle[6 + i].clamp(0., 1.) * 255.).round() as u32)
                });
                0x0200_0000 | rgb
            };
            writeln!(
                text,
                "3 {} {} {} {} {} {} {} {} {} {}\r",
                color(code),
                p[0].x,
                p[0].y,
                p[0].z,
                p[1].x,
                p[1].y,
                p[1].z,
                p[2].x,
                p[2].y,
                p[2].z
            )?;
        }
    }
    writeln!(text, "0 NOFILE\r")?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_mpd_roundtrips_metric_poses_and_embeds_custom_parts_and_colors() {
        let mut demo = crate::demo::Demo::new();
        demo.add("3003", 4, 100., 0., 200.);
        let mut scene = WorldExport {
            schema: crate::SCHEMA_VERSION,
            name: "Export\nname".into(),
            geo: Default::default(),
            bricks: demo.bricks,
            meshes: demo.meshes,
            joints: vec![],
            fixed_bricks: vec![],
            groups: vec![],
            model: None,
            ground: vec![],
        };
        scene.bricks[0].rgb = 0x123456;
        let original = scene.meshes.remove("3003.dat").unwrap();
        let mut custom = original;
        custom.part = "custom-sourced.dat".into();
        scene.bricks[0].part = custom.part.clone();
        scene.meshes.insert(custom.part.clone(), custom);
        let text = mpd(&scene).unwrap();
        assert!(
            text.contains("1 0x2123456 "),
            "{}",
            text.lines().take(6).collect::<Vec<_>>().join("\n")
        );
        assert!(text.contains("0 FILE custom-sourced.dat"));
        assert!(text.lines().any(|line| line.starts_with("3 ")));
        let document = crate::ldraw::Document::parse(&text, "construction.mpd").unwrap();
        let model = Model::from_document(&document).unwrap();
        let refs = model.references().unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "custom-sourced.dat");
        assert_eq!(refs[0].color, 0x0200_0000 | 0x123456);
        assert!((refs[0].position - DVec3::new(0., -24., 0.)).length() < 1e-9);
        assert!(crate::physical::is_proper_rotation(refs[0].transform));
    }
}
