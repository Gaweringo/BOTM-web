use anyhow::{anyhow, bail, Context};
use oauth2::{basic::BasicClient, RefreshToken, TokenResponse};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sqlx::PgPool;
use tracing::{debug, error, trace};

pub struct SpotifyConnector {
    pg_pool: PgPool,
    spotify_id: String,
    refresh_token: SecretString,
    access_token: SecretString,
    oauth: BasicClient,
}

#[derive(Debug)]
struct UserData {
    refresh_token: String,
    access_token: String,
    expiry_timestamp: chrono::DateTime<chrono::Utc>,
}

impl SpotifyConnector {
    pub async fn build(
        oauth_client: BasicClient,
        pg_pool: PgPool,
        spotify_id: &str,
    ) -> anyhow::Result<Self> {
        trace!("Building SpotifyConnector for {}", spotify_id);
        let Ok(user) = sqlx::query_as!(
        UserData,
        r#"SELECT refresh_token, access_token, expiry_timestamp FROM users WHERE spotify_id = $1"#,
            spotify_id,
        )
        .fetch_one(&pg_pool)
        .await else {
            tracing::error!("Failed to get user from database");
            return  Err(anyhow!("Failed to get user from database"));
        };

        let mut new_self = Self {
            pg_pool,
            spotify_id: spotify_id.to_owned(),
            access_token: user.access_token.into(),
            refresh_token: user.refresh_token.into(),
            oauth: oauth_client,
        };
        debug!("Comparing now to expiry");
        if chrono::Utc::now() > user.expiry_timestamp {
            tracing::debug!(
                "Found outdated access_token for user {}, getting new one",
                spotify_id
            );
            new_self.refresh_access_token().await?;
        } else {
            tracing::debug!("Found valid access_token for user {}", spotify_id);
        }

        Ok(new_self)
    }

    /// Checks if if the access_token stored in the database is still valid
    /// and if not, gets a new one using the refresh token.
    async fn refresh_access_token(&mut self) -> anyhow::Result<()> {
        debug!("Checking access token for {}", self.spotify_id);
        let Ok(user) = sqlx::query_as!(
        UserData,
        r#"SELECT refresh_token, access_token, expiry_timestamp FROM users WHERE spotify_id = $1"#,
            self.spotify_id,
        )
        .fetch_one(&self.pg_pool)
        .await else {
            tracing::error!("Failed to get user from database");
            return  Err(anyhow!("Failed to get user from database"));
        };

        if chrono::Utc::now() > user.expiry_timestamp
            || &user.access_token != self.access_token.expose_secret()
        {
            tracing::debug!(
                "Found outdated access_token for {}, getting new one",
                self.spotify_id
            );

            let refresh_token = RefreshToken::new(self.refresh_token.expose_secret().clone());
            let token_response = self
                .oauth
                .exchange_refresh_token(&refresh_token)
                .request_async(oauth2::reqwest::async_http_client)
                .await
                .with_context(|| {
                    format!(
                        "Failed to exchange_refresh_token for user: {}",
                        self.spotify_id
                    )
                })?;

            let expires_in = token_response.expires_in();
            let Ok(expires_in) = chrono::Duration::from_std(expires_in.unwrap_or_default()) else {
                error!("Failed to convert {expires_in:?} from std::time::Duration to chrono::Duration");
                bail!("Failed to convert expires in")
            };
            let expiry_timestamp: chrono::DateTime<chrono::Utc> = chrono::Utc::now() + expires_in;
            sqlx::query!(
                "UPDATE users SET access_token = $1, expiry_timestamp = $2 WHERE spotify_id = $3",
                token_response.access_token().secret(),
                expiry_timestamp,
                self.spotify_id
            )
            .execute(&self.pg_pool)
            .await
            .context("Failed to update access_token in database")?;

            self.access_token = token_response.access_token().secret().to_owned().into();

            if let Some(refresh_token) = token_response.refresh_token() {
                trace!("Saving new refresh token for user: {}", self.spotify_id);
                sqlx::query!(
                    "UPDATE users SET refresh_token = $1 WHERE spotify_id = $2",
                    refresh_token.secret(),
                    self.spotify_id
                )
                .execute(&self.pg_pool)
                .await
                .context("Failed to store new refresh_token")?;
            };

            if let Some(refresh_token) = token_response.refresh_token() {
                self.refresh_token = refresh_token.secret().to_owned().into();
            }
        } else {
            debug!("Access token for {} is still valid", self.spotify_id);
        }
        Ok(())
    }

    /// Gets the user info of the current user.
    ///
    /// Has to check if the current access token is still valid (reason for mut)
    pub async fn get_user_info(&mut self) -> anyhow::Result<UserInfo> {
        debug!("Getting user info for {}", self.spotify_id);
        self.refresh_access_token().await?;
        let client = reqwest::Client::new();
        let user_id = &self.spotify_id;
        let response = client
            .get(format!("https://api.spotify.com/v1/users/{user_id}"))
            .bearer_auth(self.access_token.expose_secret())
            .send()
            .await
            .context("Failed to send user info request")?;
        return Ok(response
            .json::<UserInfo>()
            .await
            .context("Failed to deserialize to user info")?);
    }
}

#[derive(Deserialize)]
pub struct UserInfo {
    pub display_name: String,
    pub images: Vec<Image>,
}

#[derive(Deserialize)]
pub struct Image {
    pub url: String,
}
