use std::env;

use config::Config;
use secrecy::{ExposeSecret, SecretString};
use serde_aux::prelude::deserialize_number_from_string;
use sqlx::{
    postgres::{PgConnectOptions, PgSslMode},
    ConnectOptions,
};
use tracing::error;

#[derive(serde::Deserialize, Debug, Clone)]
pub struct Configuration {
    pub application: AppConfig,
    // pub database: DatabaseConfig,
    pub spotify: SpotifyConfig,
    pub cron_ips: Vec<String>,
    pub cookie_key: SecretString,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct AppConfig {
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    // pub base_url: String,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct DatabaseConfig {
    pub username: String,
    pub password: SecretString,
    #[serde(deserialize_with = "deserialize_number_from_string")]
    pub port: u16,
    pub host: String,
    pub database_name: String,
    pub require_ssl: bool,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct SpotifyConfig {
    pub client_id: SecretString,
    pub client_secret: SecretString,
    pub redirect_uri: String,
}

impl Configuration {
    pub fn new() -> Result<Self, config::ConfigError> {
        let run_mode = env::var("ENV").unwrap_or_else(|_| "local".into());

        if let Err(e) = dotenvy::from_filename("secret.env") {
            error!("Failed to load secret.env {}", e);
        }

        tracing::debug!("Running on ENV: {run_mode}");

        let settings = Config::builder()
            .add_source(config::File::with_name("config/base"))
            .add_source(config::File::with_name(&format!("config/{}", run_mode)).required(false))
            .add_source(
                config::Environment::with_prefix("APP")
                    .prefix_separator("_")
                    .separator("__"),
            )
            .build()?;

        // println!("Config: {settings:#?}");

        settings.try_deserialize()
    }
}

impl DatabaseConfig {
    pub fn with_db(&self) -> PgConnectOptions {
        let options = self.without_db().database(&self.database_name);
        options.log_statements(tracing::log::LevelFilter::Trace)
    }
    pub fn without_db(&self) -> PgConnectOptions {
        let ssl_mode = if self.require_ssl {
            PgSslMode::Require
        } else {
            PgSslMode::Prefer
        };
        PgConnectOptions::new()
            .host(&self.host)
            .username(&self.username)
            .password(self.password.expose_secret())
            .port(self.port)
            .ssl_mode(ssl_mode)
    }
}
