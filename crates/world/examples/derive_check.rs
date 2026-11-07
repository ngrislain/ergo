//! Compare automatic `derive` profiles with every reviewed library entry.
//! Usage: cargo run -p ergo-world --example derive_check -- /path/to/ldraw [part.dat ...]
use ergo_world::{derive, geometry, part_library, part_profiles};
use glam::DVec3;
use std::path::{Path, PathBuf};

fn resolver(root: &Path) -> impl Fn(&str) -> Option<String> + '_ {
    move |name: &str| {
        ["parts", "p"].iter().find_map(|dir| {
            let path: PathBuf = root.join(dir).join(name);
            std::fs::read_to_string(path)
                .ok()
                .map(|t| t.trim_start_matches('\u{feff}').replace("\r\n", "\n"))
        })
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().expect("LDraw root"));
    let only: Vec<String> = args.collect();
    let resolve = resolver(&root);
    let (mut studs_ok, mut grids_ok, mut bounds_ok, mut total) = (0, 0, 0, 0);
    let mut ratios = Vec::new();
    for entry in part_library::reviewed_parts() {
        let id = &entry.catalogue.id;
        if !only.is_empty() && !only.contains(id) {
            continue;
        }
        total += 1;
        let derived = match derive::derive(id, &resolve) {
            Ok(d) => d,
            Err(e) => {
                println!("{id}: ERROR {e:#}");
                continue;
            }
        };
        let c = ergo_world::ldraw::conversion();
        let reviewed = geometry::nominal_body_bounds(id).unwrap();
        let body = ergo_world::Aabb::new(
            DVec3::from_array(derived.nominal.bounds_ldu[0]),
            DVec3::from_array(derived.nominal.bounds_ldu[1]),
        )
        .transformed(c, DVec3::ZERO);
        let bounds_match = (body.min - reviewed.min).abs().max_element() < 0.06
            && (body.max - reviewed.max).abs().max_element() < 0.06;
        let mut want: Vec<[i64; 2]> = part_profiles_studs(id);
        let mut got: Vec<[i64; 2]> = derived
            .nominal
            .studs_ldu
            .iter()
            .map(|p| {
                let n = c * DVec3::from_array(*p);
                [(n.x * 100.).round() as i64, (n.z * 100.).round() as i64]
            })
            .collect();
        want.sort();
        got.sort();
        let studs_match = want == got;
        let grid_cells = |grids: Vec<part_profiles::Grid>| {
            let mut cells: Vec<[i64; 3]> = grids
                .iter()
                .flat_map(|g| {
                    (0..g.width).flat_map(move |i| {
                        (0..g.depth).map(move |j| {
                            [
                                ((g.first[0] + f64::from(i)) * 100.).round() as i64,
                                ((g.first[1] + f64::from(j)) * 100.).round() as i64,
                                (g.bottom * 100.).round() as i64,
                            ]
                        })
                    })
                })
                .collect();
            cells.sort();
            cells
        };
        let want_grids = grid_cells(ergo_world::part_profiles::receiving_grids(id));
        let got_grids = grid_cells(part_profiles::nominal_grids(&derived.nominal));
        let grids_match = want_grids == got_grids;
        if !only.is_empty() {
            println!("want {want_grids:?}\n got {got_grids:?}");
        }
        let ratio = derived.abs_mass_kg() / entry.catalogue.mass_kg;
        ratios.push(ratio);
        studs_ok += usize::from(studs_match);
        grids_ok += usize::from(grids_match);
        bounds_ok += usize::from(bounds_match);
        if !(studs_match && grids_match && bounds_match) || !only.is_empty() {
            println!(
                "{id} {:?}: studs {}/{} {}, grids {} (want {:?} got {:?}), bounds {} (want {:?} got {:?}), mass ratio {ratio:.2}",
                entry.catalogue.name,
                got.len(),
                want.len(),
                if studs_match { "ok" } else { "DIFF" },
                if grids_match { "ok" } else { "DIFF" },
                want_grids.len(),
                got_grids.len(),
                if bounds_match { "ok" } else { "DIFF" },
                (reviewed.min, reviewed.max),
                (body.min, body.max),
            );
        }
    }
    ratios.sort_by(f64::total_cmp);
    let median = ratios.get(ratios.len() / 2).copied().unwrap_or(f64::NAN);
    let spread: Vec<f64> = ratios.iter().map(|r| (r / median - 1.).abs()).collect();
    let mut spread_sorted = spread.clone();
    spread_sorted.sort_by(f64::total_cmp);
    println!(
        "{total} parts: studs {studs_ok}, grids {grids_ok}, bounds {bounds_ok}; mass ratio median {median:.3}, |err| after calibration p50 {:.2} p90 {:.2} max {:.2}",
        spread_sorted
            .get(spread_sorted.len() / 2)
            .copied()
            .unwrap_or(f64::NAN),
        spread_sorted
            .get(spread_sorted.len() * 9 / 10)
            .copied()
            .unwrap_or(f64::NAN),
        spread_sorted.last().copied().unwrap_or(f64::NAN),
    );
}

fn part_profiles_studs(id: &str) -> Vec<[i64; 2]> {
    ergo_world::part_profiles::stud_centres(id)
        .iter()
        .map(|p| [(p.x * 100.).round() as i64, (p.z * 100.).round() as i64])
        .collect()
}
