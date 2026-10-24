#ifndef BALANCIFY_IP_PACKET_VALIDATION_H
#define BALANCIFY_IP_PACKET_VALIDATION_H

#include <stddef.h>

#define BALANCIFY_IPV6_MAX_EXTENSION_HEADERS 16u

static unsigned int BalancifyReadNetworkU16(const unsigned char *bytes) {
    return ((unsigned int)bytes[0] << 8) | (unsigned int)bytes[1];
}

static int BalancifyIsIpv6Extension(unsigned int nextHeader) {
    return nextHeader == 0u || nextHeader == 43u || nextHeader == 44u ||
        nextHeader == 51u || nextHeader == 60u || nextHeader == 135u ||
        nextHeader == 139u || nextHeader == 140u;
}

static int BalancifyIsValidTransport(
    const unsigned char *packet,
    size_t offset,
    size_t packetLength,
    unsigned int protocol,
    int fragmented) {
    size_t payloadLength;
    const unsigned char *payload;

    if (fragmented || (protocol != 6u && protocol != 17u)) return 1;
    if (offset > packetLength) return 0;
    payloadLength = packetLength - offset;
    payload = packet + offset;
    if (payloadLength < 4u) return 0;

    if (protocol == 6u) {
        size_t tcpHeaderLength;
        if (payloadLength < 20u) return 0;
        tcpHeaderLength = (size_t)(payload[12] >> 4) * 4u;
        return tcpHeaderLength >= 20u && tcpHeaderLength <= payloadLength;
    }

    if (payloadLength < 8u) return 0;
    {
        size_t udpLength = (size_t)BalancifyReadNetworkU16(payload + 4u);
        return udpLength >= 8u && udpLength == payloadLength;
    }
}

static int BalancifyIsValidIpv4Packet(
    const unsigned char *packet,
    size_t packetLength) {
    size_t headerLength = (size_t)(packet[0] & 0x0Fu) * 4u;
    size_t totalLength = (size_t)BalancifyReadNetworkU16(packet + 2u);
    unsigned int fragment = BalancifyReadNetworkU16(packet + 6u);
    size_t fragmentPayloadLength;
    int fragmented = (fragment & 0x3FFFu) != 0u;

    if (headerLength < 20u || headerLength > packetLength ||
        totalLength < headerLength || totalLength != packetLength ||
        (fragment & 0x8000u) != 0u) {
        return 0;
    }
    fragmentPayloadLength = totalLength - headerLength;
    if (fragmented && (
            (fragment & 0x4000u) != 0u ||
            fragmentPayloadLength == 0u ||
            ((fragment & 0x2000u) != 0u && (fragmentPayloadLength % 8u) != 0u))) {
        return 0;
    }
    return BalancifyIsValidTransport(
        packet, headerLength, totalLength, (unsigned int)packet[9], fragmented);
}

static int BalancifyIsValidIpv6Packet(
    const unsigned char *packet,
    size_t packetLength) {
    size_t totalLength = 40u + (size_t)BalancifyReadNetworkU16(packet + 4u);
    size_t offset = 40u;
    unsigned int nextHeader = (unsigned int)packet[6];
    unsigned int extensionCount = 0u;
    int fragmentHeaderSeen = 0;

    if (totalLength != packetLength) return 0;
    while (BalancifyIsIpv6Extension(nextHeader)) {
        unsigned int followingHeader;
        size_t extensionLength;

        extensionCount++;
        if (extensionCount > BALANCIFY_IPV6_MAX_EXTENSION_HEADERS || offset > totalLength) {
            return 0;
        }
        if (nextHeader == 44u) {
            unsigned int fragmentField;
            if (fragmentHeaderSeen || totalLength - offset < 8u) return 0;
            fragmentHeaderSeen = 1;
            followingHeader = (unsigned int)packet[offset];
            if (packet[offset + 1u] != 0u) return 0;
            fragmentField = BalancifyReadNetworkU16(packet + offset + 2u);
            if ((fragmentField & 0x0006u) != 0u) return 0;
            if ((fragmentField & 0xFFF9u) != 0u) {
                size_t fragmentPayloadLength = totalLength - offset - 8u;
                if (fragmentPayloadLength == 0u ||
                    ((fragmentField & 0x0001u) != 0u && (fragmentPayloadLength % 8u) != 0u)) {
                    return 0;
                }
                // Non-atomic fragments do not need a transport header here.
                // The service's bounded parser applies the same rule.
                return 1;
            }
            nextHeader = followingHeader;
            offset += 8u;
            continue;
        }

        if (totalLength - offset < 2u) return 0;
        followingHeader = (unsigned int)packet[offset];
        if (nextHeader == 51u) {
            extensionLength = ((size_t)packet[offset + 1u] + 2u) * 4u;
        } else {
            extensionLength = ((size_t)packet[offset + 1u] + 1u) * 8u;
        }
        if (extensionLength < 8u || extensionLength > totalLength - offset) return 0;
        nextHeader = followingHeader;
        offset += extensionLength;
    }

    return BalancifyIsValidTransport(packet, offset, totalLength, nextHeader, 0);
}

/*
 * Validate raw IPv4/IPv6 boundaries and TCP/UDP header lengths before a packet
 * enters the driver-owned NBL or TX queue. Checksum validation and IPv6
 * jumbograms are intentionally outside this bounded parser; the user-mode
 * service revalidates dequeued packets before tunnel processing.
 */
static int BalancifyIsValidIpPacket(
    const unsigned char *packet,
    size_t packetLength,
    size_t mtu) {
    unsigned int version;

    if (!packet || packetLength < 20u || packetLength > mtu) return 0;
    version = (unsigned int)(packet[0] >> 4);
    if (version == 4u) return BalancifyIsValidIpv4Packet(packet, packetLength);
    if (version == 6u) {
        if (packetLength < 40u) return 0;
        return BalancifyIsValidIpv6Packet(packet, packetLength);
    }
    return 0;
}

#endif /* BALANCIFY_IP_PACKET_VALIDATION_H */
