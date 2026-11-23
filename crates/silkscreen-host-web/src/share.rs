//! Share links (design/presets.md): a preset carried in the page's
//! address, `#patch=<base64url of the preset text>`. Plain Rust, tested
//! natively; `session.rs` reads and writes the address.
//!
//! The text is compacted first: the per-param comments (each param's
//! name, for people reading a file) are dropped, which keeps links to a
//! few kilobytes.

/// The fragment's prefix.
pub const FRAGMENT: &str = "#patch=";

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `text` without comment lines or the comments after param values.
/// `name` and the other text keys keep their `#`s (they're part of the
/// text).
pub fn compact(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let is_param = trimmed
            .strip_prefix('p')
            .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
        let kept = if is_param {
            trimmed.split('#').next().unwrap_or("").trim_end()
        } else {
            trimmed
        };
        out.push_str(kept);
        out.push('\n');
    }
    out
}

/// `bytes` in unpadded base64url.
pub(crate) fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
        }
    }
    out
}

/// Unpadded (or padded) base64url back to bytes.
pub(crate) fn from_base64url(text: &str) -> Result<Vec<u8>, String> {
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let v = ALPHABET
            .iter()
            .position(|&a| a == c)
            .ok_or_else(|| format!("the link has a stray character '{}'", char::from(c)))?;
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

/// The fragment (with its `#`) that carries preset `text`.
pub fn encode_patch(text: &str) -> String {
    format!("{FRAGMENT}{}", base64url(compact(text).as_bytes()))
}

/// The preset text in address fragment `hash` (`location.hash`), if it
/// carries one: `None` for any other fragment.
pub fn decode_patch(hash: &str) -> Option<Result<String, String>> {
    let data = hash.strip_prefix(FRAGMENT)?;
    Some(
        from_base64url(data)
            .and_then(|b| String::from_utf8(b).map_err(|_| "the link isn't text".to_owned())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_round_trips_every_length() {
        for n in 0..20u8 {
            let bytes: Vec<u8> = (0..n)
                .map(|i| i.wrapping_mul(37).wrapping_add(250))
                .collect();
            let text = base64url(&bytes);
            assert!(!text.contains(['+', '/', '=']), "{text}");
            assert_eq!(from_base64url(&text).unwrap(), bytes);
        }
        assert_eq!(base64url(b"Man"), "TWFu");
        assert_eq!(base64url(b"Ma"), "TWE");
        assert_eq!(from_base64url("TWE=").unwrap(), b"Ma", "padding tolerated");
    }

    #[test]
    fn compact_drops_comments_but_not_hashes_in_text() {
        let text = "# silkscreen preset\nformat = 1\nname = Lead #2\np1 = 0.5  # Level\np3 = 2  # Wave: Pulse\n";
        assert_eq!(
            compact(text),
            "format = 1\nname = Lead #2\np1 = 0.5\np3 = 2\n"
        );
    }

    #[test]
    fn a_patch_survives_the_link() {
        let text = "# silkscreen preset\nformat = 1\nname = Wörp ✓\np1 = 0.25  # Level\n";
        let hash = encode_patch(text);
        assert!(hash.starts_with("#patch="));
        assert_eq!(decode_patch(&hash).unwrap().unwrap(), compact(text));
        assert!(decode_patch("#other").is_none());
        assert!(decode_patch("").is_none());
        let bad = decode_patch("#patch=a*b").unwrap().unwrap_err();
        assert!(bad.contains('*'), "{bad}");
    }
}
