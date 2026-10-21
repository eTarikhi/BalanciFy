import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]
MINIPORT_C = (ROOT / "driver/src/miniport.c").read_text(encoding="utf-8")
INF = (ROOT / "driver/balancify.inf").read_text(encoding="utf-8")


class NdisOidContractTests(unittest.TestCase):
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
        for oid in ("OID_GEN_CURRENT_LOOKAHEAD", "OID_GEN_XMIT_OK", "OID_GEN_RCV_OK"):
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

    def test_raw_ip_inf_and_ndis_attributes_agree(self):
        self.assertRegex(INF, r"(?m)^\*MediaType\s*=\s*19\s*;\s*NdisMediumIP")
        self.assertRegex(INF, r'(?m)^HKR, Ndi\\Interfaces, LowerRange, 0, "nolower"$')
        self.assertIn("general.MediaType = NdisMediumIP;", MINIPORT_C)
        self.assertIn("general.PhysicalMediumType = NdisPhysicalMediumUnspecified;", MINIPORT_C)
        self.assertNotRegex(MINIPORT_C, r"OID_802_3_(?:CURRENT|PERMANENT)_ADDRESS")


if __name__ == "__main__":
    unittest.main()
