//! Shared MCP schema only. Implementations execute in the minimal WASM client.
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
fn tool(
    name: &str,
    description: &str,
    properties: Value,
    required: &[&str],
    read_only: bool,
) -> Value {
    json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":read_only,"destructiveHint":!read_only,"openWorldHint":false}})
}
pub fn tools() -> Value {
    let int = json!({"type":"integer","minimum":1,"maximum":4294967295_u64});
    let vec = json!({"type":"array","items":{"type":"number"},"minItems":3,"maxItems":3});
    let views = json!({"type":"array","items":{"type":"string","enum":["isometric","north","south","east","west","top"]},"minItems":1,"maxItems":4});
    let width = json!({"type":"integer","minimum":160,"maximum":1024});
    let height = json!({"type":"integer","minimum":120,"maximum":768});
    let mut publish = tool(
        "publish_construction",
        "Publish the local working copy through the configured authenticated shared-world API. Omit world_id to create a world, or import a new assembly into an existing world. Return confirmed revision/assembly metadata URL; preserve validation, permission, conflict, and unknown-outcome errors. This is not an update of a previously published assembly.",
        json!({"world_id":{"type":"string"},"base_revision":{"type":"string"},"name":{"type":"string"},"expected_local_revision":{"type":"integer","minimum":0,"maximum":4294967295_u64},"authenticated_editing":{"type":"boolean"}}),
        &[],
        false,
    );
    publish["annotations"]["openWorldHint"] = json!(true);
    let mut allowance = tool(
        "get_edit_allowance",
        "Read the signed-in actor's remaining shared-world brick-change allowance and refill period. Paid entitlements bypass pacing, not permission or physical validation.",
        json!({}),
        &[],
        true,
    );
    allowance["annotations"]["openWorldHint"] = json!(true);
    json!({"tools":[
        tool("get_catalog","Recommend standard parts and colors first. Any resolvable Studio/LDraw .dat part can be placed by ID. Set include_extended for the full embedded color palette, or part to inspect resolved geometry. Measured physics is required for publication.",json!({"include_extended":{"type":"boolean"},"part":{"type":"string"}}),&[],true),
        tool("list_bricks","List IDs, poses, bounds, and sourced masses.",json!({"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":200}}),&[],true),
        tool("inspect_brick","Inspect a brick, metric dimensions, mass provenance, contacts, and constraints.",json!({"id":int}),&["id"],true),
        tool("place_brick","Place a supported part by minimum corner [east,north,height]. Reject overlap atomically; return client stability feedback.",json!({"part":{"type":"string"},"color":{"type":"integer"},"min_corner":vec,"quarter_turns":{"type":"integer"}}),&["part","color","min_corner"],false),
        tool("move_brick","Move the selected fixed-connected group with collision checks.",json!({"id":int,"delta":vec}),&["id","delta"],false),
        tool("remove_bricks","Remove parts/incident joints and report lost support.",json!({"ids":{"type":"array","items":int,"minItems":1,"maxItems":5000}}),&["ids"],false),
        tool("connect_bricks","Add/replace a fixed or revolute constraint. Coordinates/axis use X east, Y north, Z up; limits are radians.",json!({"a":int,"b":int,"anchor":vec,"kind":{"type":"string","enum":["fixed","revolute"]},"axis":vec,"limits":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}}),&["a","b","anchor"],false),
        tool("list_groups","List nested authoring groups, members, pivots, and derived bounds. Grouping does not weld pieces.",json!({}),&[],true),
        tool("create_group","Create a group from pieces and/or existing groups. A member has one direct parent. Transform poses remain world-space.",json!({"name":{"type":"string"},"piece_ids":{"type":"array","items":int,"maxItems":5000},"group_ids":{"type":"array","items":int,"maxItems":10000},"pivot_world":vec}),&["name"],false),
        tool("set_group_members","Replace group members. reparent=true explicitly moves them from prior parents; cycles are rejected.",json!({"group_id":int,"piece_ids":{"type":"array","items":int,"maxItems":5000},"group_ids":{"type":"array","items":int,"maxItems":10000},"reparent":{"type":"boolean"}}),&["group_id"],false),
        tool("transform_group","Rigidly translate/rotate every nested member once. Axis is an orthonormal ENU direction; angle is radians. Check destination body boxes, floor, anchors, and external constraints. This is a CAD edit, not a continuous rotational sweep.",json!({"group_id":int,"translation":vec,"rotation_axis":vec,"rotation_radians":{"type":"number","minimum":(-std::f64::consts::TAU),"maximum":std::f64::consts::TAU},"pivot_world":vec}),&["group_id"],false),
        tool("ungroup","Dissolve a group while preserving pieces, nested groups, and world poses; splice its children into its parent.",json!({"group_id":int}),&["group_id"],false),
        tool("validate_construction","Run overlap/static-certificate checks locally and disclose missing calibration. No global publication.",json!({}),&[],true),
        tool("test_stability_by_gravity","Run a non-destructive short gravity equilibrium probe in local WASM: one step by default, settable step count and floating-point/solver tolerances. Return motion, static-certificate feedback, and calibration status.",json!({"steps":{"type":"integer","minimum":1,"maximum":120},"position_tolerance":{"type":"number","minimum":1e-12,"maximum":0.1},"rotation_tolerance_radians":{"type":"number","minimum":1e-12,"maximum":0.1}}),&[],true),
        tool("test_stability_by_earthquake","Run local WASM X/Y horizontal shake tests at the given PGA bound in g, not source earthquake magnitude. Default 4 Hz, 3 s shake plus 2 s settling.",json!({"peak_ground_acceleration_g":{"type":"number","minimum":0.001,"maximum":2},"frequency_hz":{"type":"number","minimum":0.1,"maximum":20},"duration_s":{"type":"number","minimum":0.1,"maximum":4},"include_images":{"type":"boolean"}}),&["peak_ground_acceleration_g"],true),
        tool("test_visual_appearance","Return actual local WASM/WebGL2 PNGs from a supplied camera or presets for vision feedback. No automatic aesthetic pass.",json!({"camera":{"type":"object","properties":{"position_world":vec,"target_world":vec,"vertical_fov_degrees":{"type":"number","minimum":10,"maximum":110}},"required":["position_world","target_world"],"additionalProperties":false},"views":views,"width":width,"height":height}),&[],true),
        tool("test_moderate_earthquake_stability","Local WASM moderate-shake preset: independent X/Y, 4 Hz, 3 s plus 2 s settling, 0.15 g acceleration bound. Optional WebGL images. Not a seismic-code rating.",json!({"include_images":{"type":"boolean"}}),&[],true),
        tool("test_solidity","Non-destructive earthquake or dropped-box test in the local WASM client, with optional post-test image.",json!({"load_case":{"oneOf":[{"type":"object","properties":{"kind":{"const":"earthquake"},"displacement_amplitude_m":{"type":"number","minimum":0,"maximum":0.05},"frequency_hz":{"type":"number","minimum":0.1,"maximum":20},"duration_s":{"type":"number","minimum":0.1,"maximum":4},"direction":vec},"required":["kind","displacement_amplitude_m","frequency_hz","duration_s","direction"],"additionalProperties":false},{"type":"object","properties":{"kind":{"const":"falling_object"},"mass_kg":{"type":"number","minimum":0.0001,"maximum":10},"dimensions_m":vec,"target_world":vec,"release_height_m":{"type":"number","minimum":0.01,"maximum":2}},"required":["kind","mass_kg","dimensions_m","target_world","release_height_m"],"additionalProperties":false}]},"include_image":{"type":"boolean"}}),&["load_case"],true),
        tool("simulate_gravity","Run client gravity and retain the resulting local working pose (not a global commit).",json!({"seconds":{"type":"number","minimum":0,"maximum":4}}),&[],false),
        tool("view_construction","Return actual local WebGL2 images from 1–4 preset viewpoints.",json!({"views":views,"width":width,"height":height}),&[],true),
        tool("undo","Restore the preceding local working copy.",json!({}),&[],false),
        tool("export_construction","Flush and identify the configured local JSON working copy. No global publication.",json!({}),&[],true),
        publish,
        allowance
    ]})
}
pub fn validate_input(value: &Value, schema: &Value, path: &str) -> Result<()> {
    if let Some(alternatives) = schema.get("oneOf").and_then(Value::as_array) {
        ensure!(
            alternatives
                .iter()
                .filter(|s| validate_input(value, s, path).is_ok())
                .count()
                == 1,
            "{path} must match one supported load case"
        );
        return Ok(());
    }
    if let Some(v) = schema.get("const") {
        ensure!(value == v, "Unexpected value at {path}");
    }
    if let Some(v) = schema.get("enum").and_then(Value::as_array) {
        ensure!(v.contains(value), "Unsupported value at {path}");
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let object = value
                .as_object()
                .with_context(|| format!("{path} must be an object"))?;
            let properties = schema["properties"].as_object().context("Invalid schema")?;
            if let Some(keys) = schema.get("required").and_then(Value::as_array) {
                for key in keys {
                    ensure!(
                        object.contains_key(key.as_str().unwrap()),
                        "Missing {path}.{key}"
                    );
                }
            }
            for (key, v) in object {
                let s = properties
                    .get(key)
                    .with_context(|| format!("Unsupported argument {path}.{key}"))?;
                validate_input(v, s, &format!("{path}.{key}"))?;
            }
        }
        Some("array") => {
            let values = value
                .as_array()
                .with_context(|| format!("{path} must be an array"))?;
            if let Some(min) = schema.get("minItems").and_then(Value::as_u64) {
                ensure!(values.len() >= min as usize, "Too few {path} items");
            }
            if let Some(max) = schema.get("maxItems").and_then(Value::as_u64) {
                ensure!(values.len() <= max as usize, "Too many {path} items");
            }
            if let Some(item) = schema.get("items") {
                for v in values {
                    validate_input(v, item, path)?;
                }
            }
        }
        Some("number") | Some("integer") => {
            let n = value
                .as_f64()
                .with_context(|| format!("{path} must be numeric"))?;
            ensure!(n.is_finite(), "{path} must be finite");
            if schema["type"] == "integer" {
                ensure!(n.fract() == 0.0, "{path} must be integer");
            }
            if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
                ensure!(n >= min, "{path} is below {min}");
            }
            if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
                ensure!(n <= max, "{path} exceeds {max}");
            }
        }
        Some("string") => ensure!(value.is_string(), "{path} must be text"),
        Some("boolean") => ensure!(value.is_boolean(), "{path} must be boolean"),
        _ => {}
    }
    Ok(())
}
