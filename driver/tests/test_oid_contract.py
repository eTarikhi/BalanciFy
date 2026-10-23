import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
MINIPORT_C = (ROOT / "driver/src/miniport.c").read_text(encoding="utf-8")
IOCTL_C = (ROOT / "driver/src/ioctl.c").read_text(encoding="utf-8")
INF = (ROOT / "driver/balancify.inf").read_text(encoding="utf-8")
MINIPORT_H = (ROOT / "driver/src/miniport.h").read_text(encoding="utf-8")
TX_QUEUE_H = (ROOT / "driver/src/tx_packet_queue.h").read_text(encoding="utf-8")
DRIVER_DEVICE_RS = (ROOT / "crates/service/src/driver_device.rs").read_text(encoding="utf-8")


class NdisOidContractTests(unittest.TestCase):
    def test_ndis_version_contract_is_defined_before_wdk_headers(self):
        for macro in ("NDIS_MINIPORT_DRIVER", "NDIS680_MINIPORT", "NDIS_SUPPORT_NDIS680"):
            define = MINIPORT_H.index(f"#define {macro} 1")
            self.assertLess(define, MINIPORT_H.index("#include <ndis.h>"))

    def test_every_advertised_oid_has_a_query_handler(self):
        match = re.search(
            r"static const NDIS_OID gSupportedOids\[\]\s*=\s*\{(.*?)\};",
            MINIPORT_C,
            re.S,
        )
        self.assertIsNotNone(match)
        advertised = re.findall(r"\bOID_[A-Z0-9_]+\b", match.group(1))
        query_cases = set(
            re.findall(r"\bcase\s+(OID_[A-Z0-9_]+)\s*:", MINIPORT_C)
        )
        self.assertTrue(advertised)
        self.assertEqual(len(advertised), len(set(advertised)))
        self.assertTrue(set(advertised).issubset(query_cases))

    def test_lookahead_and_packet_count_oids_are_advertised_and_handled(self):
        for oid in (
            "OID_GEN_CURRENT_LOOKAHEAD",
            "OID_GEN_XMIT_OK",
            "OID_GEN_XMIT_ERROR",
            "OID_GEN_RCV_OK",
        ):
            self.assertRegex(MINIPORT_C, rf"(?s)gSupportedOids\[\].*?\b{oid}\b")
            self.assertIn(f"case {oid}:", MINIPORT_C)
        self.assertIn("if (oid == OID_GEN_CURRENT_LOOKAHEAD)", MINIPORT_C)

    def test_inf_mtu_setting_is_read_and_normalized(self):
        self.assertRegex(MINIPORT_C, r"NdisReadConfiguration\(\s*&status")
        self.assertIn(
            "BalancifyNormalizeMtu(parameter->ParameterData.IntegerData, mtu)",
            MINIPORT_C,
        )
        self.assertIn("NdisCloseConfiguration(configurationHandle);", MINIPORT_C)
        registration_set = MINIPORT_C.index("(PNDIS_MINIPORT_ADAPTER_ATTRIBUTES)&registration")
        mtu_read = MINIPORT_C.index("adapter->mtu = MiniportReadConfiguredMtu")
        self.assertLess(registration_set, mtu_read)

    def test_set_mtu_cannot_diverge_from_ndis_cached_interface_attributes(self):
        mtu_case = re.search(
            r"case IOCTL_BALANCIFY_SET_MTU:\s*\{(.*?)\n\s*\}\n\s*case IOCTL_BALANCIFY_GET_STATS:",
            IOCTL_C,
            re.S,
        )
        self.assertIsNotNone(mtu_case)
        body = mtu_case.group(1)
        self.assertIn("adapter->mtu == mtu", body)
        self.assertIn("STATUS_NOT_SUPPORTED", body)
        self.assertNotIn("adapter->mtu = mtu", body)
        self.assertNotIn("adapter->lookaheadSize =", body)

    def test_unimplemented_quanta_scheduler_does_not_report_success(self):
        quanta_case = re.search(
            r"case IOCTL_BALANCIFY_SET_QUANTA:\s*\{(.*?)\n\s*\}\n\s*default:",
            IOCTL_C,
            re.S,
        )
        self.assertIsNotNone(quanta_case)
        body = quanta_case.group(1)
        self.assertIn("STATUS_INVALID_PARAMETER", body)
        self.assertIn("STATUS_NOT_SUPPORTED", body)
        self.assertNotIn("STATUS_SUCCESS", body)
        self.assertNotIn("SET_QUANTA count=", body)

    def test_transmit_path_atomically_queues_valid_ip_batches(self):
        send_callback = re.search(
            r"VOID MiniportSendNetBufferLists\(.*?\n\}", MINIPORT_C, re.S
        )
        self.assertIsNotNone(send_callback)
        body = send_callback.group(0)
        self.assertIn("MiniportQueueNetBufferList(adapter, curr)", body)
        self.assertIn("BalancifyTrackSendCallback(&adapter->pauseState)", body)
        self.assertIn("BalancifySendCallbackReturned(&adapter->pauseState)", body)
        self.assertLess(
            body.index("BalancifyTrackSendCallback(&adapter->pauseState)"),
            body.index("while (curr)"),
        )
        queue_send = re.search(
            r"static NDIS_STATUS MiniportQueueNetBufferList\(.*?\n\}", MINIPORT_C, re.S
        )
        self.assertIsNotNone(queue_send)
        body = queue_send.group(0)
        self.assertIn("NDIS_STATUS_PAUSED", body)
        self.assertIn("NDIS_STATUS_RESOURCES", body)
        self.assertIn("BalancifyIsValidIpPacket", body)
        self.assertIn("BalancifyTxQueueReserveSlot", body)
        self.assertIn("BalancifyTxQueueCommit", body)
        self.assertIn("adapter->txPackets +=", body)
        self.assertIn("adapter->txBytes +=", body)
        self.assertIn("adapter->txErrors +=", body)
        self.assertIn("case OID_GEN_XMIT_ERROR:", MINIPORT_C)

    def test_driver_tx_queue_is_bounded_fifo_and_commits_batches_atomically(self):
        self.assertIn("#define BALANCIFY_TX_QUEUE_CAPACITY 64u", TX_QUEUE_H)
        self.assertIn("#define BALANCIFY_TX_PACKET_MAX_BYTES 9000u", TX_QUEUE_H)
        self.assertIn("queue->count += packetCount;", TX_QUEUE_H)
        self.assertIn("BalancifyTxQueueCommit(&adapter->txQueue, packetCount)", MINIPORT_C)
        self.assertIn("BalancifyIsValidIpPacket", MINIPORT_C)

    def test_dequeue_tx_ioctl_and_service_wire_contract_match(self):
        self.assertIn(
            "#define IOCTL_BALANCIFY_DEQUEUE_TX CTL_CODE(FILE_DEVICE_NETWORK, 0x804, METHOD_BUFFERED, FILE_READ_ACCESS)",
            IOCTL_C,
        )
        dequeue_case = re.search(
            r"case IOCTL_BALANCIFY_DEQUEUE_TX:\s*\{(.*?)\n\s*\}\n\s*case IOCTL_BALANCIFY_SET_QUANTA:",
            IOCTL_C,
            re.S,
        )
        self.assertIsNotNone(dequeue_case)
        body = dequeue_case.group(1)
        self.assertIn("BalancifyTxQueuePeek", body)
        self.assertIn("BalancifyTxQueuePop", body)
        self.assertIn("size_t bytesReturned = 0;", IOCTL_C)
        self.assertLess(body.index("NdisMoveMemory(destination, &header"), body.index("BalancifyTxQueuePop"))
        self.assertIn("IOCTL_FUNCTION_DEQUEUE_TX: u32 = 0x804", DRIVER_DEVICE_RS)
        self.assertIn("IOCTL_BALANCIFY_DEQUEUE_TX", DRIVER_DEVICE_RS)
        self.assertIn("decode_dequeued_packet(&output[..bytes_returned])", DRIVER_DEVICE_RS)
        self.assertIn("validate_injection_packet(&packet)", DRIVER_DEVICE_RS)

    def test_pause_waits_for_receive_indication_and_send_completion(self):
        self.assertIn("BalancifyBeginPause(&adapter->pauseState)", MINIPORT_C)
        self.assertIn("BalancifyReceiveNblReturned(&adapter->pauseState)", MINIPORT_C)
        self.assertIn("BalancifyReceiveIndicationReturned(&adapter->pauseState)", IOCTL_C)
        self.assertIn("BalancifySendCallbackReturned(&adapter->pauseState)", MINIPORT_C)
        self.assertRegex(MINIPORT_C, r"if\s*\(completePause\)\s*NdisMPauseComplete\(")
        track = IOCTL_C.index("BalancifyTrackReceiveIndication(&adapter->pauseState)")
        indicate = IOCTL_C.index("NdisMIndicateReceiveNetBufferLists(")
        returned = IOCTL_C.index("BalancifyReceiveIndicationReturned(&adapter->pauseState)")
        self.assertLess(track, indicate)
        self.assertLess(indicate, returned)

    def test_universal_inf_targets_both_wdk_architectures_and_ndis_68(self):
        self.assertRegex(INF, r"(?m)^DriverPackageType\s*=\s*Universal\s*$")
        self.assertRegex(INF, r"(?m)^%Provider%\s*=\s*Balancify,\s*NTamd64,\s*NTarm64\s*$")
        for architecture in ("NTamd64", "NTarm64"):
            self.assertIn(f"[Balancify.{architecture}]", INF)
            self.assertRegex(INF, rf"(?s)\[Balancify\.{architecture}\].*?Balancify\.ndi, ROOT\\Balancify")
        self.assertRegex(INF, r"(?m)^HKR, Parameters, MajorNdisVersion, .*?, 6\s*$")
        self.assertRegex(INF, r"(?m)^HKR, Parameters, MinorNdisVersion, .*?, 80(?:\s|;)")
        self.assertIn("ServiceBinary = %12%\\balancify.sys", INF)

    def test_raw_ip_inf_and_ndis_attributes_agree(self):
        self.assertRegex(INF, r"(?m)^\*MediaType\s*=\s*19\s*;\s*NdisMediumIP")
        self.assertRegex(INF, r'(?m)^HKR, Ndi\\Interfaces, LowerRange, 0, "nolower"$')
        self.assertIn("general.MediaType = NdisMediumIP;", MINIPORT_C)
        self.assertIn("general.PhysicalMediumType = NdisPhysicalMediumUnspecified;", MINIPORT_C)
        self.assertNotRegex(MINIPORT_C, r"OID_802_3_(?:CURRENT|PERMANENT)_ADDRESS")


if __name__ == "__main__":
    unittest.main()
