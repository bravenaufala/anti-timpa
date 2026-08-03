"""
Anti Timpa QRIS Fintech SDK - Layer 2: EMVCo Payload & Structural Analysis
Module: layer2_emvco.py

Provides robust Tag-Length-Value (TLV) parsing for EMVCo MPM QRIS strings,
CRC-16/CCITT-FALSE checksum verification, and rule-based risk evaluation.
"""

from typing import Dict, Any, Tuple, Optional, List


def parse_emvco_tlv(raw_qris_str: str) -> Dict[str, Any]:
    """
    Robustly parses flat and nested Tag-Length-Value structures according to the
    EMVCo Merchant Presented Mode (MPM) specification.

    Slices sequentially: Tag (2 chars), Length (2 chars converted to int), Value (Length chars).
    Recursively parses nested merchant account tags (Tags 26 to 51) if sub-tags exist.
    Handles invalid length, missing tags, or non-EMVCo strings gracefully returning
    a structured dictionary with `valid=False`.
    """
    if not isinstance(raw_qris_str, str) or not raw_qris_str:
        return {"valid": False, "error": "Empty or non-string QRIS payload"}

    def _parse_blocks(s: str) -> Tuple[bool, Dict[str, Any]]:
        tlv_dict: Dict[str, Any] = {}
        idx = 0
        n = len(s)
        if n == 0:
            return False, {}

        while idx < n:
            # Need at least 4 characters for Tag (2) + Length (2)
            if idx + 4 > n:
                return False, {}

            tag = s[idx:idx + 2]
            len_str = s[idx + 2:idx + 4]

            if not len_str.isdigit():
                return False, {}

            length = int(len_str)
            idx += 4

            if idx + length > n:
                return False, {}

            value = s[idx:idx + length]
            idx += length

            # Check if tag is a merchant tag (26 to 51 inclusive) that may contain sub-TLVs
            if tag.isdigit() and 26 <= int(tag) <= 51:
                sub_valid, sub_dict = _parse_blocks(value)
                if sub_valid and sub_dict:
                    tlv_dict[tag] = sub_dict
                else:
                    tlv_dict[tag] = value
            else:
                tlv_dict[tag] = value

        if idx != n:
            return False, {}

        return True, tlv_dict

    success, parsed_dict = _parse_blocks(raw_qris_str)
    if not success:
        return {"valid": False, "error": "Structural TLV parsing failure"}

    parsed_dict["valid"] = True
    return parsed_dict


def verify_crc16(raw_qris_str: str) -> bool:
    """
    Computes CRC-16/CCITT-FALSE checksum on the QRIS payload up to Tag 6304 ("...6304")
    and compares it against the encoded 4-character hex checksum at the end of Tag 63.

    Polynomial: 0x1021
    Initial value: 0xFFFF
    Reflect In/Out: False
    XOR Out: 0x0000
    """
    if not isinstance(raw_qris_str, str):
        return False

    crc_pos = raw_qris_str.rfind("6304")
    if crc_pos == -1 or len(raw_qris_str) < crc_pos + 8:
        return False

    payload_to_check = raw_qris_str[:crc_pos + 4]
    expected_crc = raw_qris_str[crc_pos + 4:crc_pos + 8]

    if len(expected_crc) != 4:
        return False

    crc = 0xFFFF
    for byte in payload_to_check.encode('ascii', errors='ignore'):
        crc ^= (byte << 8)
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF

    calculated_crc_hex = f"{crc:04X}"
    return calculated_crc_hex.upper() == expected_crc.upper()


def process_layer2_tlv(raw_qris_str: str, scan_context: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
    """
    Evaluates payload integrity, structural validity, and risk rules,
    returning an `l2_score` bounded in [0.0, 1.0].

    Returns dictionary:
    {
        "l2_score": float,
        "crc_valid": bool,
        "initiation_mode": str,
        "mcc": str,
        "merchant_name": str,
        "merchant_city": str,
        "parsed_tlv": dict,
        "warnings": list
    }
    """
    scan_context = scan_context or {}
    warnings: List[str] = []
    accumulated_risk = 0.0

    parsed_tlv = parse_emvco_tlv(raw_qris_str)
    crc_valid = verify_crc16(raw_qris_str)

    # 1. Structural TLV check
    if not parsed_tlv.get("valid", False):
        warnings.append("Invalid TLV payload structure")

    # 2. Checksum Failure (Tag 63 missing or verify_crc16() False)
    has_tag63 = isinstance(parsed_tlv, dict) and "63" in parsed_tlv
    if not crc_valid or not has_tag63:
        warnings.append("CRC-16 checksum verification failed or Tag 63 missing")
        accumulated_risk = 1.0

    # Extract standard fields
    initiation_mode = str(parsed_tlv.get("01", "")) if isinstance(parsed_tlv, dict) else ""
    mcc = str(parsed_tlv.get("52", "")) if isinstance(parsed_tlv, dict) else ""
    merchant_name = str(parsed_tlv.get("59", "")) if isinstance(parsed_tlv, dict) else ""
    merchant_city = str(parsed_tlv.get("60", "")) if isinstance(parsed_tlv, dict) else ""
    payload_format = str(parsed_tlv.get("00", "")) if isinstance(parsed_tlv, dict) else ""
    currency = str(parsed_tlv.get("53", "")) if isinstance(parsed_tlv, dict) else ""
    country = str(parsed_tlv.get("58", "")) if isinstance(parsed_tlv, dict) else ""

    # If CRC is valid and TLV is valid, calculate fine-grained risk rules
    if crc_valid and parsed_tlv.get("valid", False):
        # Rule A: Format Anomaly (Tag 00 != "01")
        if payload_format != "01":
            accumulated_risk += 0.50
            warnings.append("Payload Format Indicator (Tag 00) is invalid or not '01'")

        # Rule B: Currency / Country Anomaly (Tag 53 != "360" or Tag 58 != "ID")
        if currency != "360" or country != "ID":
            accumulated_risk += 0.30
            warnings.append("Transaction currency (Tag 53) or country code (Tag 58) anomaly")

        # Rule C: Initiation Mode & Context Mismatch (Tag 01 == "12" scanned in physical camera context)
        optical_type = scan_context.get("optical_type")
        if initiation_mode == "12" and optical_type == "physical_camera_scan":
            accumulated_risk += 0.40
            warnings.append("Dynamic QR code (Tag 01=12) scanned in physical camera scan context")

        # Rule D: MCC & Merchant Category Misrepresentation
        charity_mccs = {"8661", "8398"}
        commercial_keywords = ["toko", "store", "cell", "mart", "warung", "cafe", "kopi"]
        mname_lower = merchant_name.lower()

        if mcc in charity_mccs and any(kw in mname_lower for kw in commercial_keywords):
            accumulated_risk += 0.50
            warnings.append(f"MCC misrepresentation anomaly: Charity MCC ({mcc}) paired with commercial merchant name ('{merchant_name}')")

    # Bound score in [0.0, 1.0]
    l2_score = min(1.0, max(0.0, float(accumulated_risk)))

    return {
        "l2_score": l2_score,
        "crc_valid": crc_valid,
        "initiation_mode": initiation_mode,
        "mcc": mcc,
        "merchant_name": merchant_name,
        "merchant_city": merchant_city,
        "parsed_tlv": parsed_tlv,
        "warnings": warnings
    }
