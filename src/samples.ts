/**
 * Demo fixtures. These are the exact payloads from `test_layer2.py`, with the
 * CRC appended by the same CRC-16/CCITT-FALSE routine. They let every rule be
 * exercised from the UI before the camera pipeline exists.
 */

function crc16(input: string): string {
  let crc = 0xffff;
  for (const ch of input) {
    crc ^= ch.charCodeAt(0) << 8;
    for (let i = 0; i < 8; i++) {
      crc = crc & 0x8000 ? ((crc << 1) ^ 0x1021) & 0xffff : (crc << 1) & 0xffff;
    }
  }
  return crc.toString(16).toUpperCase().padStart(4, "0");
}

const withCrc = (body: string): string => {
  const payload = `${body}6304`;
  return `${payload}${crc16(payload)}`;
};

const T00 = "000201";
const T01_STATIC = "010211";
const T01_DYNAMIC = "010212";
const T26 = "26330010A0000006020115ID1020000000001";
const T52_GROCERY = "52045411";
const T52_CHARITY = "52048661";
const T53_IDR = "5303360";
const T58_ID = "5802ID";
const T59_WARUNG = "5913WARUNG MAKMUR";
const T59_TOKO_CHARITY = "5919TOKO CHARITY BERKAH";
const T60_JAKARTA = "6007JAKARTA";

const cleanBody =
  T00 + T01_STATIC + T26 + T52_GROCERY + T53_IDR + T58_ID + T59_WARUNG + T60_JAKARTA;

export interface SamplePayload {
  label: string;
  description: string;
  payload: string;
}

export const SAMPLE_PAYLOADS: SamplePayload[] = [
  {
    label: "Valid (statis)",
    description: "QRIS statis bersih — CRC valid, tanpa anomali.",
    payload: withCrc(cleanBody),
  },
  {
    label: "Dimanipulasi",
    description: "Nama merchant diubah tanpa memperbarui CRC — veto keras.",
    payload: withCrc(cleanBody).replace("WARUNG MAKMUR", "WARUNG HACKED"),
  },
  {
    label: "MCC palsu",
    description: "MCC amal (8661) dipasangkan dengan nama komersial.",
    payload: withCrc(
      T00 + T01_STATIC + T26 + T52_CHARITY + T53_IDR + T58_ID + T59_TOKO_CHARITY + T60_JAKARTA,
    ),
  },
  {
    label: "QR dinamis",
    description: "Tag 01=12 dipindai lewat kamera fisik — mismatch konteks.",
    payload: withCrc(
      T00 + T01_DYNAMIC + T26 + T52_GROCERY + T53_IDR + T58_ID + T59_WARUNG + T60_JAKARTA,
    ),
  },
];
