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

#[test]
fn mail_worker_requires_configuration_without_exposing_secrets() {
    let output = Command::new(env!("CARGO_BIN_EXE_mail-worker"))
        .arg("--once")
        .env_remove("MAIL_FROM")
        .env("MAIL_ENCRYPTION_KEY", "secret-key-marker")
        .env("SMTP_PASSWORD", "secret-smtp-marker")
        .output()
        .expect("worker should execute");
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Configuration"));
    assert!(!error.contains("secret-key-marker"));
    assert!(!error.contains("secret-smtp-marker"));
}
