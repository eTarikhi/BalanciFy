#ifndef BALANCIFY_IP_PACKET_VALIDATION_H
#define BALANCIFY_IP_PACKET_VALIDATION_H

#include <stddef.h>

/*
 * Validate a complete raw IPv4 or IPv6 packet before the miniport retains it.
 * This helper deliberately has no WDK dependency so the parsing contract can
 * be exercised by the portable host-side unit test.
 */
static int BalancifyIsValidIpPacket(
    const unsigned char *packet,
    size_t packetLength,
    size_t mtu) {
    unsigned int version;

    if (!packet || packetLength < 20 || packetLength > mtu) return 0;
    version = (unsigned int)(packet[0] >> 4);
    if (version == 4) {
        size_t headerLength = (size_t)(packet[0] & 0x0F) * 4;
        size_t totalLength = ((size_t)packet[2] << 8) | packet[3];
        return headerLength >= 20 && headerLength <= packetLength &&
            totalLength >= headerLength && totalLength == packetLength;
    }
    if (version == 6) {
        size_t totalLength;
        if (packetLength < 40) return 0;
        totalLength = 40 + (((size_t)packet[4] << 8) | packet[5]);
        return totalLength == packetLength;
    }
    return 0;
}

#endif /* BALANCIFY_IP_PACKET_VALIDATION_H */
