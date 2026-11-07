//! Installing a new library revision swaps it in for later lookups, while
//! references taken before the swap stay valid. Own process: it replaces the
//! global library, which unit tests in other binaries share.
use ergo_world::part_library;

#[test]
fn a_new_revision_replaces_the_installed_library() {
    let before = part_library::get("3001.dat").unwrap();
    let mut next = part_library::library().clone();
    next.parts.get_mut("3001.dat").unwrap().catalogue.name = "Brick 2 × 4 (next)".into();
    next.revision = part_library::content_revision(&next).unwrap();
    part_library::install_json(&serde_json::to_vec(&next).unwrap()).unwrap();
    assert_eq!(part_library::library().revision, next.revision);
    assert_eq!(
        part_library::get("3001.dat").unwrap().catalogue.name,
        "Brick 2 × 4 (next)"
    );
    assert_eq!(before.catalogue.name, "Brick 2 × 4");
    // The same revision again is a no-op; a tampered document is refused.
    part_library::install_json(&serde_json::to_vec(&next).unwrap()).unwrap();
    next.parts.get_mut("3001.dat").unwrap().catalogue.mass_kg = 1.;
    assert!(part_library::install_json(&serde_json::to_vec(&next).unwrap()).is_err());
}
