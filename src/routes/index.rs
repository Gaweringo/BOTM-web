use actix_session::Session;
use actix_web::{web, HttpResponse};
use actix_web_flash_messages::IncomingFlashMessages;
use askama_actix::{Template, TemplateToResponse};
use oauth2::basic::BasicClient;
use sqlx::PgPool;

use crate::{Image, SpotifyConnector, UserInfo};

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    logged_in: bool,
    user: &'a str,
    show_image: bool,
    profile_image_url: &'a str,
    flash_message: Option<&'a str>,
}

pub async fn index(
    session: Session,
    messages: IncomingFlashMessages,
    oauth_client: web::Data<BasicClient>,
    pg_pool: web::Data<PgPool>,
) -> HttpResponse {
    let login = session.get::<String>("login").unwrap();

    let user_info = if let Some(spotify_id) = &login {
        let spotty_con = SpotifyConnector::build(
            oauth_client.as_ref().clone(),
            pg_pool.as_ref().clone(),
            spotify_id,
        )
        .await;
        if let Ok(mut spotty_con) = spotty_con {
            spotty_con.get_user_info().await.ok()
        } else {
            None
        }
    } else {
        None
    };

    let user_info = user_info.unwrap_or_else(|| UserInfo {
        display_name: login.clone().unwrap_or_default(),
        images: vec![Image { url: "".to_owned() }],
    });

    let message = messages.iter().next();
    tracing::debug!(
        "Flash messages: {:?}",
        message.and_then(|m| Some(m.content()))
    );

    IndexTemplate {
        logged_in: login.is_some(),
        user: &user_info.display_name,
        show_image: false,
        profile_image_url: &user_info
            .images
            .get(0)
            .and_then(|i| Some(i.url.to_owned()))
            .unwrap_or_default(),
        flash_message: message.and_then(|m| Some(m.content())),
    }
    .to_response()
}
