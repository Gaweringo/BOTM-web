use actix_session::Session;
use actix_web::{http::header, web, HttpResponse};
use sqlx::PgPool;

pub async fn disconnect(session: Session, pg_pool: web::Data<PgPool>) -> HttpResponse {
    let Ok(user) = session.get::<String>("login") else {
        return HttpResponse::Found()
            .append_header((header::LOCATION, "/"))
            .finish();
    };

    let res = sqlx::query!("DELETE FROM users WHERE spotify_id = $1", user)
        .execute(pg_pool.as_ref())
        .await;

    if res.is_ok() {
        session.purge();
    }

    HttpResponse::Found()
        .append_header((header::LOCATION, "/"))
        .finish()
}
