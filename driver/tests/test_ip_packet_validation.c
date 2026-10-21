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

static void test_ipv4_rejects_bad_lengths(void) {
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
}

static void test_ipv6_exact_payload_length(void) {
    unsigned char empty[40] = { 0x60, 0, 0, 0, 0, 0 };
    unsigned char payload[45] = { 0x60, 0, 0, 0, 0, 5 };
    assert(BalancifyIsValidIpPacket(empty, sizeof(empty), 1500));
    assert(BalancifyIsValidIpPacket(payload, sizeof(payload), 1500));
    payload[5] = 4;
    assert(!BalancifyIsValidIpPacket(payload, sizeof(payload), 1500));
}

static void test_rejects_truncated_unknown_and_oversize_packets(void) {
    unsigned char packet[40] = { 0x60, 0, 0, 0, 0, 0 };
    assert(!BalancifyIsValidIpPacket(NULL, sizeof(packet), 1500));
    assert(!BalancifyIsValidIpPacket(packet, 19, 1500));
    assert(!BalancifyIsValidIpPacket(packet, sizeof(packet), 39));
    packet[0] = 0x70;
    assert(!BalancifyIsValidIpPacket(packet, sizeof(packet), 1500));
}

int main(void) {
    test_ipv4_minimum_packet();
    test_ipv4_options_and_payload();
    test_ipv4_rejects_bad_lengths();
    test_ipv6_exact_payload_length();
    test_rejects_truncated_unknown_and_oversize_packets();
    return 0;
}
