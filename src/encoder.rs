use std::time::Duration;

pub(crate) struct Encoder {
    encoder: vpx_encode::Encoder,
    width: u32,
    height: u32,
    pts: i64,
}

unsafe impl Send for Encoder {}
unsafe impl Sync for Encoder {}

impl Encoder {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let config = vpx_encode::Config {
            width,
            height,
            timebase: [1, 60],
            bitrate: 5 * 1024 * 1024,
            codec: vpx_encode::VideoCodecId::VP8,
        };
        let encoder = vpx_encode::Encoder::new(config).unwrap();
        Self {
            encoder,
            width,
            height,
            pts: 0,
        }
    }

    pub(crate) fn encode(&mut self, bgra_pixels: &[u8], duration: Duration) -> Vec<u8> {
        let pts = self.pts;
        let mut yuv = Vec::new();
        argb_to_i420(
            self.width as usize,
            self.height as usize,
            bgra_pixels,
            &mut yuv,
        );
        let packets = self.encoder.encode(pts, &yuv).unwrap();
        self.pts += duration.as_millis() as i64;

        let mut packet = Vec::new();

        for p in packets {
            packet.extend_from_slice(p.data);
        }

        packet
    }
}

fn argb_to_i420(width: usize, height: usize, src: &[u8], dest: &mut Vec<u8>) {
    let stride = src.len() / height;

    dest.clear();

    for y in 0..height {
        for x in 0..width {
            let o = y * stride + 4 * x;

            let b = src[o] as i32;
            let g = src[o + 1] as i32;
            let r = src[o + 2] as i32;

            let y = (66 * r + 129 * g + 25 * b + 128) / 256 + 16;
            dest.push(clamp(y));
        }
    }

    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let o = y * stride + 4 * x;

            let b = src[o] as i32;
            let g = src[o + 1] as i32;
            let r = src[o + 2] as i32;

            let u = (-38 * r - 74 * g + 112 * b + 128) / 256 + 128;
            dest.push(clamp(u));
        }
    }

    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let o = y * stride + 4 * x;

            let b = src[o] as i32;
            let g = src[o + 1] as i32;
            let r = src[o + 2] as i32;

            let v = (112 * r - 94 * g - 18 * b + 128) / 256 + 128;
            dest.push(clamp(v));
        }
    }
}

fn clamp(x: i32) -> u8 {
    x.min(255).max(0) as u8
}
