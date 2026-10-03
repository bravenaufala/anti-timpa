# Anti Timpa: PITCH DECK MATERIAL

HackNusa 2026 · Presentation Deck (10–15 slides) · Slot 12 menit (pitching + Q&A)

Dokumen ini berisi dua hal:
1. Prompt AI siap tempel ke generator deck (Gamma, Canva AI, ChatGPT/Slides, Tome, dll).
2. Materi lengkap per slide + speaker notes + alokasi waktu, untuk membuat deck manual atau memverifikasi hasil AI.

> Bahasa: materi ini ditulis dalam Bahasa Indonesia (sesuai audiens pitching lokal). Untuk versi Inggris, lihat catatan di akhir prompt.

---

## BAGIAN 1: PROMPT AI (copy-paste)

Salin blok di bawah ini utuh ke alat AI pembuat slide. Prompt sudah memuat semua fakta agar AI tidak mengarang angka.

```
Buatkan presentation deck profesional 13 slide (16:9) untuk final pitching hackathon,
berbahasa Indonesia, dengan detail teknis yang kuat namun ringkas dan sangat visual.

KONTEKS PROYEK (gunakan fakta ini, JANGAN mengarang angka lain):
- Nama: Anti Timpa, "On-Device QRIS Tamper Detection".
- Acara: HackNusa 2026. Track: Cybersecurity / Trust & Safety in Digital Payments.
- Masalah: QRIS adalah alat bayar utama UMKM Indonesia, tapi token-nya fisik dan
  mudah diganti. Serangan "overlay": penyerang menempel stiker QRIS miliknya di atas
  QRIS merchant; pelanggan memindai dan membayar ke penyerang.
- Kenapa kontrol lama gagal: semua memeriksa DATA (CRC-16/Tag 63, struktur TLV, nama
  merchant, konfirmasi bank), tidak ada yang memeriksa OBJEK FISIK. Overlay mengubah
  objek tanpa mengubah data, jadi tak terlihat oleh seluruh stack yang ada.
- Solusi: aplikasi lintas platform (desktop/Android/iOS dari satu codebase Rust +
  Tauri 2 + React 18) dengan 3 lapisan analisis lokal:
  L1 Optical (Sobel edge density + glare): satu-satunya yang memeriksa objek fisik;
  L2 EMVCo (parsing TLV + CRC-16/CCITT-FALSE + 4 aturan risiko);
  L3 Geofence (klasifikasi kelayakan kota, Haversine, tidak bisa memicu HIGH RISK).
  Output: verdict (LOW/CAUTION/HIGH RISK) + temuan bernama + laporan coverage.
- USP: (1) satu-satunya yang memeriksa objek fisik; (2) 100% on-device, ditegakkan
  lewat CSP default-src 'self': tidak ada server/backend untuk dibobol; (3) melaporkan
  coverage-nya sendiri sehingga scan sebagian TIDAK pernah tampak "aman"; (4) menolak
  membandingkan saat tidak bermakna (NOT_COMPARABLE / NOT RUN, bukan skor nol palsu);
  (5) offline secara arsitektur.
- Bukti PoC: pada fixture: clean QR skor 0.000 (LOW RISK); QR berstiker edge 0.2767,
  skor 0.371 (CAUTION); CRC dimanipulasi skor 1.000 (HIGH RISK, veto); wisatawan beda
  kota skor 0.650 (CAUTION). Clean vs stiker punya PAYLOAD IDENTIK: itulah serangannya.
  Ring width terpilih 0.12 dengan separasi ~8,6x. Test suite: 162 lulus. Kamera
  Android sudah diverifikasi di perangkat (Samsung A14, Android 15).
- Arsitektur: burst 4 frame -> decode QR (rqrr, pure Rust) -> L1/L2/L3 -> skor gabungan
  (veto CRC, selain itu max) -> findings + hash chain riwayat (FNV-1a) ->
  UI + banner coverage. Satu-satunya batas kepercayaan = IPC in-process
  antara WebView dan core Rust. Tidak ada egress jaringan.
- Keamanan: threat model A1 overlay attacker, A2 payload forger, A3 local attacker,
  A4 curious third party. CSP ketat, tanpa kunci rahasia, memory-safe (Rust), nilai
  dari QR hanya ditampilkan sebagai teks (di-escape React, bukan diinterpretasi),
  lokasi opt-in & coarse-only, desktop tanpa izin sama sekali.
- Skalabilitas: tanpa server = biaya marginal nol; skala ke sejuta pengguna tanpa
  capacity planning. Roadmap: Phase 1 kalibrasi lapangan (prioritas tertinggi, 4-6
  minggu), Phase 2 pelengkapan platform (iOS/Windows/macOS), Phase 3 adopsi (app
  standalone + SDK untuk PSP/bank), Phase 4 ekosistem (gazetteer lengkap, opt-in
  corroboration, engagement Bank Indonesia).
- Keterbatasan (sebutkan dengan jujur): threshold masih di-fit pada fixture sintetik, belum
  diuji pada foto overlay asli; tabel kota masih kecil; iOS/Windows/macOS belum
  diverifikasi. Ini justru rencana kerja, bukan risiko engineering.

GAYA VISUAL: bersih, modern, korporat-teknologi. Palet biru navy + aksen teal/hijau,
latar terang, tipografi sans-serif. Banyak diagram dan ikon, teks per slide maksimal
6 bullet pendek (<= 10 kata per bullet). Sertakan diagram alur arsitektur, diagram
3 lapisan, dan timeline roadmap. Beri placeholder gambar dengan label "[SISIPKAN
GAMBAR: ...]" untuk 5 visual berikut:
  1) infografis serangan overlay, 2) diagram 3 lapisan, 3) screenshot aplikasi,
  4) diagram arsitektur & alur data, 5) timeline roadmap.
Tambahkan speaker notes ringkas (2-4 kalimat) di tiap slide.

STRUKTUR SLIDE:
1  Judul + tagline.  2  Masalah & keselarasan track.  3  Ikhtisar solusi.
4  USP / diferensiasi.  5  PoC: fitur inti & walkthrough.
6  PoC: bukti hasil & status terverifikasi.  7  Arsitektur & tech stack.
8  Kerangka keamanan.  9  Skalabilitas & roadmap deployment.
10 Dampak.  11 Kesimpulan.  12 Q&A / terima kasih.
13-15 (backup, hanya dibuka saat tanya-jawab): keterbatasan, analisis IP,
     benchmark performa + placeholder data yang bisa diisi.

Untuk versi bahasa Inggris, terjemahkan seluruh teks slide ke Inggris profesional dan
pertahankan istilah teknis (TLV, CRC-16, Sobel, Haversine, CSP).
```

---

## BAGIAN 2: MATERI PER SLIDE

Total 13 slide utama + 3 backup. Alokasi untuk pitch ±8–9 menit, sisanya Q&A.

### Slide 1: Judul (±15 detik)
- Anti Timpa: *On-Device QRIS Tamper Detection*
- Tagline: "Deteksi QRIS yang ditempel orang lain, langsung di perangkat, 100% offline."
- Baris bawah: `<Nama Tim>` · HackNusa 2026 · Track Cybersecurity / Trust & Safety in Digital Payments
- [SISIPKAN: logo tim/institusi]
- Notes: "Kami menunjukkan cara mendeteksi serangan QRIS paling umum hari ini, tanpa server dan tanpa mengirim data apa pun."

### Slide 2: Masalah & Keselarasan Track (±60 detik)
- QRIS = alat bayar utama UMKM; tetapi token-nya fisik dan mudah diganti (kertas, laminasi, stiker).
- Serangan overlay: penyerang menempel QRIS miliknya di atas milik merchant, sehingga pelanggan membayar ke penyerang.
- Nama merchant bisa dibuat mirip; bank hanya memastikan kode valid, dan memang valid (milik penyerang).
- Akar masalah: semua kontrol yang ada memeriksa data, tidak ada yang memeriksa objek fisik.
- Selaras track: melindungi kepercayaan pembayaran dari sisi pengguna akhir, tanpa mengubah rails QRIS.
- [SISIPKAN: infografis 4 langkah serangan overlay]

### Slide 3: Ikhtisar Solusi (±45 detik)
- Satu aplikasi lintas platform (desktop · Android · iOS) dari satu codebase Rust + Tauri 2 + React.
- Tiga lapisan analisis lokal:
  - L1 Optical: memeriksa objek fisik (edge density + glare).
  - L2 EMVCo: validitas payload (TLV + CRC-16 + 4 aturan risiko).
  - L3 Geofence: kelayakan lokasi (kota + jarak Haversine).
- Output: verdict risiko + temuan bernama + laporan coverage yang bisa dibagikan.
- [SISIPKAN: diagram 3 lapisan]

### Slide 4: USP / Diferensiasi (±60 detik)
- Hanya solusi yang memeriksa objek fisik; penyerang mengubah objek, bukan data.
- 100% on-device, ditegakkan arsitektur (CSP `default-src 'self'`): tidak ada backend untuk dibobol.
- Melaporkan coverage sendiri: scan sebagian tidak pernah tampak "aman" (ada banner peringatan).
- Menolak membandingkan saat tak bermakna: `NOT_COMPARABLE` / `NOT RUN`, bukan skor nol palsu.
- Offline secara arsitektur: mode pesawat, sinyal jelek, basement, hasil sama.
- Bandingkan dengan alternatif: scanner bank/PSP (hanya payload), app scanner generik (tanpa analisis keamanan), label hologram (biaya merchant, tanpa verifikasi pembeli).

### Slide 5: PoC: Fitur Inti & Walkthrough (±75 detik)
- Alur: burst 4 frame → decode QR (`rqrr`, pure Rust) → L1 + L2 + L3 → skor gabungan → findings + riwayat.
- Dua mode UI: Sederhana (verdict + bahasa awam) dan Teknis (metrik per-layer, payload mentah).
- Fitur jadi: analisis kamera, impor gambar (jalur validasi), riwayat dengan rantai hash tamper-evident.
- [SISIPKAN: 3 screenshot aplikasi build Tauri saat ini: simple mode, kartu hasil, coverage banner]

### Slide 6: PoC: Bukti Hasil & Status Terverifikasi (±75 detik)
- Hasil fixture (build terkini):
  - clean QR → 0.000 · LOW RISK
  - QR berstiker → edge 0.2767 → 0.371 · CAUTION
  - CRC dimanipulasi → 1.000 · HIGH RISK (veto)
  - wisatawan beda kota → 0.650 · CAUTION (tanpa veto)
- Poin kunci: clean vs stiker punya payload identik; itulah serangannya, dan alasan L1 menjadi inti.
- Ring width 0.12 → separasi edge ~8,6× antara clean dan stiker.
- 162 unit test lulus; kamera Android terverifikasi di perangkat (Samsung A14, Android 15).

### Slide 7: Arsitektur & Tech Stack (±60 detik)
- Diagram alur (lihat [SISIPKAN: diagram arsitektur & alur data]):
  kamera → decode → L1/L2/L3 → skor (veto CRC, selain itu `max`) → findings + hash chain → laporan → UI.
- Satu-satunya trust boundary = IPC in-process WebView ↔ core Rust. Tidak ada egress jaringan.
- Stack: Rust (inti analisis), Tauri 2 (shell aman, CSP ketat), React 18 + TS (UI), `rqrr` (decode), `nokhwa`/CameraX (kamera).

### Slide 8: Kerangka Keamanan (±60 detik)
- Threat model: A1 overlay attacker (utama), A2 payload forger, A3 local attacker, A4 curious third party.
- Kontrol: L1 menangani A1; CRC + 4 aturan menangani A2; rantai hash menangani A3; CSP & tanpa egress menangani A4.
- Properti: tanpa attack surface eksternal, tanpa rahasia untuk bocor, memory-safe (Rust), input penyerang hanya ditampilkan sebagai teks.
- Lokasi least-privilege: opt-in, coarse-only, tidak dipersistensi; desktop tanpa izin sama sekali.

### Slide 9: Skalabilitas & Roadmap Deployment (±60 detik)
- Tanpa server, biaya marginal nol. Skala ke sejuta pengguna tanpa capacity planning.
- Roadmap: P1 kalibrasi lapangan (prioritas tertinggi, 4–6 minggu) → P2 pelengkapan platform → P3 adopsi (app + SDK) → P4 ekosistem & engagement Bank Indonesia.
- Infrastruktur produksi: tidak ada backend; distribusi via app store (~10–15 MB).
- [SISIPKAN: timeline roadmap] · [SISIPKAN DATA: tabel performa on-device]

### Slide 10: Dampak (±45 detik)
- Pelanggan: perlindungan nyata sebelum uang berpindah, tanpa instalasi rumit dan tanpa akun.
- UMKM & ekosistem: menurunkan overlay fraud tanpa biaya tambahan bagi merchant.
- Jalur dampak terbesar: SDK untuk aplikasi PSP/bank; pengguna lama dapat pemeriksaan optikal tanpa install baru.
- Model keberlanjutan: lisensi B2B SDK + white-label, dengan free tier untuk publik.

### Slide 11: Kesimpulan (±30 detik)
- Anti Timpa menyerang satu titik yang dilewatkan semua kontrol: objek fisik.
- Tanpa server, tanpa unggah, tanpa akun; klaim privasi bersifat arsitektural, bukan janji kebijakan.
- Risiko engineering rendah; sisa pekerjaan adalah kalibrasi data, bukan kelayakan.
- Coverage yang jujur = standar minimum untuk produk keamanan yang diandalkan orang untuk memutuskan mengirim uang.

### Slide 12: Q&A / Terima Kasih (±20 detik)
- "Terima kasih. Kami siap menerima pertanyaan."
- Cantumkan repo & kontak.
- Siapkan deck backup untuk dibuka saat Q&A.

### Slide 13–15: Backup (buka saat Q&A saja)
- Keterbatasan: threshold sintetik, tabel kota kecil, iOS/Windows/macOS belum diverifikasi, belum diuji pada foto overlay asli.
- Analisis IP: paten kemungkinan gagal (prior art); moat sebenarnya = dataset hasil tangkapan berlabel + threshold ter-fit + publikasi hasil negatif.
- Benchmark: [SISIPKAN DATA: performa on-device per perangkat].

---

## BAGIAN 3: ANTISIPASI PERTANYAAN JURI

| Pertanyaan | Jawaban singkat |
|---|---|
| Bagaimana kalau penyerang meniru geometri stiker persis? | L1 mendeteksi edge sembarang di margin, bukan satu pola stiker. Untuk lolos, overlay harus membiarkan quiet zone utuh, hal yang mustahil bagi overlay yang opak. Verdict tetap advisory ("konfirmasi ke merchant"). |
| Akurasi sudah berapa? | Belum ada presisi/recall lapangan; itu Phase 1. Semua angka saat ini dari fixture sintetik, dan kami menyatakannya terbuka. |
| Kenapa tidak pakai server saja? | Server = attack surface, biaya, dan pelanggaran privasi. Karena analisis bisa dilakukan di perangkat, server tidak memberi nilai tambah. |
| Bagaimana kalau HP-nya dimodifikasi/di-root? | Di luar cakupan: malware dengan kontrol perangkat bisa mengalahkan pemeriksaan on-device apa pun. |
| Bisakah dipatenkan? | Kemungkinan kecil (teknik visi komputer lama). Kami fokus ke dataset & eksekusi, bukan paten. |
| Bagaimana integrasi ke bank? | SDK: core Rust diekspos sebagai library; surface integrasi = satu pemanggilan fungsi. |

---

## BAGIAN 4: CHECKLIST ASET VISUAL

Siapkan sebelum generate/menyusun deck:

- [ ] Logo tim / institusi (Sl. 1)
- [ ] Infografis serangan overlay (Sl. 2)
- [ ] Diagram 3 lapisan (Sl. 3)
- [ ] 3 screenshot aplikasi Tauri terkini (Sl. 5)
- [ ] Diagram arsitektur & alur data (Sl. 7)
- [ ] Timeline roadmap (Sl. 9)
- [ ] Tabel benchmark on-device (Sl. 9 / 15): ukur, jangan mengira-ngira
- [ ] Catatan pembicara tercetak (opsional, untuk sesi pitch)
