"""
Verify WHY a QRIS payload fails the CRC-16 check.

Usage:
    python3 check_crc.py '<paste Qris payload>'

It recomputes the expected QRIS CRC-16/CCITT-FALSE and compares against what
is encoded in Tag 63, then prints a diagnosis:

  * VALID          -> payload is genuine, app shows LOW/CAUTION as expected.
  * BAD CRC        -> the QR's encoded checksum is wrong (bad generator or
                      tampered/modified payload). App correctly flags HIGH RISK.
  * TAG63 MISSING  -> no 6304 checksum tag at all -> HIGH RISK by design.
  * CUT / `Not at end` -> detector likely truncated the trailing bytes; the
                      camera decode is the culprit, not the QR.
"""

import sys


def crc_ccitt_false(data: bytes) -> int:
    crc = 0xFFFF
    for b in data:
        crc ^= (b << 8)
        for _ in range(8):
            if crc & 0x8000:
                crc = ((crc << 1) ^ 0x1021) & 0xFFFF
            else:
                crc = (crc << 1) & 0xFFFF
    return crc


def diagnose(raw: str) -> None:
    print("Payload  :", raw)
    print("Length   :", len(raw))
    crc_pos = raw.rfind("6304")
    if crc_pos == -1:
        print("-> TAG63 MISSING: tidak ada tag 6304 sama sekali.")
        print("   QRIS wajib diakhiri Tag 63 (CRC). HIGH RISK di-expected.")
        return
    if len(raw) < crc_pos + 8:
        print("-> CUT: string pendek / terpotong, CRC tidak lengkap.")
        print("   Kemungkinan decoder kamera memotong byte terakhir.")
        return
    after = raw[crc_pos + 4:]
    if not (len(after) == 4 and all(c in "0123456789ABCDEFabcdef" for c in after)):
        print("-> GANJIL: sesudah 6304 bukan 4 karakter hex. Ada data mencurigakan.")
        return
    encoded = raw[crc_pos + 4:crc_pos + 8].upper()
    payload = raw[:crc_pos + 4]
    expect = f"{crc_ccitt_false(payload.encode('ascii')):04X}"
    truncated = (len(raw) == crc_pos + 8)  # adalah char terakhir dari string
    print("Encoded  :", encoded)
    print("Expected :", expect, "(CRC atas payload s/d 6304)")
    print("CRC Tag  :", "di akhir string" if truncated else f"bukan di akhir (ada {len(raw)-crc_pos-8} char lagi sesudahnya)")
    if encoded == expect and truncated:
        print("=> VALID: QRIS asli & checksum cocok. App normal.")
    elif encoded != expect:
        print("=> BAD CRC: checksum di QR salah, tapi body ter-parse OK.")
        print("   Penyebab: (a) generator QRIS murahan tidak menghitung CRC,")
        print("   atau (b) payload sudah dimodifikasi/dipotong. App benar menandai HIGH RISK.")
    else:
        print("=> CRC benar tapi ada data tambahan sesudah tag 63 (tidak sesuai standar).")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    diagnose(sys.argv[1])
