use crate::encoder::Encoder;
use std::io::ErrorKind;
use std::net::{IpAddr, UdpSocket};
use std::time::Instant;
use str0m::media::{Direction, MediaKind, MediaTime, Mid, PayloadParams};
use str0m::net::Receive;
use str0m::{Candidate, Event, IceConnectionState, Input, Offer, Output, Rtc};
use systemstat::Duration;

fn select_host_address() -> IpAddr {
    use systemstat::{Platform, System};

    let system = System::new();
    let networks = system.networks().unwrap();

    let mut ips = vec![];

    for net in networks.values() {
        for n in &net.addrs {
            match n.addr {
                systemstat::IpAddr::V4(v) => {
                    if !v.is_loopback() && !v.is_link_local() && !v.is_broadcast() {
                        ips.push(IpAddr::V4(v));
                    }
                }
                _ => {} // we could use ipv6 too
            }
        }
    }

    println!("Found ips: {ips:?}");

    ips[0]
}

pub async fn start_session(offer: &str) -> String {
    let offer = Offer::from_sdp_string(offer).unwrap();

    let rtc_config = Rtc::builder().clear_codecs().enable_vp8();

    let mut rtc = rtc_config.build();

    let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
    let addr = socket.local_addr().unwrap();
    println!("Using socket: {addr:?}");

    let candidate_addr = format!("{}:{}", select_host_address(), addr.port())
        .parse()
        .unwrap();
    let candidate = Candidate::host(candidate_addr).unwrap();
    rtc.add_local_candidate(candidate);

    let answer = rtc.accept_offer(offer).unwrap();

    std::thread::spawn(|| run(rtc, socket));

    answer.to_sdp_string()
}

struct Track {
    mid: Mid,
    params: PayloadParams,
    accumulated_time: Duration,
}

fn run(mut rtc: Rtc, socket: UdpSocket) {
    // Buffer for incoming data.
    let mut buf = Vec::new();

    let (w, h) = (300, 300);

    let mut encoder = Encoder::new(w, h);

    let mut image_track: Option<Track> = None;

    let bgra_pixels: Vec<u8> = (0..(w * h)).flat_map(|_| [0u8, 255, 255, 255]).collect();

    loop {
        let timeout = match rtc.poll_output().unwrap() {
            Output::Timeout(v) => v,

            Output::Transmit(v) => {
                socket.send_to(&v.contents, v.destination).unwrap();
                continue;
            }

            Output::Event(v) => {
                println!("Event {v:?}");
                match v {
                    Event::IceConnectionStateChange(IceConnectionState::Disconnected) => return,
                    Event::MediaAdded(mid, kind, dir) => {
                        assert_eq!(kind, MediaKind::Video);
                        assert_eq!(dir, Direction::SendRecv);
                        let m = rtc.media(mid).unwrap();
                        let params = m.payload_params();
                        image_track = Some(Track {
                            mid,
                            params: params[0].clone(),
                            accumulated_time: Duration::ZERO,
                        });
                    }
                    Event::ChannelData(d) => {
                        let mut chan = rtc.channel(d.id).unwrap();
                        chan.write(false, "pong".as_bytes()).unwrap();
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

        if let Some(track) = &mut image_track {
            let media = rtc.media(track.mid).unwrap();
            let pt = media.match_params(track.params).unwrap();
            let time = track.accumulated_time;
            let frame_dur = Duration::from_millis(10);
            track.accumulated_time += frame_dur;

            // tracing::info!("MATCHED PT: {:?}, TIME: {:?}", pt, time);

            let media_time: MediaTime = time.into();

            let frame_data = encoder.encode(&bgra_pixels, frame_dur);

            media
                .write(pt, None, media_time.rebase(90_000), &frame_data)
                .unwrap();
        }

        socket.set_read_timeout(Some(timeout)).unwrap();
        buf.resize(2000, 0);

        let input = match socket.recv_from(&mut buf) {
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
            Err(e) => match e.kind() {
                // Expected error for set_read_timeout(). One for windows, one for the rest.
                ErrorKind::WouldBlock | ErrorKind::TimedOut => Input::Timeout(Instant::now()),
                _ => panic!("Socket reading error: {e}"),
            },
        };

        rtc.handle_input(input).unwrap();
    }
}
