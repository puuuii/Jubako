use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const RLE_MAGIC: &[u8; 4] = b"JRLE";
pub const THUMBNAIL_MAX_WIDTH: usize = 240;
pub const THUMBNAIL_MAX_HEIGHT: usize = 160;

pub fn parse_image_description(description: &str) -> Option<(usize, usize)> {
    let dimensions = description.split(':').next()?;
    let mut parts = dimensions.split('x');
    let width = parts.next()?.parse().ok()?;
    let height = parts.next()?.parse().ok()?;
    Some((width, height))
}

pub fn format_image_description(width: usize, height: usize, hash: u64) -> String {
    format!("{width}x{height}:{hash:016x}")
}

pub fn hash_rgba_sample(width: usize, height: usize, bytes: &[u8]) -> u64 {
    const SAMPLE_CHUNK: usize = 4096;

    let mut hasher = DefaultHasher::new();
    width.hash(&mut hasher);
    height.hash(&mut hasher);
    bytes.len().hash(&mut hasher);

    if bytes.len() <= SAMPLE_CHUNK * 3 {
        bytes.hash(&mut hasher);
    } else {
        bytes[..SAMPLE_CHUNK].hash(&mut hasher);
        let mid = bytes.len() / 2;
        bytes[mid - SAMPLE_CHUNK / 2..mid + SAMPLE_CHUNK / 2].hash(&mut hasher);
        bytes[bytes.len() - SAMPLE_CHUNK..].hash(&mut hasher);
    }

    hasher.finish()
}

pub fn compress_rgba(rgba: &[u8]) -> Vec<u8> {
    if rgba.is_empty() || rgba.len() % 4 != 0 {
        return rgba.to_vec();
    }

    let mut out = Vec::with_capacity(rgba.len() / 2 + 4);
    out.extend_from_slice(RLE_MAGIC);

    let mut chunks = rgba.chunks_exact(4);
    let Some(first) = chunks.next() else {
        return rgba.to_vec();
    };
    let mut current: [u8; 4] = [first[0], first[1], first[2], first[3]];
    let mut count: u8 = 1;

    for chunk in chunks {
        let pixel = [chunk[0], chunk[1], chunk[2], chunk[3]];
        if pixel == current && count < u8::MAX {
            count += 1;
        } else {
            out.push(count);
            out.extend_from_slice(&current);
            current = pixel;
            count = 1;
        }
    }
    out.push(count);
    out.extend_from_slice(&current);

    if out.len() >= rgba.len() {
        rgba.to_vec()
    } else {
        out
    }
}

pub fn decompress_rgba(data: &[u8], width: usize, height: usize) -> Vec<u8> {
    let expected_len = width.saturating_mul(height).saturating_mul(4);
    if data.len() != expected_len && data.starts_with(RLE_MAGIC) {
        let payload = &data[RLE_MAGIC.len()..];
        if payload.len() % 5 == 0 {
            let mut out = Vec::with_capacity(expected_len);
            for run in payload.chunks_exact(5) {
                let count = run[0] as usize;
                let pixel = &run[1..5];
                for _ in 0..count {
                    out.extend_from_slice(pixel);
                }
            }
            if out.len() == expected_len {
                return out;
            }
        }
    }

    data.to_vec()
}

pub fn downscale_rgba(
    rgba: &[u8],
    src_w: usize,
    src_h: usize,
    max_w: usize,
    max_h: usize,
) -> (usize, usize, Vec<u8>) {
    if src_w == 0
        || src_h == 0
        || (src_w <= max_w && src_h <= max_h)
        || rgba.len() < src_w.saturating_mul(src_h).saturating_mul(4)
    {
        return (src_w, src_h, rgba.to_vec());
    }

    let scale = (max_w as f32 / src_w as f32).min(max_h as f32 / src_h as f32);
    let dst_w = ((src_w as f32 * scale).round() as usize).max(1);
    let dst_h = ((src_h as f32 * scale).round() as usize).max(1);

    let mut out = Vec::with_capacity(dst_w * dst_h * 4);
    for y in 0..dst_h {
        let sy = (y * src_h) / dst_h;
        for x in 0..dst_w {
            let sx = (x * src_w) / dst_w;
            let idx = (sy * src_w + sx) * 4;
            out.extend_from_slice(&rgba[idx..idx + 4]);
        }
    }

    (dst_w, dst_h, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_roundtrip() {
        let mut rgba = Vec::new();
        for _ in 0..100 {
            rgba.extend_from_slice(&[10, 20, 30, 255]);
        }
        let compressed = compress_rgba(&rgba);
        assert!(compressed.len() < rgba.len());
        let restored = decompress_rgba(&compressed, 10, 10);
        assert_eq!(restored, rgba);
    }

    #[test]
    fn parse_description_works() {
        assert_eq!(parse_image_description("1920x1080:0123456789abcdef"), Some((1920, 1080)));
        assert_eq!(parse_image_description("invalid"), None);
    }

    #[test]
    fn downscale_preserves_aspect_ratio() {
        let rgba = vec![0u8; 400 * 200 * 4];
        let (w, h, out) = downscale_rgba(&rgba, 400, 200, 200, 200);
        assert_eq!((w, h), (200, 100));
        assert_eq!(out.len(), 200 * 100 * 4);
    }
}
