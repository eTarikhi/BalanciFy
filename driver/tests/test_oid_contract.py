import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
MINIPORT_C = (ROOT / "driver/src/miniport.c").read_text(encoding="utf-8")
IOCTL_C = (ROOT / "driver/src/ioctl.c").read_text(encoding="utf-8")
INF = (ROOT / "driver/balancify.inf").read_text(encoding="utf-8")
MINIPORT_H = (ROOT / "driver/src/miniport.h").read_text(encoding="utf-8")


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

    def test_unimplemented_transmit_path_fails_instead_of_counting_success(self):
        send_callback = re.search(
            r"VOID MiniportSendNetBufferLists\(.*?\n\}", MINIPORT_C, re.S
        )
        self.assertIsNotNone(send_callback)
        body = send_callback.group(0)
        self.assertIn(
            "NET_BUFFER_LIST_STATUS(curr) = paused ? NDIS_STATUS_PAUSED : NDIS_STATUS_FAILURE;",
            body,
        )
        self.assertIn("adapter->txErrors++", body)
        send_track = body.index("BalancifyTrackSendCallback(&adapter->pauseState)")
        send_loop = body.index("while (curr)")
        send_return = body.index("BalancifySendCallbackReturned(&adapter->pauseState)")
        self.assertLess(send_track, send_loop)
        self.assertLess(send_loop, send_return)
        self.assertNotIn("adapter->txPackets++", body)
        self.assertNotIn("adapter->txBytes +=", body)
        self.assertIn("case OID_GEN_XMIT_ERROR:", MINIPORT_C)

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
