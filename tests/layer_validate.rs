//! Integration tests for layer validation.
//!
//! Each test builds a Docker image with the layer installed and runs its
//! validation commands to confirm the toolchain is functional.
//!
//! These tests require Docker and the `claudine:latest` base image.
//! Run with: `cargo test --test layer_validate -- --ignored`
//! Run a single layer: `cargo test --test layer_validate validate_rust -- --ignored`

use std::process::Command;

fn validate(layer: &str) {
    let bin = env!("CARGO_BIN_EXE_claudine");
    let output = Command::new(bin)
        .args(["layer", "validate", layer])
        .output()
        .unwrap_or_else(|e| panic!("Failed to execute claudine: {e}"));

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "Layer '{layer}' validation failed.\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}

// -- Fast layers (pre-built binaries / packages) --

#[test]
#[ignore]
fn validate_node_20() {
    validate("node-20");
}

#[test]
#[ignore]
fn validate_node_22() {
    validate("node-22");
}

#[test]
#[ignore]
fn validate_node_24() {
    validate("node-24");
}

#[test]
#[ignore]
fn validate_gh() {
    validate("gh");
}

#[test]
#[ignore]
fn validate_heroku() {
    validate("heroku");
}

#[test]
#[ignore]
fn validate_python_venv() {
    validate("python-venv");
}

#[test]
#[ignore]
fn validate_rust() {
    validate("rust");
}

#[test]
#[ignore]
fn validate_go() {
    validate("go");
}

#[test]
#[ignore]
fn validate_java() {
    validate("java");
}

#[test]
#[ignore]
fn validate_flyway() {
    validate("flyway");
}

#[test]
#[ignore]
fn validate_aws() {
    validate("aws");
}

#[test]
#[ignore]
fn validate_terraform() {
    validate("terraform");
}

#[test]
#[ignore]
fn validate_doctl() {
    validate("doctl");
}

#[test]
#[ignore]
fn validate_postgres() {
    validate("postgres");
}

// -- Slow layers (built from source) --

#[test]
#[ignore]
fn validate_lin() {
    validate("lin");
}

#[test]
#[ignore]
fn validate_exp() {
    validate("exp");
}

#[test]
#[ignore]
fn validate_glab() {
    validate("glab");
}

#[test]
#[ignore]
fn validate_rodney() {
    validate("rodney");
}
