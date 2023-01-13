use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use sdp::{Codec, MediaAttribute};
use std::time::Instant;
use str0m::net::Receive;
use str0m::{Answer, Candidate, Event, IceConnectionState, Input, Offer, Output, Rtc};
use tokio::net::UdpSocket;

pub(crate) struct WebrtcStream {
    rtc: Rtc,
    socket: UdpSocket,
}

impl WebrtcStream {
    pub(crate) async fn new(offer: Offer) -> (Self, Answer) {
        let rtc_config = Rtc::builder().clear_codecs().enable_h264();

        let mut rtc = rtc_config.build();

        let socket = UdpSocket::bind("0.0.0.0:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        println!("Using socket: {addr:?}");

        let candidate = Candidate::host(addr).unwrap();
        rtc.add_local_candidate(candidate);

        for m in &offer.media_lines {
            for attr in &m.attrs {
                match attr {
                    MediaAttribute::RtpMap(c) if c.codec == Codec::H264 => println!("{c:?}"),
                    _ => (),
                }
            }
        }

        let answer = rtc.accept_offer(offer).unwrap();

        (Self { rtc, socket }, answer)
    }

    pub(crate) async fn run(self) {
        let mut rtc = self.rtc;
        let socket = self.socket;

        // Buffer for incoming data.
        let mut buf = Vec::new();

        loop {
            let timeout = match rtc.poll_output().unwrap() {
                Output::Timeout(v) => v,

                Output::Transmit(v) => {
                    socket.send_to(&v.contents, v.destination).await.unwrap();
                    continue;
                }

                Output::Event(v) => {
                    println!("Event {v:?}");
                    match v {
                        Event::IceConnectionStateChange(IceConnectionState::Disconnected) => return,
                        Event::MediaAdded(mid, kind, dir) => {
                            let m = rtc.media(mid).unwrap();
                            let params = m.payload_params();
                            println!("{mid:?}, {kind:?}, {dir:?}");
                            for p in params {
                                println!("{:?}", p.codec());
                            }
                        }
                        _ => (),
                    }
                    continue;
                }
            };

            let timeout = timeout - Instant::now();

            if timeout.is_zero() {
                rtc.handle_input(Input::Timeout(Instant::now())).unwrap();
                continue;
            }

            let sleep = tokio::time::sleep(timeout);

            buf.resize(2000, 0);

            let input = tokio::select! {
                s = socket.recv_from(&mut buf) => match s {
                    Ok((n, source)) => {
                        buf.truncate(n);
                        Input::Receive(
                            Instant::now(),
                            Receive {
                                source,
                                destination: socket.local_addr().unwrap(),
                                contents: buf.as_slice().try_into().unwrap(),
                            },
                        )
                    }
                    Err(e) => panic!("Socket reading error: {e}"),
                },
                _ = sleep => Input::Timeout(Instant::now())
            };

            rtc.handle_input(input).unwrap();
        }
    }
}

#[tokio::main]
async fn main() {
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
    let offer = Offer::from_sdp_string(&offer_req.offer).unwrap();
    let (webrtc_stream, answer) = WebrtcStream::new(offer).await;
    tokio::spawn(async {
        webrtc_stream.run().await;
    });
    Json(AnswerResp {
        answer: answer.to_sdp_string(),
    })
}