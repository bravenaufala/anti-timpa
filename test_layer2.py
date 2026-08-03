"""
Anti Timpa QRIS Fintech SDK - Layer 2 Test Suite
Module: test_layer2.py

Tests for EMVCo TLV parsing, CRC-16 verifier, and process_layer2_tlv risk scoring engine.
"""

import sys
import unittest
from layer2_emvco import parse_emvco_tlv, verify_crc16, process_layer2_tlv


def generate_qris_with_crc(payload_without_crc: str) -> str:
    """
    Helper function to calculate and append Tag 63 CRC-16/CCITT-FALSE to mock QRIS payloads.
    """
    payload_to_checksum = payload_without_crc + "6304"
    crc = 0xFFFF
    for byte in payload_to_checksum.encode('ascii'):
        crc ^= (byte << 8)
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
    crc_hex = f"{crc:04X}"
    return payload_to_checksum + crc_hex


class TestLayer2EMVCo(unittest.TestCase):

    def setUp(self):
        # Base valid components
        self.tag00 = "000201"
        self.tag01_static = "010211"
        self.tag01_dynamic = "010212"
        self.tag26 = "26330010A0000006020115ID1020000000001"
        self.tag52_grocery = "52045411"
        self.tag52_charity = "52048661"
        self.tag53_idr = "5303360"
        self.tag58_id = "5802ID"
        self.tag59_warung = "5913WARUNG MAKMUR"
        self.tag59_toko_charity = "5919TOKO CHARITY BERKAH"
        self.tag60_jakarta = "6007JAKARTA"

    def test_1_valid_static_qris(self):
        """
        Test 1: Valid sample static QRIS string (expects crc_valid=True and l2_score=0.0).
        """
        payload_raw = (
            self.tag00 +
            self.tag01_static +
            self.tag26 +
            self.tag52_grocery +
            self.tag53_idr +
            self.tag58_id +
            self.tag59_warung +
            self.tag60_jakarta
        )
        valid_qris = generate_qris_with_crc(payload_raw)

        # Verify CRC function directly
        self.assertTrue(verify_crc16(valid_qris))

        # Verify process_layer2_tlv
        result = process_layer2_tlv(valid_qris, scan_context={"optical_type": "physical_camera_scan"})

        self.assertTrue(result["crc_valid"])
        self.assertEqual(result["l2_score"], 0.0)
        self.assertEqual(result["initiation_mode"], "11")
        self.assertEqual(result["mcc"], "5411")
        self.assertEqual(result["merchant_name"], "WARUNG MAKMUR")
        self.assertEqual(result["merchant_city"], "JAKARTA")
        self.assertEqual(len(result["warnings"]), 0)
        self.assertTrue(result["parsed_tlv"].get("valid"))
        self.assertIn("26", result["parsed_tlv"])
        self.assertIsInstance(result["parsed_tlv"]["26"], dict)

    def test_2_tampered_payload(self):
        """
        Test 2: Tampered payload string with altered characters (expects crc_valid=False and l2_score=1.0).
        """
        payload_raw = (
            self.tag00 +
            self.tag01_static +
            self.tag26 +
            self.tag52_grocery +
            self.tag53_idr +
            self.tag58_id +
            self.tag59_warung +
            self.tag60_jakarta
        )
        valid_qris = generate_qris_with_crc(payload_raw)

        # Alter merchant name in payload without updating CRC63
        tampered_qris = valid_qris.replace("WARUNG MAKMUR", "WARUNG HACKED")

        self.assertFalse(verify_crc16(tampered_qris))

        result = process_layer2_tlv(tampered_qris)

        self.assertFalse(result["crc_valid"])
        self.assertEqual(result["l2_score"], 1.0)
        self.assertTrue(any("CRC-16" in w for w in result["warnings"]))

    def test_3_mcc_misrepresentation_anomaly(self):
        """
        Test 3: MCC vs Merchant Name misrepresentation anomaly (expects elevated risk score and warning logged).
        Charity MCC (8661) paired with commercial keyword "TOKO".
        """
        payload_raw = (
            self.tag00 +
            self.tag01_static +
            self.tag26 +
            self.tag52_charity +
            self.tag53_idr +
            self.tag58_id +
            self.tag59_toko_charity +
            self.tag60_jakarta
        )
        qris_str = generate_qris_with_crc(payload_raw)

        self.assertTrue(verify_crc16(qris_str))

        result = process_layer2_tlv(qris_str)

        self.assertTrue(result["crc_valid"])
        self.assertEqual(result["mcc"], "8661")
        self.assertGreaterEqual(result["l2_score"], 0.50)
        self.assertTrue(any("MCC misrepresentation" in w for w in result["warnings"]))

    def test_4_dynamic_qr_camera_context_mismatch(self):
        """
        Test 4: Dynamic QR (Tag 01 = 12) scanned in physical camera context (expects warning and context mismatch risk).
        """
        payload_raw = (
            self.tag00 +
            self.tag01_dynamic +
            self.tag26 +
            self.tag52_grocery +
            self.tag53_idr +
            self.tag58_id +
            self.tag59_warung +
            self.tag60_jakarta
        )
        qris_str = generate_qris_with_crc(payload_raw)

        scan_context = {"optical_type": "physical_camera_scan"}
        result = process_layer2_tlv(qris_str, scan_context=scan_context)

        self.assertTrue(result["crc_valid"])
        self.assertEqual(result["initiation_mode"], "12")
        self.assertEqual(result["l2_score"], 0.40)
        self.assertTrue(any("Dynamic QR code" in w for w in result["warnings"]))

    def test_5_malformed_tlv_length(self):
        """
        Additional Test: Malformed TLV structure with invalid length bounds.
        """
        malformed_str = "0002010102115999TOO_SHORT63041234"
        result = process_layer2_tlv(malformed_str)
        self.assertEqual(result["l2_score"], 1.0)
        self.assertFalse(result["parsed_tlv"].get("valid"))


if __name__ == "__main__":
    unittest.main()
