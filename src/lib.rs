use actix_session::Session;

use actix_web::{http::header, HttpResponse};

pub mod configuration;
pub use configuration::*;

pub mod routes;
pub use routes::*;

pub mod spotify;
pub use spotify::*;

pub mod telementery;
pub use telementery::*;

pub mod startup;
pub use startup::*;

async fn logout(session: Session) -> HttpResponse {
    session.purge();
    HttpResponse::Found()
        .append_header((header::LOCATION, "/"))
        .finish()
}
