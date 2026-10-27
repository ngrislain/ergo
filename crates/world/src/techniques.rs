//! Curated, attributed building guidance. These patterns do not certify a model.
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub const SOURCE: &str = "https://brickbuildershandbook.com/basic-lego-techniques/";

pub fn library(query: Option<&str>, category: Option<&str>, id: Option<&str>) -> Result<Value> {
    let entries = [
        (
            "running-bond",
            "Running-bond walls",
            "walls",
            "chalet house palace walls brickwork",
            vec!["3001.dat", "3003.dat", "2456.dat"],
            false,
            vec![
                "Overlap joints in adjacent courses; a 2x4 course typically shifts by two studs.",
                "Use shorter end bricks to retain the planned wall footprint and openings.",
            ],
            "Stacks joined only at their vertical sides are not bonded together.",
        ),
        (
            "interlocked-corner",
            "Interlocked 90-degree corners",
            "junctions",
            "chalet house palace corners walls",
            vec!["3001.dat", "3003.dat", "3002.dat"],
            false,
            vec![
                "Alternate which wall runs through the corner on successive courses.",
                "Keep the corner footprint fixed and tie both wall directions into the surrounding courses.",
            ],
            "Two independently stacked walls that merely touch at a corner can separate.",
        ),
        (
            "interlocked-t-junction",
            "Bonded T-junctions",
            "junctions",
            "chalet house palace partitions walls",
            vec!["3001.dat", "3002.dat", "3003.dat"],
            false,
            vec![
                "Run the perpendicular wall into the main wall on alternating courses.",
                "Use shorter bricks around the junction so its bond joins both wall segments.",
            ],
            "Avoid a full-height seam between a partition and its exterior wall.",
        ),
        (
            "english-bond",
            "English-bond double walls",
            "walls",
            "chalet house palace thick walls timber framing",
            vec!["3010.dat", "3004.dat"],
            false,
            vec![
                "Offset the seams of the front and rear lengthwise rows.",
                "Alternate with crosswise bricks that join the two rows through the wall thickness.",
            ],
            "Two parallel skins are not one structure unless courses bridge between them.",
        ),
        (
            "opening-lintels",
            "Strapped door and window openings",
            "openings",
            "chalet house palace windows doors lintels",
            vec!["3007.dat", "2456.dat", "3001.dat"],
            false,
            vec![
                "Size doors and windows for the selected model scale.",
                "Bridge each opening with a supported lintel and bond its ends into both jambs.",
                "Resolve matching catalogue frames and inserts when using purpose-made windows or doors.",
            ],
            "Openings interrupt running bond; the course above must lock their sides together.",
        ),
        (
            "staggered-slabs",
            "Overlapping floor and foundation layers",
            "floors",
            "chalet house palace floors foundations plates",
            vec!["41539.dat", "3035.dat", "3031.dat", "3034.dat"],
            false,
            vec![
                "Apply the overlapping-course principle to plates with seams offset between layers.",
                "Place columns on complete stud footprints and provide bearing beneath each slab span.",
            ],
            "This is a project application of the source's overlap principle; plate overlap alone is not a gravity or strength certificate.",
        ),
        (
            "stepped-roof",
            "Studs-up stepped roofs",
            "roofs",
            "chalet house roof gable eaves ridge starter",
            vec!["3007.dat", "2456.dat", "3034.dat", "3832.dat", "4282.dat"],
            false,
            vec![
                "Use roof courses at least two studs deep and step each successive course inward by one stud.",
                "Keep a connected row of engaged studs between courses; offset lengthwise seams.",
                "Provide gables, rafters or other explicit bearing for roof strips and overhangs.",
            ],
            "Brick and plate rises create different roof pitches; do not replace connected courses with freely tilted slabs.",
        ),
        (
            "slope-roof",
            "Catalogue-slope roofs",
            "roofs",
            "chalet house roof gable slopes realistic",
            vec![],
            true,
            vec![
                "Select a compatible family of roof slopes for the intended pitch.",
                "Stagger adjacent courses and use the appropriate ridge, end and corner pieces.",
                "Resolve actual part IDs, geometry, stud locations and physical data before placement.",
            ],
            "A slope is not a tilted rectangular plate. Source geometry does not automatically provide calibrated connectors or a supported collision model.",
        ),
        (
            "roof-dormer",
            "Framed roof dormers",
            "roofs",
            "chalet house dormer windows roof",
            vec![],
            true,
            vec![
                "Plan a supported opening in the main roof rather than overlapping it with a dormer.",
                "Frame the dormer window and use compatible roof and inside-corner parts.",
            ],
            "Dormers interrupt the roof bond; preserve bearing and engagement around every cutout.",
        ),
    ];
    let query = query.unwrap_or("").to_lowercase();
    let mut techniques = Vec::new();
    for (key, name, topic, tags, parts, needs_catalog, steps, pitfall) in entries {
        if id.is_some_and(|id| id != key) || category.is_some_and(|c| c != topic) {
            continue;
        }
        let aliases = if tags.contains("chalet") {
            "swiss alpine chalets houses"
        } else {
            ""
        };
        let haystack = format!(
            "lego brick building {key} {name} {topic} {tags} {aliases} {steps:?} {pitfall}"
        )
        .to_lowercase();
        if !query.split_whitespace().all(|word| haystack.contains(word)) {
            continue;
        }
        techniques.push(json!({"id":key,"name":name,"category":topic,"starter_part_ids":parts,
            "requires_additional_catalogue_geometry_and_physics":needs_catalog,"steps":steps,"pitfalls":[pitfall],
            "source":{"publisher":"Brick Builder's Handbook","title":"Basic LEGO Techniques","url":SOURCE},
            "status":"Design guidance; validate the resulting construction independently"}));
    }
    ensure!(
        id.is_none() || !techniques.is_empty(),
        "Unknown or filtered-out technique ID"
    );
    Ok(
        json!({"library":"ergo-building-techniques-1","reviewed":"2026-10-26","techniques":techniques,
        "selection":"Curated category and case-insensitive keyword filtering, not live semantic catalogue search",
        "building_policy":"Use minifigure scale, nine upright brick courses per house storey, snapped coherent placements and proper metric isometries. Propose suitable techniques before generating geometry.",
        "workflow_guidance":{"source":"User-confirmed Ergo workflow","rules":["Grow the same construction in small self-supporting published edits; preserve its assembly identity.","Publication performs authoritative validation. Separate pre-publication validation and dynamics are optional diagnostics, not a default duplicate gate.","Use atomic batches when intermediate states would be unsupported.","Use genuine catalogue architectural parts and retain working user feedback."]},
        "validation":"Technique descriptions are paraphrased guidance, not automatic construction or stability guarantees. Resolve genuine parts and run complete-domain commit and physical checks."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chalet_proposals_are_sourced_and_distinguish_guidance_from_available_parts() {
        let result = library(Some("chalet"), None, None).unwrap();
        let entries = result["techniques"].as_array().unwrap();
        assert!(entries.iter().any(|e| e["id"] == "interlocked-corner"));
        assert!(entries.iter().any(|e| e["id"] == "stepped-roof"));
        for entry in entries {
            assert_eq!(entry["source"]["url"], SOURCE);
            for part in entry["starter_part_ids"].as_array().unwrap() {
                assert!(crate::geometry::part_spec(part.as_str().unwrap()).is_some());
            }
        }
        let slopes = library(None, None, Some("slope-roof")).unwrap();
        assert_eq!(
            slopes["techniques"][0]["requires_additional_catalogue_geometry_and_physics"],
            true
        );
        assert!(library(None, None, Some("invented-technique")).is_err());
    }
}
