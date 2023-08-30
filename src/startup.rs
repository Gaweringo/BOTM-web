use std::{env, net::TcpListener};

use actix_files::Files;
use actix_ip_filter::IPFilter;
use actix_session::{storage::CookieSessionStore, SessionMiddleware};
use actix_web::{
    cookie::Key,
    dev::{Server, ServiceRequest},
    web, App, HttpResponse, HttpServer,
};
use actix_web_flash_messages::{storage::CookieMessageStore, FlashMessagesFramework};
use anyhow::Context;
use oauth2::{basic::BasicClient, AuthUrl, ClientId, ClientSecret, RedirectUrl, TokenUrl};
use secrecy::{ExposeSecret, SecretString};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tracing::log::info;
use tracing_actix_web::TracingLogger;
use url::form_urlencoded::Target;

use crate::{
    disconnect, generate, get_connect, index, logout, not_found, redirect, Configuration,
    DatabaseConfig, SpotifyConfig,
};

pub struct Botm {
    port: u16,
    server: Server,
}

impl Botm {
    /// Set up all the configuration for the BOTM web server.
    /// Botm can then be run with `run_until_stopped().await`.
    ///
    /// ```ignore
    /// let configuration = Configuration::new().expect("Failed to load configuration");
    /// let botm = Botm::build(configuration).await.unwrap();
    /// botm.run_until_stopped().await?;
    /// ```
    pub async fn build(configuration: Configuration) -> anyhow::Result<Self> {
        if "local".to_owned() == env::var("ENV").unwrap_or_else(|_| "local".into()) {
            dotenvy::dotenv()?;
        }
        let pg_pool = PgPool::connect_lazy(
            &env::var("DATABASE_URL").context("Failed to load DATABASE_URL in prod")?,
        )
        .context("Failed to connect lazy to db")?;

        sqlx::migrate!()
            .run(&pg_pool)
            .await
            .context("Failed to run migration")?;

        let address = format!(
            "{}:{}",
            configuration.application.host, configuration.application.port
        );
        let listener = TcpListener::bind(address).expect("Failed to bind to address");
        let port = listener.local_addr().unwrap().port();

        let oauth_client = oauth_client_from_config(configuration.spotify);

        let server = run(
            listener,
            pg_pool,
            oauth_client,
            configuration.cron_ips,
            configuration.cookie_key,
        )
        .expect("Failed to create server");

        Ok(Self { port, server })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        self.server.await
    }
}

pub struct ApplicationBaseUrl(pub String);

pub fn run(
    listener: TcpListener,
    pg_pool: PgPool,
    oauth_client: BasicClient,
    _cron_ips: Vec<String>,
    cookie_key: SecretString,
) -> Result<Server, std::io::Error> {
    let connection_pool = web::Data::new(pg_pool);
    let secret_key = Key::from(cookie_key.expose_secret().as_bytes());

    let oauth_client = web::Data::new(oauth_client);

    let message_store = CookieMessageStore::builder(secret_key.clone()).build();
    let message_framework = FlashMessagesFramework::builder(message_store).build();

    let server = HttpServer::new(move || {
        App::new()
            .wrap(TracingLogger::default())
            .wrap(SessionMiddleware::new(
                CookieSessionStore::default(),
                secret_key.clone(),
            ))
            // .wrap(
            //     IPFilter::new()
            //         .allow(cron_ips.iter().map(|ip| ip.as_str()).collect())
            //         .on_block(on_block_handler)
            //         .limit_to(vec!["/generate"]),
            // )
            .wrap(message_framework.clone())
            .route("/", web::get().to(index))
            .route("/connect", web::get().to(get_connect))
            .route("/redirect", web::get().to(redirect))
            .route("/generate", web::post().to(generate))
            .route("/logout", web::get().to(logout))
            .route("/disconnect", web::get().to(disconnect))
            .service(Files::new("/assets/css", "./assets/css"))
            .default_service(web::to(not_found))
            .app_data(connection_pool.clone())
            .app_data(oauth_client.clone())
    })
    .listen(listener)?
    .run();
    Ok(server)
}

pub fn get_connection_pool(database_settings: &DatabaseConfig) -> sqlx::Pool<sqlx::Postgres> {
    PgPoolOptions::new()
        .acquire_timeout(std::time::Duration::from_secs(2))
        .connect_lazy_with(database_settings.with_db())
}

pub fn oauth_client_from_config(spotify_config: SpotifyConfig) -> BasicClient {
    let client_id = ClientId::new(spotify_config.client_id.expose_secret().to_owned());
    let client_secret = ClientSecret::new(spotify_config.client_secret.expose_secret().to_owned());
    let redirect_uri = RedirectUrl::new(spotify_config.redirect_uri).expect("Parse redirect uri");
    let auth_url =
        AuthUrl::new("https://accounts.spotify.com/authorize".to_owned()).expect("Parse auth url");
    let token_url =
        TokenUrl::new("https://accounts.spotify.com/api/token".to_owned()).expect("Parse auth url");
    BasicClient::new(client_id, Some(client_secret), auth_url, Some(token_url))
        .set_redirect_uri(redirect_uri)
}

fn _on_block_handler(_flt: &IPFilter, ip: &str, _req: &ServiceRequest) -> Option<HttpResponse> {
    info!("Blocked ip: {ip} from accessing protected path");
    Some(HttpResponse::Forbidden().body(format!("IP not allowed: {}", ip).finish()))
}
