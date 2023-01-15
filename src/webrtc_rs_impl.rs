use std::sync::Arc;
use systemstat::Duration;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_VP8};
use webrtc::api::APIBuilder;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::media::Sample;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::sdp_type::RTCSdpType;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::rtp_codec::{
    RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType,
};
use webrtc::sdp::SessionDescription;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;

use crate::encoder::Encoder;

fn find_codec(desc: &SessionDescription) -> Option<RTCRtpCodecParameters> {
    desc.media_descriptions
        .iter()
        .flat_map(|md| &md.media_name.formats)
        .flat_map(|format| format.parse())
        .flat_map(|payload_type| desc.get_codec_for_payload_type(payload_type))
        .filter(|codec| codec.name == "VP8")
        .map(|codec| {
            let capability = RTCRtpCodecCapability {
                mime_type: MIME_TYPE_VP8.to_string(),
                ..RTCRtpCodecCapability::default()
            };
            RTCRtpCodecParameters {
                capability,
                payload_type: codec.payload_type,
                stats_id: "Hello123".to_string(),
            }
        })
        .next()
}

pub async fn start_session(offer: &str) -> String {
    let mut desc = RTCSessionDescription::default();
    desc.sdp_type = RTCSdpType::Offer;
    desc.sdp = offer.to_string();

    let codec = find_codec(&desc.unmarshal().unwrap()).unwrap();
    let mut media_engine = MediaEngine::default();
    media_engine
        .register_codec(codec.clone(), RTPCodecType::Video)
        .unwrap();

    let api = APIBuilder::default()
        .with_media_engine(media_engine)
        .build();

    let stun_servers = ["stun:stun.l.google.com:19302"];

    let pc_conf = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: stun_servers.iter().map(|url| url.to_string()).collect(),
            ..Default::default()
        }],
        ..Default::default()
    };

    let peer_conn = api.new_peer_connection(pc_conf).await.unwrap();

    let track_local = TrackLocalStaticSample::new(
        codec.capability,
        "video".to_string(),
        "webrtc-rs".to_string(),
    );
    let track_local = Arc::new(track_local);
    peer_conn.add_track(track_local.clone()).await.unwrap();

    peer_conn.set_remote_description(desc).await.unwrap();

    let answer = peer_conn.create_answer(None).await.unwrap();

    let mut gather_complete = peer_conn.gathering_complete_promise().await;

    peer_conn
        .set_local_description(answer.clone())
        .await
        .unwrap();

    let _ = gather_complete.recv().await;

    tokio::spawn(async move {
        run(track_local).await;
    });

    answer.sdp
}

async fn run(track: Arc<TrackLocalStaticSample>) {
    let (w, h) = (300, 300);

    let mut encoder = Encoder::new(w, h);

    let bgra_pixels: Vec<u8> = (0..(w * h)).flat_map(|_| [0u8, 255, 255, 255]).collect();

    loop {
        let frame_dur = Duration::from_millis(10);

        let frame_data = encoder.encode(&bgra_pixels, frame_dur);

        let sample = Sample {
            data: frame_data.into(),
            duration: frame_dur,
            ..Sample::default()
        };
        track.write_sample(&sample).await.unwrap();
    }
}
