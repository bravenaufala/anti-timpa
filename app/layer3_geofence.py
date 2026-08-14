import json
from typing import Dict, Any, Optional

def process_layer3_geofence(
    client_city: Optional[str], 
    merchant_city: Optional[str]
) -> Dict[str, Any]:
    """
    Process Layer 3: City Geofencing.
    
    Args:
        client_city: The city name obtained from GPS reverse geocoding.
        merchant_city: The city name extracted from the QR code (Tag 60).
        
    Returns:
        Dict containing l3_score, risk_level, and warnings.
    """
    if not merchant_city:
        return {
            "l3_score": 0.0,
            "risk_level": "LOW RISK",
            "warnings": ["Merchant City missing from QR code - skipped geofence check"]
        }
        
    if not client_city:
        return {
            "l3_score": 0.0,
            "risk_level": "LOW RISK",
            "warnings": ["Client Location unavailable - skipped geofence check"]
        }

    if client_city == "LOADING":
        return {
            "l3_score": 0.5,
            "risk_level": "CAUTION",
            "warnings": ["Client Location is loading (API call in progress)..."]
        }

    c_city = client_city.upper().strip()
    m_city = merchant_city.upper().strip()

    # Simplify common prefixes/suffixes for robust matching
    removals = ["KOTA ", "KABUPATEN ", "KAB. "]
    for r in removals:
        c_city = c_city.replace(r, "")
        m_city = m_city.replace(r, "")
        
    c_city = c_city.strip()
    m_city = m_city.strip()

    l3_score = 0.0
    risk_level = "LOW RISK"
    warnings = []

    # Check for exact match or substring match (e.g. "JAKARTA" in "JAKARTA SELATAN")
    if c_city == m_city or c_city in m_city or m_city in c_city:
        l3_score = 0.0
        risk_level = "LOW RISK"
    else:
        l3_score = 1.0
        risk_level = "HIGH RISK"
        warnings.append(f"Geofence Anomaly: Client is in '{client_city}', but QR is for '{merchant_city}'")

    return {
        "l3_score": round(l3_score, 4),
        "risk_level": risk_level,
        "warnings": warnings,
        "client_city": client_city,
        "merchant_city": merchant_city
    }
