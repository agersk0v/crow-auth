//! Renders escaped Askama pages with fixed form targets and no third-party assets.

use super::*;
use askama::Template;

#[derive(Template)]
#[template(path = "message.html")]
struct MessageTemplate<'a> {
    title: &'a str,
    message: &'a str,
}
#[derive(Template)]
#[template(path = "verify_email.html")]
struct ConfirmationTemplate<'a> {
    csrf: &'a str,
    token: &'a str,
}

#[derive(Template)]
#[template(path = "register.html")]
struct RegistrationTemplate<'a> {
    csrf: &'a str,
    username: &'a str,
    email: &'a str,
    error: &'a str,
}
#[derive(Template)]
#[template(path = "resend.html")]
struct ResendTemplate<'a> {
    csrf: &'a str,
    email: &'a str,
    error: &'a str,
}

pub fn registration_page(csrf: &str, username: &str, email: &str, error: &str) -> Response {
    render(
        RegistrationTemplate {
            csrf,
            username,
            email,
            error,
        }
        .render(),
    )
}
pub fn resend_page(csrf: &str, email: &str, error: &str) -> Response {
    render(ResendTemplate { csrf, email, error }.render())
}

pub fn message_page(title: &str, message: &str) -> Response {
    render(MessageTemplate { title, message }.render())
}
pub fn confirmation_page(csrf: &Secret, token: &str) -> Response {
    render(
        ConfirmationTemplate {
            csrf: csrf.expose_secret(),
            token,
        }
        .render(),
    )
}
fn render(result: Result<String, askama::Error>) -> Response {
    let response = match result {
        Ok(html) => axum::response::Html(html).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Page rendering unavailable",
        )
            .into_response(),
    };
    secure_response(response)
}
