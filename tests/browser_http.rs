//! Checks escaped Askama rendering, strict Origin policy, body bounds and headers in an Axum harness.

use axum::{
    Form, Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    routing::{get, post},
};
use crow_auth::browser::{BrowserError, BrowserSecurity, message_page};
use serde::Deserialize;
use tower::ServiceExt;

#[derive(Deserialize)]
struct Fields {
    value: String,
}
fn app() -> Router {
    BrowserSecurity::new("https://crow.localhost:3000")
        .unwrap()
        .protect(
            Router::new()
                .route(
                    "/",
                    get(|| async {
                        message_page(
                            "<script>alert(1)</script>",
                            "<img src=x onerror=alert(1)> & test",
                        )
                    }),
                )
                .route(
                    "/form",
                    post(|Form(fields): Form<Fields>| async move {
                        message_page("Received", &fields.value)
                    }),
                )
                .route("/failure", get(|| async { BrowserError::Unavailable })),
        )
}
fn assert_headers(response: &axum::response::Response) {
    let h = response.headers();
    assert_eq!(h["cache-control"], "no-store");
    assert_eq!(h["referrer-policy"], "no-referrer");
    assert_eq!(h["x-frame-options"], "DENY");
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert!(
        h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert!(
        h.get_all("vary")
            .iter()
            .any(|v| v.to_str().unwrap().contains("HX-Request"))
    );
}
#[tokio::test]
async fn templates_escape_and_errors_have_security_headers() {
    let response = app()
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_headers(&response);
    let bytes = to_bytes(response.into_body(), 20000).await.unwrap();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!html.contains("<script>"));
    assert!(!html.contains("<img"));
    assert!(html.contains("script"));
    assert!(html.contains("&#") || html.contains("&lt;"));
    assert!(!html.contains("https://"));
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/failure")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_headers(&response);
}
#[tokio::test]
async fn ordinary_and_htmx_origin_checks_and_body_limits_match() {
    for htmx in [false, true] {
        for origin in [
            None,
            Some("null"),
            Some("https://evil.test"),
            Some("https://crow.localhost:3000/"),
        ] {
            let mut request = Request::builder()
                .method("POST")
                .uri("/form")
                .header("content-type", "application/x-www-form-urlencoded");
            if let Some(origin) = origin {
                request = request.header("origin", origin);
            }
            if htmx {
                request = request.header("hx-request", "true");
            }
            let response = app()
                .oneshot(request.body(Body::from("value=ok")).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            assert_headers(&response);
        }
        let mut request = Request::builder()
            .method("POST")
            .uri("/form")
            .header("origin", "https://crow.localhost:3000")
            .header("content-type", "application/x-www-form-urlencoded");
        if htmx {
            request = request.header("hx-request", "true");
        }
        let response = app()
            .oneshot(request.body(Body::from("value=ok")).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_headers(&response);
    }
    for declared in [false, true] {
        let body = format!("value={}", "x".repeat(16 * 1024));
        let mut request = Request::builder()
            .method("POST")
            .uri("/form")
            .header("origin", "https://crow.localhost:3000")
            .header("content-type", "application/x-www-form-urlencoded");
        if declared {
            request = request.header("content-length", body.len());
        }
        let response = app()
            .oneshot(request.body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_headers(&response);
    }
    let response = app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/form")
                .header("origin", "https://crow.localhost:3000")
                .header("origin", "https://evil.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
#[test]
fn only_canonical_https_origins_are_accepted() {
    for origin in [
        "http://localhost:3000",
        "https://crow.test/",
        "https://crow.test/path",
        "https://user@crow.test",
        "https://crow.test?x=1",
        "https://crow.test#x",
        "null",
        "https://crow.test:443",
    ] {
        assert!(matches!(
            BrowserSecurity::new(origin),
            Err(BrowserError::Configuration)
        ));
    }
    assert!(BrowserSecurity::new("https://crow.test").is_ok());
}
