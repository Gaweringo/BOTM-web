use std::time::Duration;

use actix_session::Session;
use actix_web::{web, HttpResponse, Responder};
use actix_web_flash_messages::FlashMessage;
use oauth2::{AuthorizationCode, CsrfToken, TokenResponse};
use reqwest::header;
use secrecy::{ExposeSecret, SecretString};
use sqlx::PgPool;
use tracing::error;

use crate::STATE_COOKIE;

#[derive(serde::Deserialize, Debug)]
pub struct RedirectParams {
    #[serde(flatten)]
    outcome: Outcome,
    state: String,
}

#[derive(serde::Deserialize, Debug)]
pub enum Outcome {
    #[serde(rename = "error")]
    Error(String),
    #[serde(rename = "code")]
    Code(SecretString),
}

#[derive(serde::Deserialize, Debug)]
struct MeResponse {
    id: String,
}

pub async fn redirect(
    session: Session,
    params: web::Query<RedirectParams>,
    oauth: web::Data<oauth2::basic::BasicClient>,
    pg_pool: web::Data<PgPool>,
) -> impl Responder {
    // Checking state
    let Ok(Some(cookie_state)) = session.get::<CsrfToken>(STATE_COOKIE) else {
        FlashMessage::error("Failed to connect to Spotify.\nNo state cookie.").send();
        return HttpResponse::Found().append_header((header::LOCATION, "/")).finish();
        // return HttpResponse::Forbidden().body("No state cookie");
    };
    if cookie_state.secret() != &params.state {
        FlashMessage::error("Failed to connect to Spotify.\nMismatched state.").send();
        return HttpResponse::Found()
            .append_header((header::LOCATION, "/"))
            .finish();
        // return HttpResponse::Unauthorized().body("Wrong state");
    }
    session.remove(STATE_COOKIE);

    if let Outcome::Error(error) = &params.outcome {
        let message = match error.as_str() {
       "access_denied" => "You need to agree in order to use this service.\nWe only use the data we need for the service to work.",
        _ => "Failed to connect with Spotify",
    };
        FlashMessage::error(message).send();
        return HttpResponse::Found()
            .append_header((header::LOCATION, "/"))
            .finish();
    }

    let Outcome::Code(code) = &params.outcome else {
        return HttpResponse::InternalServerError().finish();
    };

    // Get access_token
    let Ok(token_response) =
        oauth
        .exchange_code(AuthorizationCode::new(code.expose_secret().clone()))
        .request_async(oauth2::reqwest::async_http_client)
        .await else {
        FlashMessage::error("Failed to connect to Spotify.\nCould not get access token.").send();
        return HttpResponse::Found().append_header((header::LOCATION, "/")).finish();
        // return HttpResponse::Unauthorized().body("Failed to get token");
    };

    let access_token = token_response.access_token();
    let expires_in = token_response.expires_in();
    let Ok(expires_in) = chrono::Duration::from_std(expires_in.unwrap_or_default()) else {
        error!("Failed to convert {expires_in:?} from std::time::Duration to chrono::Duration");
        return HttpResponse::InternalServerError().finish();
    };
    let expiry_timestamp: chrono::DateTime<chrono::Utc> = chrono::Utc::now() + expires_in;

    // Get spotify_id
    tracing::info!("Making /me request");

    let client = reqwest::Client::new();
    let Ok(spotify_id) = client
        .get("https://api.spotify.com/v1/me")
        .bearer_auth(access_token.secret())
        .timeout(Duration::from_secs(10))
        .send()
        .await else {
        return HttpResponse::InternalServerError().body("Failed to access user info");
    };

    let me_response: MeResponse = spotify_id.json().await.unwrap();

    println!("Me response: {:#?}", me_response);

    // Save into users table
    let query_res = sqlx::query!(
        r#"INSERT INTO users (spotify_id, active, refresh_token, access_token, expiry_timestamp) VALUES ($1, true, $2, $3, $4)
            ON CONFLICT (spotify_id) DO UPDATE SET refresh_token = $2"#,
        me_response.id,
        token_response
            .refresh_token()
            .expect("Failed to unwrap refresh_token")
            .secret(),
        token_response.access_token().secret(),
        expiry_timestamp,
    )
    .execute(pg_pool.as_ref())
    .await;

    if query_res.is_err() {
        return HttpResponse::InternalServerError().body(format!(
            "Failed to store user\n\n{:#?}",
            query_res.err().unwrap(),
        ));
    }

    session
        .insert("login", me_response.id)
        .expect("Set session value");

    HttpResponse::Found()
        .append_header((header::LOCATION, "/"))
        .finish()
}
