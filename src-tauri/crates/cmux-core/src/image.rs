//! Recognizing an image from its bytes.

/// What a stored image may be, as the extension its files take. Checked
/// against the bytes themselves rather than a header or a file extension,
/// so nothing but a raster image is ever written to disk or handed to the
/// webview — an SVG, which can carry script, does not qualify. Release-note
/// images and pasted ones both go through it.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG) {
        return Some("png");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("jpeg");
    }
    if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if bytes.len() > 12 && &bytes[4..12] == b"ftypavif" {
        return Some("avif");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_raster_image_bytes_are_accepted() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(sniff_image(b"GIF89a....."), Some("gif"));
        assert_eq!(sniff_image(b"GIF87a....."), Some("gif"));
        assert_eq!(sniff_image(&[0xff, 0xd8, 0xff, 0xe0, 0x00]), Some("jpeg"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff_image(b"\0\0\0\x20ftypavif\0\0"), Some("avif"));

        // An SVG can carry script, so it is not something this will store
        // or hand to the webview, whatever the server called it.
        assert_eq!(sniff_image(b"<svg xmlns=\"http://www.w3.org/2000/svg\">"), None);
        assert_eq!(sniff_image(b"<!doctype html><script>"), None);
        assert_eq!(sniff_image(b""), None);
        assert_eq!(sniff_image(b"RIFF"), None, "a truncated header is not a webp");
    }
}
