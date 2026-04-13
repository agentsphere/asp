// Copyright (c) 2026 Steven Hooker. Exclusively licensed to and distributed by AgentSphere GmbH.
// SPDX-License-Identifier: BUSL-1.1

//! Git pkt-line protocol helpers.
//!
//! Used by both the smart HTTP and SSH git transports.

/// Scan a buffer for the `0000` flush-pkt that terminates the ref command section.
///
/// Walks pkt-lines to find it. Returns the byte position immediately after the
/// flush-pkt, or `None` if not yet fully received.
pub fn find_flush_pkt(buf: &[u8]) -> Option<usize> {
    let mut pos = 0;
    while pos + 4 <= buf.len() {
        let len_hex = &buf[pos..pos + 4];
        if len_hex == b"0000" {
            return Some(pos + 4);
        }
        let Ok(len_str) = std::str::from_utf8(len_hex) else {
            return None;
        };
        let pkt_len = match usize::from_str_radix(len_str, 16) {
            Ok(n) if n >= 4 => n,
            _ => return None,
        };
        if pos + pkt_len > buf.len() {
            return None; // Incomplete pkt-line, need more data
        }
        pos += pkt_len;
    }
    None
}

/// Build pkt-line header for info/refs response.
///
/// Returns the announcement pkt-line + flush-pkt for the given service name.
pub fn pkt_line_header(service: &str) -> Vec<u8> {
    let announcement = format!("# service={service}\n");
    let pkt_len = announcement.len() + 4; // 4 bytes for the length prefix itself
    let mut buf = Vec::new();
    buf.extend_from_slice(format!("{pkt_len:04x}").as_bytes());
    buf.extend_from_slice(announcement.as_bytes());
    buf.extend_from_slice(b"0000"); // flush-pkt
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- pkt_line_header --

    #[test]
    fn pkt_line_header_upload_pack() {
        let header = pkt_line_header("git-upload-pack");
        let s = String::from_utf8(header).unwrap();
        // "# service=git-upload-pack\n" = 26 chars + 4 hex prefix = 30
        assert!(s.starts_with("001e"));
        assert!(s.contains("# service=git-upload-pack\n"));
        assert!(s.ends_with("0000"));
    }

    #[test]
    fn pkt_line_header_receive_pack() {
        let header = pkt_line_header("git-receive-pack");
        let s = String::from_utf8(header).unwrap();
        assert!(s.contains("# service=git-receive-pack\n"));
        assert!(s.ends_with("0000"));
    }

    #[test]
    fn pkt_line_header_custom_service() {
        let header = pkt_line_header("custom-service");
        let s = String::from_utf8(header).unwrap();
        assert!(s.contains("# service=custom-service\n"));
        assert!(s.ends_with("0000"));
        let announcement = "# service=custom-service\n";
        let expected_len = announcement.len() + 4;
        let hex_prefix = format!("{expected_len:04x}");
        assert!(s.starts_with(&hex_prefix));
    }

    #[test]
    fn pkt_line_header_length_is_correct() {
        let header = pkt_line_header("git-upload-pack");
        let s = String::from_utf8(header.clone()).unwrap();
        let hex_prefix = &s[..4];
        let expected_len = "# service=git-upload-pack\n".len() + 4;
        assert_eq!(
            hex_prefix,
            format!("{expected_len:04x}"),
            "length prefix should match"
        );
        // Total header size = pkt-line + flush-pkt
        assert_eq!(header.len(), expected_len + 4); // +4 for "0000"
    }

    // -- find_flush_pkt --

    #[test]
    fn find_flush_pkt_simple() {
        assert_eq!(find_flush_pkt(b"0000"), Some(4));
    }

    #[test]
    fn find_flush_pkt_after_one_command() {
        // pkt-line: "0010" (16 bytes) + 12 bytes of data + "0000"
        let mut buf = b"0010".to_vec();
        buf.extend_from_slice(&[b'x'; 12]); // 16 - 4 = 12 data bytes
        buf.extend_from_slice(b"0000");
        assert_eq!(find_flush_pkt(&buf), Some(20)); // 16 + 4
    }

    #[test]
    fn find_flush_pkt_after_multiple_commands() {
        // Two pkt-lines of 8 bytes each, then flush
        let mut buf = b"0008".to_vec(); // 8-byte pkt-line
        buf.extend_from_slice(&[b'a'; 4]); // 4 data bytes
        buf.extend_from_slice(b"0008"); // another 8-byte pkt-line
        buf.extend_from_slice(&[b'b'; 4]);
        buf.extend_from_slice(b"0000");
        assert_eq!(find_flush_pkt(&buf), Some(20)); // 8 + 8 + 4
    }

    #[test]
    fn find_flush_pkt_incomplete() {
        let mut buf = b"0010".to_vec();
        buf.extend_from_slice(&[b'x'; 6]); // Only 10 total, need 16
        assert_eq!(find_flush_pkt(&buf), None);
    }

    #[test]
    fn find_flush_pkt_empty() {
        assert_eq!(find_flush_pkt(b""), None);
    }

    #[test]
    fn find_flush_pkt_with_trailing_pack_data() {
        let mut buf = b"0000".to_vec();
        buf.extend_from_slice(b"PACK\x00\x00\x00\x02");
        assert_eq!(find_flush_pkt(&buf), Some(4));
    }

    #[test]
    fn find_flush_pkt_invalid_utf8() {
        let buf: &[u8] = &[0xFF, 0xFE, 0xFD, 0xFC];
        assert_eq!(find_flush_pkt(buf), None);
    }

    #[test]
    fn find_flush_pkt_invalid_hex_length() {
        assert_eq!(find_flush_pkt(b"zzzz"), None);
    }

    #[test]
    fn find_flush_pkt_length_less_than_4() {
        assert_eq!(find_flush_pkt(b"0003xxx"), None);
        assert_eq!(find_flush_pkt(b"0001xxxx"), None);
        assert_eq!(find_flush_pkt(b"0002xxxx"), None);
    }

    #[test]
    fn find_flush_pkt_three_bytes_only() {
        assert_eq!(find_flush_pkt(b"000"), None);
    }

    #[test]
    fn find_flush_pkt_exactly_4_bytes_pkt() {
        // "0004" is the minimum valid pkt-line (4 bytes total, 0 data bytes)
        let mut buf = b"0004".to_vec();
        buf.extend_from_slice(b"0000");
        assert_eq!(find_flush_pkt(&buf), Some(8)); // 4 + 4
    }

    #[test]
    fn find_flush_pkt_flush_not_at_pkt_boundary() {
        // "0008" = 8 bytes total (4 len + 4 data "0000").
        // The "0000" inside the payload is NOT a flush packet.
        let buf = b"00080000";
        assert_eq!(find_flush_pkt(buf), None);
    }

    #[test]
    fn find_flush_pkt_data_contains_0000_pattern() {
        // "000c" = 12 bytes total (4 length + 8 data).
        let mut buf = b"000c".to_vec();
        buf.extend_from_slice(b"xx0000yy"); // 8 data bytes containing "0000"
        buf.extend_from_slice(b"0000"); // actual flush
        assert_eq!(find_flush_pkt(&buf), Some(16)); // 12 + 4
    }

    #[test]
    fn find_flush_pkt_large_pkt_line() {
        // "0100" = 256 bytes total pkt-line
        let mut buf = b"0100".to_vec();
        buf.extend_from_slice(&vec![b'x'; 252]); // 256 - 4 = 252 data bytes
        buf.extend_from_slice(b"0000");
        assert_eq!(find_flush_pkt(&buf), Some(260)); // 256 + 4
    }

    #[test]
    fn find_flush_pkt_flush_immediately_at_start() {
        assert_eq!(find_flush_pkt(b"0000"), Some(4));
    }

    #[test]
    fn find_flush_pkt_multiple_flushes() {
        let mut buf = b"0000".to_vec();
        buf.extend_from_slice(b"0000");
        assert_eq!(find_flush_pkt(&buf), Some(4)); // first one found
    }
}
