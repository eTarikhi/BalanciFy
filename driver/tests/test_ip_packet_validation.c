#include <assert.h>
#include <string.h>
#include <stdint.h>
#include "../src/ip_packet_validation.h"

static void test_ipv4_minimum_packet(void) {
    unsigned char packet[20] = { 0x45, 0, 0, 20 };
    assert(BalancifyIsValidIpPacket(packet, sizeof(packet), 1500));
}

static void test_ipv4_options_and_payload(void) {
    unsigned char packet[27] = { 0x46, 0, 0, 27 };
    assert(BalancifyIsValidIpPacket(packet, sizeof(packet), 1500));
}

static void test_ipv4_rejects_bad_lengths_and_reserved_flag(void) {
    unsigned char packet[28] = { 0x45, 0, 0, 27 };
    assert(!BalancifyIsValidIpPacket(packet, sizeof(packet), 1500));
    packet[0] = 0x44; /* IHL smaller than the IPv4 minimum */
    assert(!BalancifyIsValidIpPacket(packet, 20, 1500));
    packet[0] = 0x4F; /* IHL extends beyond supplied packet */
    assert(!BalancifyIsValidIpPacket(packet, 28, 1500));
    packet[0] = 0x45;
    packet[2] = 0;
    packet[3] = 19; /* total length smaller than header */
    assert(!BalancifyIsValidIpPacket(packet, 20, 1500));
    packet[2] = 0;
    packet[3] = 28;
    packet[6] = 0x80; /* reserved IPv4 fragment flag */
    assert(!BalancifyIsValidIpPacket(packet, 28, 1500));
}

static void test_ipv4_tcp_udp_and_fragment_bounds(void) {
    unsigned char tcp[40] = { 0x45, 0, 0, 40 };
    tcp[9] = 6;
    tcp[32] = 0x50; /* data offset: five 32-bit words */
    assert(BalancifyIsValidIpPacket(tcp, sizeof(tcp), 1500));
    tcp[32] = 0x40; /* TCP header shorter than the fixed header */
    assert(!BalancifyIsValidIpPacket(tcp, sizeof(tcp), 1500));

    {
        unsigned char udp[32] = { 0x45, 0, 0, 32 };
        udp[9] = 17;
        udp[24] = 0;
        udp[25] = 12;
        assert(BalancifyIsValidIpPacket(udp, sizeof(udp), 1500));
        udp[25] = 8; /* UDP length must cover the exact unfragmented IP payload */
        assert(!BalancifyIsValidIpPacket(udp, sizeof(udp), 1500));
        udp[25] = 13; /* UDP length cannot exceed the IP payload */
        assert(!BalancifyIsValidIpPacket(udp, sizeof(udp), 1500));
    }

    {
        unsigned char first_fragment[28] = { 0x45, 0, 0, 28 };
        first_fragment[6] = 0x20; /* More Fragments; transport header may be split */
        first_fragment[9] = 6;
        assert(BalancifyIsValidIpPacket(first_fragment, sizeof(first_fragment), 1500));
        first_fragment[6] = 0x60; /* DF and fragmentation cannot be combined */
        assert(!BalancifyIsValidIpPacket(first_fragment, sizeof(first_fragment), 1500));
    }
    {
        unsigned char empty_fragment[20] = { 0x45, 0, 0, 20 };
        unsigned char unaligned_fragment[29] = { 0x45, 0, 0, 29 };
        empty_fragment[6] = 0x20;
        unaligned_fragment[6] = 0x20;
        assert(!BalancifyIsValidIpPacket(empty_fragment, sizeof(empty_fragment), 1500));
        assert(!BalancifyIsValidIpPacket(unaligned_fragment, sizeof(unaligned_fragment), 1500));
    }
    {
        unsigned char options[24] = { 0x46, 0, 0, 24 };
        options[9] = 59; /* No transport header */
        options[20] = 1; /* NOP followed by zero padding */
        assert(BalancifyIsValidIpPacket(options, sizeof(options), 1500));
        options[20] = 0x9e;
        options[21] = 1; /* TLV length shorter than its header */
        assert(!BalancifyIsValidIpPacket(options, sizeof(options), 1500));
        options[21] = 5; /* TLV overruns the option area */
        assert(!BalancifyIsValidIpPacket(options, sizeof(options), 1500));
        options[20] = 0; /* End of Option List requires zero padding */
        options[21] = 1;
        assert(!BalancifyIsValidIpPacket(options, sizeof(options), 1500));
        options[20] = 0x9e;
        options[21] = 3;
        options[23] = 0x9e; /* Final TLV is missing its length byte */
        assert(!BalancifyIsValidIpPacket(options, sizeof(options), 1500));
    }
}

static void test_ipv6_exact_payload_length(void) {
    unsigned char empty[40] = { 0x60, 0, 0, 0, 0, 0 };
    unsigned char payload[45] = { 0x60, 0, 0, 0, 0, 5 };
    empty[6] = 59; /* No Next Header */
    payload[6] = 59;
    assert(BalancifyIsValidIpPacket(empty, sizeof(empty), 1500));
    assert(BalancifyIsValidIpPacket(payload, sizeof(payload), 1500));
    payload[5] = 4;
    assert(!BalancifyIsValidIpPacket(payload, sizeof(payload), 1500));
}

static void test_ipv6_extension_and_transport_bounds(void) {
    unsigned char packet[56] = { 0x60, 0, 0, 0, 0, 16 };
    {
        unsigned char udp[48] = { 0x60, 0, 0, 0, 0, 8 };
        udp[6] = 17;
        udp[44] = 0;
        udp[45] = 8;
        assert(BalancifyIsValidIpPacket(udp, sizeof(udp), 1500));
        udp[45] = 7;
        assert(!BalancifyIsValidIpPacket(udp, sizeof(udp), 1500));
    }
    packet[6] = 0; /* Hop-by-Hop followed by UDP */
    packet[40] = 17;
    packet[41] = 0; /* 8-byte extension header */
    packet[52] = 0;
    packet[53] = 8; /* UDP header length */
    assert(BalancifyIsValidIpPacket(packet, sizeof(packet), 1500));
    packet[41] = 1; /* Declared extension exceeds the IPv6 payload */
    assert(!BalancifyIsValidIpPacket(packet, sizeof(packet), 1500));

    {
        unsigned char truncated_tcp[48] = { 0x60, 0, 0, 0, 0, 8 };
        truncated_tcp[6] = 6;
        assert(!BalancifyIsValidIpPacket(truncated_tcp, sizeof(truncated_tcp), 1500));
    }

    {
        unsigned char duplicate_fragments[56] = { 0x60, 0, 0, 0, 0, 16 };
        duplicate_fragments[6] = 44;
        duplicate_fragments[40] = 44;
        duplicate_fragments[48] = 59;
        assert(!BalancifyIsValidIpPacket(duplicate_fragments, sizeof(duplicate_fragments), 1500));
    }

    {
        unsigned char fragment[56] = { 0x60, 0, 0, 0, 0, 16 };
        fragment[6] = 44;
        fragment[40] = 6;
        fragment[43] = 1; /* More Fragments, with an aligned 8-byte payload */
        assert(BalancifyIsValidIpPacket(fragment, sizeof(fragment), 1500));
        fragment[41] = 1; /* Reserved byte must be zero */
        assert(!BalancifyIsValidIpPacket(fragment, sizeof(fragment), 1500));
        fragment[41] = 0;
        fragment[43] = 3; /* Reserved fragment-field bit must be zero */
        assert(!BalancifyIsValidIpPacket(fragment, sizeof(fragment), 1500));
    }

    {
        unsigned char empty_fragment[48] = { 0x60, 0, 0, 0, 0, 8 };
        unsigned char unaligned_fragment[55] = { 0x60, 0, 0, 0, 0, 15 };
        empty_fragment[6] = 44;
        unaligned_fragment[6] = 44;
        empty_fragment[40] = 17;
        unaligned_fragment[40] = 17;
        empty_fragment[43] = 1; /* More Fragments without payload */
        unaligned_fragment[43] = 1; /* More Fragments with a 7-byte payload */
        assert(!BalancifyIsValidIpPacket(empty_fragment, sizeof(empty_fragment), 1500));
        assert(!BalancifyIsValidIpPacket(unaligned_fragment, sizeof(unaligned_fragment), 1500));
    }
}

static void test_rejects_truncated_unknown_and_oversize_packets(void) {
    unsigned char packet[40] = { 0x60, 0, 0, 0, 0, 0 };
    packet[6] = 59;
    assert(!BalancifyIsValidIpPacket(NULL, sizeof(packet), 1500));
    assert(!BalancifyIsValidIpPacket(packet, 19, 1500));
    assert(!BalancifyIsValidIpPacket(packet, sizeof(packet), 39));
    packet[0] = 0x70;
    assert(!BalancifyIsValidIpPacket(packet, sizeof(packet), 1500));
}

static uint32_t next_random(uint32_t *state) {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    return *state;
}

static unsigned char random_protocol(uint32_t value) {
    static const unsigned char protocols[] = { 0, 6, 17, 43, 44, 51, 60, 59 };
    return protocols[value % (uint32_t)(sizeof(protocols) / sizeof(protocols[0]))];
}

/* Deterministic malformed/near-valid length mutations for sanitizer CI. */
static void test_randomized_length_fields_remain_bounded(void) {
    unsigned char packet[2048];
    uint32_t state = UINT32_C(0xA17E5EED);
    unsigned int iteration;

    for (iteration = 0u; iteration < 12000u; iteration++) {
        size_t length = (size_t)(next_random(&state) % (uint32_t)sizeof(packet));
        size_t index;
        for (index = 0u; index < length; index++) {
            packet[index] = (unsigned char)(next_random(&state) & UINT32_C(0xFF));
        }

        if (length >= 20u && (iteration & 1u) == 0u) {
            packet[0] = (unsigned char)(0x40u + 5u + (next_random(&state) % 11u));
            packet[2] = (unsigned char)((length >> 8u) & 0xFFu);
            packet[3] = (unsigned char)(length & 0xFFu);
            packet[9] = random_protocol(next_random(&state));
        } else if (length >= 40u) {
            size_t payloadLength = length - 40u;
            packet[0] = 0x60u;
            packet[4] = (unsigned char)((payloadLength >> 8u) & 0xFFu);
            packet[5] = (unsigned char)(payloadLength & 0xFFu);
            packet[6] = random_protocol(next_random(&state));
        }

        (void)BalancifyIsValidIpPacket(packet, length,
            (iteration & 1u) == 0u ? 1500u : 9000u);
    }
}

int main(void) {
    test_ipv4_minimum_packet();
    test_ipv4_options_and_payload();
    test_ipv4_rejects_bad_lengths_and_reserved_flag();
    test_ipv4_tcp_udp_and_fragment_bounds();
    test_ipv6_exact_payload_length();
    test_ipv6_extension_and_transport_bounds();
    test_rejects_truncated_unknown_and_oversize_packets();
    test_randomized_length_fields_remain_bounded();
    return 0;
}
