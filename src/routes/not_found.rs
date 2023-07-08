use actix_web::HttpResponse;

pub async fn not_found() -> HttpResponse {
    HttpResponse::Ok().body("404 Page not found")
}
