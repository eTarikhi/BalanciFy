//! Bounded raw-IP packet parsing for the service's eventual miniport adapter.
//!
//! The driver exposes packets on an IP-only NDIS medium. This parser extracts
//! the flow key without depending on Windows APIs or allocating based on packet
//! fields. It does not reassemble fragments; all fragments of a fragmented
//! datagram use the source/destination/protocol tuple with zero ports.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use balancify_core::FlowKey;

/// Largest inner packet accepted by the current driver contract.
pub const MAX_INNER_PACKET_LEN: usize = 9000;
const IPV4_MIN_HEADER_LEN: usize = 20;
const IPV6_HEADER_LEN: usize = 40;
const MAX_IPV6_EXTENSION_HEADERS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketFlowError {
    EmptyOrTruncated,
    Oversized,
    UnsupportedIpVersion,
    InvalidIpv4Header,
    InvalidIpv6Header,
    LengthMismatch,
    UnsupportedIpv6Jumbogram,
    InvalidIpv6Extension,
    TooManyIpv6ExtensionHeaders,
    InvalidTransportHeader,
}

impl std::fmt::Display for PacketFlowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::EmptyOrTruncated => "packet is empty or truncated",
            Self::Oversized => "packet exceeds the configured inner-packet limit",
            Self::UnsupportedIpVersion => "packet has an unsupported IP version",
            Self::InvalidIpv4Header => "IPv4 header is malformed",
            Self::InvalidIpv6Header => "IPv6 header is malformed",
            Self::LengthMismatch => "IP-declared length does not match packet length",
            Self::UnsupportedIpv6Jumbogram => "IPv6 jumbograms are not supported",
            Self::InvalidIpv6Extension => "IPv6 extension header is malformed",
            Self::TooManyIpv6ExtensionHeaders => "IPv6 extension chain exceeds the parser limit",
            Self::InvalidTransportHeader => "TCP or UDP header is malformed",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for PacketFlowError {}

/// Extract a directional flow key from one complete raw IPv4 or IPv6 packet.
///
/// IP total/payload lengths must exactly cover `packet`. TCP and UDP headers
/// are bounds-checked before their ports are read. IPv6 extension traversal is
/// capped at 16 headers. Fragmented packets deliberately share a zero-port
/// tuple because the non-initial fragment does not contain transport ports.
pub fn flow_key_from_packet(packet: &[u8]) -> Result<FlowKey, PacketFlowError> {
    if packet.is_empty() {
        return Err(PacketFlowError::EmptyOrTruncated);
    }
    if packet.len() > MAX_INNER_PACKET_LEN {
        return Err(PacketFlowError::Oversized);
    }

    match packet[0] >> 4 {
        4 => parse_ipv4(packet),
        6 => parse_ipv6(packet),
        _ => Err(PacketFlowError::UnsupportedIpVersion),
    }
}

fn valid_ipv4_options(packet: &[u8], header_len: usize) -> bool {
    let mut offset = IPV4_MIN_HEADER_LEN;
    while offset < header_len {
        match packet[offset] {
            0 => return packet[offset + 1..header_len].iter().all(|byte| *byte == 0),
            1 => offset += 1,
            _ => {
                if header_len - offset < 2 {
                    return false;
                }
                let option_len = usize::from(packet[offset + 1]);
                if option_len < 2 || option_len > header_len - offset {
                    return false;
                }
                offset += option_len;
            }
        }
    }
    true
}

fn parse_ipv4(packet: &[u8]) -> Result<FlowKey, PacketFlowError> {
    if packet.len() < IPV4_MIN_HEADER_LEN {
        return Err(PacketFlowError::EmptyOrTruncated);
    }
    let header_len = usize::from(packet[0] & 0x0f) * 4;
    if header_len < IPV4_MIN_HEADER_LEN || header_len > packet.len() {
        return Err(PacketFlowError::InvalidIpv4Header);
    }
    let total_len = usize::from(read_u16(packet, 2)?);
    if total_len != packet.len() || total_len < header_len {
        return Err(PacketFlowError::LengthMismatch);
    }
    if !valid_ipv4_options(packet, header_len) {
        return Err(PacketFlowError::InvalidIpv4Header);
    }
    let fragment = read_u16(packet, 6)?;
    if fragment & 0x8000 != 0 {
        return Err(PacketFlowError::InvalidIpv4Header);
    }
    let fragmented = fragment & 0x3fff != 0;
    let fragment_payload_len = total_len - header_len;
    if fragmented
        && ((fragment & 0x4000 != 0)
            || fragment_payload_len == 0
            || (fragment & 0x2000 != 0 && fragment_payload_len % 8 != 0))
    {
        return Err(PacketFlowError::InvalidIpv4Header);
    }
    let protocol = packet[9];
    let source = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
    let destination = Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]);
    let (source_port, destination_port) = transport_ports(packet, header_len, total_len, protocol, fragmented)?;

    Ok(FlowKey::new(
        protocol,
        SocketAddr::new(IpAddr::V4(source), source_port),
        SocketAddr::new(IpAddr::V4(destination), destination_port),
    ))
}

fn parse_ipv6(packet: &[u8]) -> Result<FlowKey, PacketFlowError> {
    if packet.len() < IPV6_HEADER_LEN {
        return Err(PacketFlowError::EmptyOrTruncated);
    }
    let payload_len = usize::from(read_u16(packet, 4)?);
    if payload_len == 0 && packet.len() > IPV6_HEADER_LEN {
        return Err(PacketFlowError::UnsupportedIpv6Jumbogram);
    }
    let total_len = IPV6_HEADER_LEN
        .checked_add(payload_len)
        .ok_or(PacketFlowError::LengthMismatch)?;
    if total_len != packet.len() {
        return Err(PacketFlowError::LengthMismatch);
    }
    let source_bytes: [u8; 16] = packet[8..24]
        .try_into()
        .map_err(|_| PacketFlowError::InvalidIpv6Header)?;
    let destination_bytes: [u8; 16] = packet[24..40]
        .try_into()
        .map_err(|_| PacketFlowError::InvalidIpv6Header)?;
    let source = Ipv6Addr::from(source_bytes);
    let destination = Ipv6Addr::from(destination_bytes);

    let mut next_header = packet[6];
    let mut offset = IPV6_HEADER_LEN;
    let mut extension_count = 0usize;
    let mut fragment_header_seen = false;
    loop {
        if !is_ipv6_extension(next_header) {
            break;
        }
        extension_count += 1;
        if extension_count > MAX_IPV6_EXTENSION_HEADERS {
            return Err(PacketFlowError::TooManyIpv6ExtensionHeaders);
        }
        if next_header == 44 {
            if fragment_header_seen {
                return Err(PacketFlowError::InvalidIpv6Extension);
            }
            fragment_header_seen = true;
            let end = offset.checked_add(8).ok_or(PacketFlowError::InvalidIpv6Extension)?;
            if end > total_len {
                return Err(PacketFlowError::InvalidIpv6Extension);
            }
            let following = packet[offset];
            if packet[offset + 1] != 0 {
                return Err(PacketFlowError::InvalidIpv6Extension);
            }
            let fragment_field = read_u16(packet, offset + 2)
                .map_err(|_| PacketFlowError::InvalidIpv6Extension)?;
            if fragment_field & 0x0006 != 0 {
                return Err(PacketFlowError::InvalidIpv6Extension);
            }
            let is_fragmented = fragment_field & 0xfff9 != 0;
            if is_fragmented {
                let fragment_payload_len = total_len - end;
                if fragment_payload_len == 0
                    || (fragment_field & 0x0001 != 0 && fragment_payload_len % 8 != 0)
                {
                    return Err(PacketFlowError::InvalidIpv6Extension);
                }
                // All fragments carry the same immediate post-fragment
                // protocol value, even when an extension header follows it.
                return Ok(FlowKey::new(
                    following,
                    SocketAddr::new(IpAddr::V6(source), 0),
                    SocketAddr::new(IpAddr::V6(destination), 0),
                ));
            }
            next_header = following;
            offset = end;
            continue;
        }

        let first = *packet.get(offset).ok_or(PacketFlowError::InvalidIpv6Extension)?;
        let second = *packet.get(offset + 1).ok_or(PacketFlowError::InvalidIpv6Extension)?;
        let header_len = if next_header == 51 {
            (usize::from(second) + 2) * 4 // Authentication Header
        } else {
            (usize::from(second) + 1) * 8 // Hop-by-Hop, Routing, Destination, etc.
        };
        let end = offset
            .checked_add(header_len)
            .ok_or(PacketFlowError::InvalidIpv6Extension)?;
        if header_len < 8 || end > total_len {
            return Err(PacketFlowError::InvalidIpv6Extension);
        }
        next_header = first;
        offset = end;
    }

    let (source_port, destination_port) =
        transport_ports(packet, offset, total_len, next_header, false)?;
    Ok(FlowKey::new(
        next_header,
        SocketAddr::new(IpAddr::V6(source), source_port),
        SocketAddr::new(IpAddr::V6(destination), destination_port),
    ))
}

fn is_ipv6_extension(next_header: u8) -> bool {
    matches!(next_header, 0 | 43 | 44 | 51 | 60 | 135 | 139 | 140)
}

fn transport_ports(
    packet: &[u8],
    offset: usize,
    end: usize,
    protocol: u8,
    fragmented: bool,
) -> Result<(u16, u16), PacketFlowError> {
    if fragmented || !matches!(protocol, 6 | 17) {
        return Ok((0, 0));
    }
    if offset > end || end > packet.len() {
        return Err(PacketFlowError::InvalidTransportHeader);
    }
    let payload = &packet[offset..end];
    if payload.len() < 4 {
        return Err(PacketFlowError::InvalidTransportHeader);
    }
    let source = read_u16(payload, 0).map_err(|_| PacketFlowError::InvalidTransportHeader)?;
    let destination = read_u16(payload, 2).map_err(|_| PacketFlowError::InvalidTransportHeader)?;
    match protocol {
        6 => {
            if payload.len() < 20 {
                return Err(PacketFlowError::InvalidTransportHeader);
            }
            let header_len = usize::from(payload[12] >> 4) * 4;
            if header_len < 20 || header_len > payload.len() {
                return Err(PacketFlowError::InvalidTransportHeader);
            }
        }
        17 => {
            if payload.len() < 8 {
                return Err(PacketFlowError::InvalidTransportHeader);
            }
            let udp_len = usize::from(
                read_u16(payload, 4).map_err(|_| PacketFlowError::InvalidTransportHeader)?,
            );
            if udp_len < 8 || udp_len != payload.len() {
                return Err(PacketFlowError::InvalidTransportHeader);
            }
        }
        _ => unreachable!("protocol was checked above"),
    }
    Ok((source, destination))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, PacketFlowError> {
    let pair = bytes
        .get(offset..offset + 2)
        .ok_or(PacketFlowError::EmptyOrTruncated)?;
    Ok(u16::from_be_bytes([pair[0], pair[1]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tcp_header(source: u16, destination: u16) -> [u8; 20] {
        let mut header = [0u8; 20];
        header[..2].copy_from_slice(&source.to_be_bytes());
        header[2..4].copy_from_slice(&destination.to_be_bytes());
        header[12] = 0x50;
        header
    }

    fn udp_header(source: u16, destination: u16) -> [u8; 8] {
        let mut header = [0u8; 8];
        header[..2].copy_from_slice(&source.to_be_bytes());
        header[2..4].copy_from_slice(&destination.to_be_bytes());
        header[4..6].copy_from_slice(&8u16.to_be_bytes());
        header
    }

    fn ipv4(protocol: u8, payload: &[u8], fragment: u16, options: bool) -> Vec<u8> {
        let header_len = if options { 24 } else { 20 };
        let total_len = (header_len + payload.len()) as u16;
        let mut packet = vec![0u8; usize::from(total_len)];
        packet[0] = 0x40 | (header_len as u8 / 4);
        packet[2..4].copy_from_slice(&total_len.to_be_bytes());
        packet[6..8].copy_from_slice(&fragment.to_be_bytes());
        packet[9] = protocol;
        packet[12..16].copy_from_slice(&[10, 1, 2, 3]);
        packet[16..20].copy_from_slice(&[198, 51, 100, 9]);
        packet[header_len..].copy_from_slice(payload);
        packet
    }

    fn ipv6(next_header: u8, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; IPV6_HEADER_LEN + payload.len()];
        packet[0] = 0x60;
        packet[4..6].copy_from_slice(&(payload.len() as u16).to_be_bytes());
        packet[6] = next_header;
        packet[8..24].copy_from_slice(&Ipv6Addr::LOCALHOST.octets());
        packet[24..40].copy_from_slice(&Ipv6Addr::UNSPECIFIED.octets());
        packet[IPV6_HEADER_LEN..].copy_from_slice(payload);
        packet
    }

    #[test]
    fn parses_ipv4_tcp_and_udp_ports_with_options() {
        let tcp = ipv4(6, &tcp_header(1234, 443), 0, true);
        let key = flow_key_from_packet(&tcp).unwrap();
        assert_eq!(key.proto, 6);
        assert_eq!(key.src.to_string(), "10.1.2.3:1234");
        assert_eq!(key.dst.to_string(), "198.51.100.9:443");

        let udp = ipv4(17, &udp_header(53000, 53), 0, false);
        let key = flow_key_from_packet(&udp).unwrap();
        assert_eq!(key.proto, 17);
        assert_eq!(key.src.port(), 53000);
        assert_eq!(key.dst.port(), 53);
    }

    #[test]
    fn fragments_share_a_portless_ipv4_flow_key() {
        let mut first_payload = tcp_header(1234, 443).to_vec();
        first_payload.extend_from_slice(&[0; 4]); // 24-byte non-final fragment payload
        let first = ipv4(6, &first_payload, 0x2000, false);
        let later = ipv4(6, &[0xaa; 8], 1, false);
        let first_key = flow_key_from_packet(&first).unwrap();
        let later_key = flow_key_from_packet(&later).unwrap();
        assert_eq!(first_key, later_key);
        assert_eq!(first_key.src.port(), 0);
        assert_eq!(first_key.dst.port(), 0);
    }

    #[test]
    fn rejects_invalid_ipv4_fragment_payload_lengths_and_df() {
        assert_eq!(
            flow_key_from_packet(&ipv4(17, &[], 0x2000, false)),
            Err(PacketFlowError::InvalidIpv4Header),
        );
        assert_eq!(
            flow_key_from_packet(&ipv4(17, &[0; 9], 0x2000, false)),
            Err(PacketFlowError::InvalidIpv4Header),
        );
        assert_eq!(
            flow_key_from_packet(&ipv4(17, &[0; 8], 0x6000, false)),
            Err(PacketFlowError::InvalidIpv4Header),
        );
        assert!(flow_key_from_packet(&ipv4(17, &[0xaa], 1, false)).is_ok());
    }

    #[test]
    fn parses_ipv6_transport_after_destination_options() {
        let mut payload = vec![6, 0, 0, 0, 0, 0, 0, 0];
        payload.extend_from_slice(&tcp_header(40000, 8443));
        let key = flow_key_from_packet(&ipv6(60, &payload)).unwrap();
        assert_eq!(key.proto, 6);
        assert_eq!(key.src, "[::1]:40000".parse().unwrap());
        assert_eq!(key.dst, "[::]:8443".parse().unwrap());
    }

    #[test]
    fn ipv6_unfragmented_udp_length_must_cover_exact_payload() {
        let valid = ipv6(17, &udp_header(53000, 53));
        assert!(flow_key_from_packet(&valid).is_ok());

        let mut short_length = valid.clone();
        short_length[44..46].copy_from_slice(&7u16.to_be_bytes());
        assert_eq!(
            flow_key_from_packet(&short_length),
            Err(PacketFlowError::InvalidTransportHeader),
        );

        let mut trailing_bytes = udp_header(53000, 53).to_vec();
        trailing_bytes.extend_from_slice(&[0; 4]);
        let mismatched_ip_payload = ipv6(17, &trailing_bytes);
        assert_eq!(
            flow_key_from_packet(&mismatched_ip_payload),
            Err(PacketFlowError::InvalidTransportHeader),
        );
    }

    #[test]
    fn ipv6_fragmented_packets_use_a_consistent_portless_key() {
        let mut first_fragment = vec![6, 0, 0, 1, 0, 0, 0, 1]; // M bit set
        first_fragment.extend_from_slice(&tcp_header(1234, 443));
        first_fragment.extend_from_slice(&[0; 4]); // 24-byte non-final fragment payload
        let later_fragment = vec![6, 0, 0, 8, 0, 0, 0, 1, 0xaa, 0xbb];
        let first = flow_key_from_packet(&ipv6(44, &first_fragment)).unwrap();
        let later = flow_key_from_packet(&ipv6(44, &later_fragment)).unwrap();
        assert_eq!(first, later);
        assert_eq!(first.proto, 6);
        assert_eq!(first.src.port(), 0);
    }

    #[test]
    fn rejects_nonzero_ipv6_fragment_reserved_fields() {
        let valid_fragment = vec![6, 0, 0, 1, 0, 0, 0, 1, 0xaa, 0xbb];

        let mut reserved_octet = valid_fragment.clone();
        reserved_octet[1] = 1;
        assert_eq!(
            flow_key_from_packet(&ipv6(44, &reserved_octet)),
            Err(PacketFlowError::InvalidIpv6Extension),
        );

        let mut reserved_bits = valid_fragment;
        reserved_bits[3] |= 0x02;
        assert_eq!(
            flow_key_from_packet(&ipv6(44, &reserved_bits)),
            Err(PacketFlowError::InvalidIpv6Extension),
        );
    }

    #[test]
    fn rejects_invalid_ipv6_nonfinal_fragment_payload_lengths() {
        let empty_nonfinal_fragment = [17, 0, 0, 1, 0, 0, 0, 0];
        assert_eq!(
            flow_key_from_packet(&ipv6(44, &empty_nonfinal_fragment)),
            Err(PacketFlowError::InvalidIpv6Extension),
        );

        let mut short_nonfinal_fragment = empty_nonfinal_fragment.to_vec();
        short_nonfinal_fragment.extend_from_slice(&[0xaa; 7]);
        assert_eq!(
            flow_key_from_packet(&ipv6(44, &short_nonfinal_fragment)),
            Err(PacketFlowError::InvalidIpv6Extension),
        );

        let mut aligned_nonfinal_fragment = empty_nonfinal_fragment.to_vec();
        aligned_nonfinal_fragment.extend_from_slice(&[0xaa; 8]);
        assert!(flow_key_from_packet(&ipv6(44, &aligned_nonfinal_fragment)).is_ok());
    }

    #[test]
    fn rejects_duplicate_ipv6_fragment_headers() {
        let duplicate_fragments = [
            44, 0, 0, 0, 0, 0, 0, 0,
            59, 0, 0, 0, 0, 0, 0, 0,
        ];
        assert_eq!(
            flow_key_from_packet(&ipv6(44, &duplicate_fragments)),
            Err(PacketFlowError::InvalidIpv6Extension),
        );
    }

    #[test]
    fn rejects_malformed_ipv4_option_lengths_and_padding() {
        let mut valid = ipv4(59, &[], 0, true);
        valid[20..24].copy_from_slice(&[1, 0, 0, 0]); // NOP, then zero padding
        assert!(flow_key_from_packet(&valid).is_ok());

        for options in [
            [0x9e, 1, 0, 0], // TLV length is shorter than its header
            [0x9e, 5, 0, 0], // TLV extends beyond the option area
            [0x9e, 0, 0, 0], // TLV length zero
            [0x9e, 3, 0, 0x9e], // no length byte for the final TLV
            [0, 1, 0, 0], // nonzero byte after End of Option List
        ] {
            let mut malformed = ipv4(59, &[], 0, true);
            malformed[20..24].copy_from_slice(&options);
            assert_eq!(
                flow_key_from_packet(&malformed),
                Err(PacketFlowError::InvalidIpv4Header),
                "malformed IPv4 options must be rejected: {options:?}",
            );
        }
    }

    #[test]
    fn rejects_malformed_lengths_transport_and_extension_chains() {
        let mut packet = ipv4(17, &udp_header(1, 2), 0, false);
        packet[2..4].copy_from_slice(&20u16.to_be_bytes());
        assert_eq!(flow_key_from_packet(&packet), Err(PacketFlowError::LengthMismatch));

        let short_tcp = ipv4(6, &[0; 10], 0, false);
        assert_eq!(flow_key_from_packet(&short_tcp), Err(PacketFlowError::InvalidTransportHeader));

        let mut udp_with_trailing_ip_payload = ipv4(17, &[0; 12], 0, false);
        udp_with_trailing_ip_payload[20..22].copy_from_slice(&1u16.to_be_bytes());
        udp_with_trailing_ip_payload[22..24].copy_from_slice(&2u16.to_be_bytes());
        udp_with_trailing_ip_payload[24..26].copy_from_slice(&8u16.to_be_bytes());
        assert_eq!(
            flow_key_from_packet(&udp_with_trailing_ip_payload),
            Err(PacketFlowError::InvalidTransportHeader),
            "an unfragmented UDP datagram must exactly cover its IP payload",
        );

        let mut extension_chain = Vec::new();
        for _ in 0..=MAX_IPV6_EXTENSION_HEADERS {
            extension_chain.extend_from_slice(&[60, 0, 0, 0, 0, 0, 0, 0]);
        }
        let packet = ipv6(60, &extension_chain);
        assert_eq!(flow_key_from_packet(&packet), Err(PacketFlowError::TooManyIpv6ExtensionHeaders));
    }

    #[test]
    fn rejects_empty_oversized_unknown_version_and_jumbograms() {
        assert_eq!(flow_key_from_packet(&[]), Err(PacketFlowError::EmptyOrTruncated));
        assert_eq!(flow_key_from_packet(&[0x70]), Err(PacketFlowError::UnsupportedIpVersion));
        assert_eq!(flow_key_from_packet(&vec![0; MAX_INNER_PACKET_LEN + 1]), Err(PacketFlowError::Oversized));
        let mut jumbo = ipv6(59, &[0]);
        jumbo[4..6].fill(0);
        assert_eq!(flow_key_from_packet(&jumbo), Err(PacketFlowError::UnsupportedIpv6Jumbogram));
    }
}
