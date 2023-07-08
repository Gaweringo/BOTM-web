use actix_session::Session;
use actix_web::{http::header, web, HttpResponse, Responder};
use oauth2::{basic::BasicClient, CsrfToken, Scope};

pub const STATE_COOKIE: &str = "spotify_auth_state";

pub async fn get_connect(session: Session, oauth: web::Data<BasicClient>) -> impl Responder {
    let (auth_url, csrf_token) = oauth
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("playlist-modify-private".to_string()))
        .add_scope(Scope::new("playlist-modify-public".to_string()))
        .add_scope(Scope::new("user-top-read".to_string()))
        .add_scope(Scope::new("user-read-private".to_string()))
        .url();

    session
        .insert(STATE_COOKIE, csrf_token)
        .expect("Save state cookie");

    tracing::debug!("Sending user to {auth_url}");

    HttpResponse::Found()
        .append_header((header::LOCATION, auth_url.to_string()))
        .finish()
}
