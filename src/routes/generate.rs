use std::{
    collections::{HashMap, HashSet},
    env,
};

use actix_web::{
    http::header::{self, HeaderMap},
    web, HttpRequest, HttpResponse,
};
use anyhow::{anyhow, Context};
use base64::{engine::general_purpose, Engine};
use chrono::Datelike;
use oauth2::{basic::BasicClient, RefreshToken, TokenResponse};
use secrecy::{ExposeSecret, Secret, SecretString};
use sqlx::PgPool;
use tracing::{debug, log::trace};
use url::Url;

#[derive(serde::Deserialize, Debug)]
pub struct GenerateParams {
    spotify_id: Option<String>,
}

#[derive(Debug)]
struct UserData {
    spotify_id: String,
    refresh_token: String,
}

/// Endpoint to generate the BOTMs for all active users
pub async fn generate(
    pg_pool: web::Data<PgPool>,
    oauth: web::Data<oauth2::basic::BasicClient>,
    request: HttpRequest,
    params: web::Query<GenerateParams>,
) -> HttpResponse {
    // Protected endpoint with basic auth
    let Ok(credentials) = basic_authentication(request.headers()) else {
        return HttpResponse::Unauthorized()
            .insert_header((header::WWW_AUTHENTICATE, r#"Basic realm="publish""#))
            .finish();
    };

    let Ok(username) = env::var("GENERATE_USERNAME") else {
        return HttpResponse::InternalServerError().finish();
    };
    let Ok(password) = env::var("GENERATE_PASSWORD") else {
        return HttpResponse::InternalServerError().finish();
    };

    if credentials.username != username || credentials.password.expose_secret() != &password {
        return HttpResponse::Unauthorized()
            .insert_header((header::WWW_AUTHENTICATE, r#"Basic realm="publish""#))
            .finish();
    }

    if let Some(spotify_id) = &params.spotify_id {
        tracing::info!("Generating for specific user: {}", spotify_id);
    }

    let Ok(users) = (match &params.spotify_id {
        Some(spotify_id) => sqlx::query_as!(
            UserData,
            r#"SELECT spotify_id, refresh_token FROM users WHERE spotify_id = $1 AND active = true"#,
            spotify_id
        ).fetch_all(pg_pool.as_ref()).await,
        None => sqlx::query_as!(
            UserData,
            r#"SELECT spotify_id, refresh_token FROM users WHERE active = true"#
        ).fetch_all(pg_pool.as_ref()).await,
    })
    else {
        tracing::error!("Failed to get users from database");
        return HttpResponse::InternalServerError().finish();
    };

    tracing::info!("Found {} users", users.len());

    let botm_generator = BotmGenerator::new(oauth.as_ref(), pg_pool.as_ref());
    let mut error_users = HashSet::new();
    for user in users.iter() {
        if let Err(err) = botm_generator.generate_for(user).await {
            error_users.insert(&user.spotify_id);
            tracing::error!("Failed to generate BOTM for {}", &user.spotify_id);
            tracing::error!("{}", err);
            continue;
        }
    }

    if error_users.len() != 0 {
        tracing::error!(
            "Failed to generate BOTM for {} of {} users",
            error_users.len(),
            users.len()
        );
        return HttpResponse::InternalServerError().finish();
    }

    HttpResponse::Ok().body(format!("Generated for {} users", users.len()))
}

struct Credentials {
    username: String,
    password: SecretString,
}

fn basic_authentication(headers: &HeaderMap) -> anyhow::Result<Credentials> {
    let header_value = headers
        .get("Authorization")
        .context("The 'Authorization' header was missing")?
        .to_str()
        .context("The 'Authorization' header was not a valid UTF8 string.")?;
    let base64encoded_segment = header_value
        .strip_prefix("Basic ")
        .context("The authorization scheme was not 'Basic'.")?;
    let decoded_bytes = general_purpose::STANDARD
        .decode(base64encoded_segment)
        .context("Failed to base64-decode 'Basic' credentials.")?;
    let decoded_credentials = String::from_utf8(decoded_bytes)
        .context("The decoded credentials string is not valid UTF8.")?;

    let mut credentials = decoded_credentials.splitn(2, ':');
    let username = credentials
        .next()
        .ok_or_else(|| anyhow!("A username must be provided in 'Basic' auth."))?
        .to_string();
    let password = credentials
        .next()
        .ok_or_else(|| anyhow!("A password must be provided in 'Basic' auth."))?
        .to_string();

    Ok(Credentials {
        username,
        password: Secret::new(password),
    })
}

struct BotmGenerator<'a> {
    spotify_api_base: Url,
    reqwest_client: reqwest::Client,
    oauth: &'a BasicClient,
    pg_pool: &'a PgPool,
}

impl<'a> BotmGenerator<'a> {
    fn new(oauth: &'a BasicClient, pg_pool: &'a PgPool) -> Self {
        let reqwest_client = reqwest::Client::new();
        let spotify_api_base = Url::parse("https://api.spotify.com/v1/").expect("Parse base url");
        Self {
            spotify_api_base,
            reqwest_client,
            oauth,
            pg_pool,
        }
    }

    async fn generate_for(&self, user: &UserData) -> anyhow::Result<()> {
        tracing::trace!(
            "Getting access token from spotify for user: {}",
            user.spotify_id
        );
        // Token stuff
        let refresh_token = RefreshToken::new(user.refresh_token.to_owned());
        let token_response = self
            .oauth
            .exchange_refresh_token(&refresh_token)
            .request_async(oauth2::reqwest::async_http_client)
            .await
            .with_context(|| {
                format!(
                    "Failed to exchange_refresh_token for user: {}",
                    user.spotify_id
                )
            })?;

        if let Some(refresh_token) = token_response.refresh_token() {
            trace!("Saving new refresh token for user: {}", user.spotify_id);
            sqlx::query!(
                "UPDATE users SET refresh_token = $1 WHERE spotify_id = $2",
                refresh_token.secret(),
                user.spotify_id
            )
            .execute(self.pg_pool)
            .await
            .context("Failed to store new refresh_token")?;
        };

        // Get top tracks
        trace!("Getting top tracks for user: {}", user.spotify_id);
        let top_tracks_url = self
            .spotify_api_base
            .join("me/top/tracks")
            .context("Failed to parse path to top tracks")?;

        let response = self
            .reqwest_client
            .get(top_tracks_url)
            .bearer_auth(token_response.access_token().secret())
            .query(&[("time_range", "short_term"), ("limit", "50")])
            .send()
            .await;

        let response = response.context("Failed to get top tracks")?;
        let top_tracks = response
            .json::<TopTracksResponse>()
            .await
            .context("Failed to parse top tracks response")?;

        debug!(
            "Got {} top tracks for {}",
            top_tracks.items.len(),
            user.spotify_id
        );

        // Create playlist
        let mut now = chrono::Local::now();
        // If the current time is before the 15 of the month (~half of month) the playlist
        // has more from the month before and should therefor be named for that month.
        // This counteracts any difference in time there would be between the time cron-job.org
        // and fly.io use. So that if 00:01 on 1st from cron-job.org is still last month on fly.io
        // we still get the playlist named for the right month.
        if now.day() < 15 {
            now = now.with_day(1).unwrap_or(now);
            if now.month() == 1 {
                now = now.with_year(now.year() - 1).unwrap_or(now);
                now = now.with_month(12).unwrap_or(now);
            } else {
                now = now.with_month(now.month() - 1).unwrap_or(now);
            }
        }
        let playlist_name = now.format("%Y-%m (%b) BOTM").to_string();
        let description = now.format("Bangers of the month for %B %Y").to_string();
        let description = format!(
            "{}, (generated on {})",
            description,
            chrono::Local::now().format("%F")
        );

        debug!("Generating playlist \"{playlist_name}\" with description \"{description}\"");

        let mut json_body = HashMap::new();
        json_body.insert("name", playlist_name);
        json_body.insert("description", description);
        // debug!("Creating playlist {:?}", json_body);
        let create_playlist_res = self
            .reqwest_client
            .post(
                self.spotify_api_base
                    .join(&format!("users/{}/playlists", user.spotify_id))
                    .context("Failed to parse playlist url")?,
            )
            .json(&json_body)
            .bearer_auth(token_response.access_token().secret())
            .send()
            .await
            .context("Failed to send create playlist")?
            .json::<CreatePlaylistResponse>()
            .await
            .context("Failed to parse playlist create response")?;

        tracing::debug!("Create playlist: {:?}", create_playlist_res);

        // Add songs
        let uris: Vec<&str> = top_tracks.items.iter().map(|i| i.uri.as_str()).collect();
        let add_tracks_body = AddTracksBody { uris, position: 0 };
        tracing::debug!("Add tracks body: {:#?}", add_tracks_body);
        self.reqwest_client
            .post(
                self.spotify_api_base
                    .join(&format!("playlists/{}/tracks", create_playlist_res.id))
                    .context("Failed to parse playlist add")?,
            )
            .json(&add_tracks_body)
            .bearer_auth(token_response.access_token().secret())
            .send()
            .await
            .context("Failed to send playlist add")?
            .error_for_status()
            .context("Error status returned")?;
        Ok(())
    }
}

#[derive(serde::Serialize, Debug)]
struct AddTracksBody<'a> {
    uris: Vec<&'a str>,
    position: i32,
}

#[derive(serde::Deserialize, Debug)]
struct TopTracksResponse {
    items: Vec<Item>,
}

#[derive(serde::Deserialize, Debug)]
struct Item {
    uri: String,
}

#[derive(serde::Deserialize, Debug)]
struct CreatePlaylistResponse {
    id: String,
}
