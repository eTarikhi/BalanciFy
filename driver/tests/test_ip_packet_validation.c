#include <assert.h>
#include <string.h>
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
        unsigned char fragment[48] = { 0x60, 0, 0, 0, 0, 8 };
        fragment[6] = 44;
        fragment[40] = 6;
        fragment[43] = 1; /* More Fragments: transport header may be split */
        assert(BalancifyIsValidIpPacket(fragment, sizeof(fragment), 1500));
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

int main(void) {
    test_ipv4_minimum_packet();
    test_ipv4_options_and_payload();
    test_ipv4_rejects_bad_lengths_and_reserved_flag();
    test_ipv4_tcp_udp_and_fragment_bounds();
    test_ipv6_exact_payload_length();
    test_ipv6_extension_and_transport_bounds();
    test_rejects_truncated_unknown_and_oversize_packets();
    return 0;
}
