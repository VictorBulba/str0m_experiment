mod encoder;
pub mod str0m_impl;
pub mod webrtc_rs_impl;

use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};

// Choose one of the implementations

use str0m_impl as webrtc_impl;
// use webrtc_rs_impl as webrtc_impl;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let app = Router::new()
        .route("/", get(serve_page))
        .route("/make_session", post(make_session));

    let addr = "0.0.0.0:8080".parse().unwrap();

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn serve_page() -> Html<String> {
    let data = std::fs::read_to_string("index.html").unwrap();
    Html(data)
}

#[derive(serde::Deserialize)]
struct OfferReq {
    offer: String,
}

#[derive(serde::Serialize)]
struct AnswerResp {
    answer: String,
}

async fn make_session(Json(offer_req): Json<OfferReq>) -> Json<AnswerResp> {
    let answer = webrtc_impl::start_session(&offer_req.offer).await;
    Json(AnswerResp { answer })
}
