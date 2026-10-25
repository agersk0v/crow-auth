//! Verifies startup fails safely for missing or invalid database configuration.

use std::process::Command;

#[test]
fn missing_database_url_stops_startup() {
    let output = Command::new(env!("CARGO_BIN_EXE_crow-auth"))
        .env_remove("DATABASE_URL")
        .output()
        .expect("service should execute");

    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("DATABASE_URL must be set"));
}

#[test]
fn invalid_database_url_stops_startup_without_exposing_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_crow-auth"))
        .env(
            "DATABASE_URL",
            "postgres://user:secret-marker@localhost:invalid/database",
        )
        .output()
        .expect("service should execute");

    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Failed to connect to PostgreSQL"));
    assert!(!error.contains("secret-marker"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("secret-marker"));
}
