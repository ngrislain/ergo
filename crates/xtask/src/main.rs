//! Explicit workspace build orchestration, outside Cargo's build.rs lifecycle.
use std::{
    env,
    ffi::OsStr,
    path::PathBuf,
    process::{Command, ExitCode},
};

const TARGET: &str = "wasm32-unknown-unknown";
const USAGE: &str = "Usage: cargo xtask build-web | build-site\n\nBuild-web generates the browser app and minimal MCP WASM client.\nBuild-site generates the public site's shared-renderer preview and logo.\nRequires the wasm32-unknown-unknown target and matching wasm-bindgen CLI from README.md.";

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let command = arguments.next();
    let extra = arguments.next();
    let result = match (command.as_deref(), extra) {
        (None | Some("--help" | "-h"), None) => {
            println!("{USAGE}");
            Ok(())
        }
        (Some("build-web"), None) => build_web(),
        (Some("build-site"), None) => build_site(),
        _ => Err(format!("Unknown command or arguments.\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            ExitCode::FAILURE
        }
    }
}

fn build_web() -> Result<(), String> {
    build_targets(&[
        ("ergo-viewer", None, "ergo_viewer.wasm", "web/pkg"),
        (
            "ergo-client",
            Some("mcp-entry"),
            "ergo_client.wasm",
            "web/mcp-pkg",
        ),
    ])
}

fn build_site() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let package: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("site/package.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let version = package["version"].as_str().ok_or("Missing site version")?;
    if !version
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
    {
        return Err("Invalid site version".into());
    }
    let output = format!("site/public/engine/v{version}");
    build_targets(&[(
        "ergo-client",
        Some("site-entry"),
        "ergo_client.wasm",
        &output,
    )])?;
    std::fs::copy(
        root.join("web/favicon.svg"),
        root.join("site/public/favicon.svg"),
    )
    .map_err(|e| format!("Cannot copy the shared logo: {e}"))?;
    std::fs::write(
        root.join("site/public/brick-outline.svg"),
        ergo_world::illustration::brick_outline("3003.dat")
            .ok_or("Missing catalog illustration")?,
    )
    .map_err(|e| format!("Cannot write catalog illustration: {e}"))?;
    Ok(())
}

fn build_targets(targets: &[(&str, Option<&str>, &str, &str)]) -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .ok_or("Cannot locate the workspace root")?
        .to_path_buf();
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());

    // Fail before compilation when the external binding generator is missing.
    let bindgen = Command::new("wasm-bindgen")
        .arg("--version")
        .output()
        .map_err(|error| format!("Cannot run wasm-bindgen: {error}. Install the matching wasm-bindgen-cli version from README.md."))?;
    if !bindgen.status.success() {
        return Err(format!(
            "wasm-bindgen --version failed: {}",
            String::from_utf8_lossy(&bindgen.stderr)
        ));
    }

    // Cargo metadata honors target-dir configuration and CARGO_TARGET_DIR; do
    // not assume artifacts always live in the repository's target/ directory.
    let metadata = Command::new(&cargo)
        .current_dir(&root)
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|error| format!("Cannot run Cargo metadata: {error}"))?;
    if !metadata.status.success() {
        return Err(format!(
            "Cargo metadata failed: {}",
            String::from_utf8_lossy(&metadata.stderr)
        ));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout)
        .map_err(|error| format!("Cannot read Cargo metadata: {error}"))?;
    let target_directory = metadata["target_directory"]
        .as_str()
        .ok_or("Cargo metadata did not report a target directory")?;
    let artifacts = PathBuf::from(target_directory).join(TARGET).join("release");

    for &(package, feature, artifact, output) in targets {
        eprintln!("Building {package} for {TARGET}…");
        let mut build = Command::new(&cargo);
        build.current_dir(&root).args([
            "build",
            "--locked",
            "--release",
            "--target",
            TARGET,
            "--package",
            package,
        ]);
        if let Some(feature) = feature {
            build.args(["--features", feature]);
        }
        run(&mut build, "Cargo WASM build")?;

        let artifact = artifacts.join(artifact);
        if !artifact.is_file() {
            return Err(format!("Cargo did not produce {}", artifact.display()));
        }
        eprintln!("Generating {package} web bindings…");
        run(
            Command::new("wasm-bindgen")
                .current_dir(&root)
                .args(["--target", "web", "--out-dir"])
                .arg(root.join(output))
                .arg("--no-typescript")
                .arg(artifact),
            "wasm-bindgen",
        )?;
    }
    eprintln!("WASM bindings built successfully.");
    Ok(())
}

fn run(command: &mut Command, label: &str) -> Result<(), String> {
    let program = command.get_program().to_owned();
    let status = command.status().map_err(|error| {
        format!(
            "Cannot run {label} ({}): {error}",
            OsStr::to_string_lossy(&program)
        )
    })?;
    if !status.success() {
        return Err(format!("{label} failed with {status}"));
    }
    Ok(())
}
