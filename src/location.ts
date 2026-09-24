/**
 * Location acquisition, per platform.
 *
 * The previous implementation called `navigator.geolocation` directly. That
 * never worked, and the failure was silent: inside a Tauri WebView on Linux the
 * WebKitGTK geolocation backend is not wired to anything, so the call resolves to
 * an error, `readLocation` swallows it and returns `null`, and Layer 3 reports
 * "client location unavailable" on every single scan. The UI *looked* like it
 * supported location (there was a checkbox) while the data never arrived.
 *
 * So there are now three distinct, explicit sources:
 *
 * | Platform | Source | Why |
 * |---|---|---|
 * | Mobile (Android/iOS) | `@tauri-apps/plugin-geolocation` | Real OS location service, with a proper permission prompt |
 * | Desktop | Offline city table, resolved from the typed city name | Desktop has no OS location service to ask |
 * | Browser (`npm run dev`) | `navigator.geolocation`, best-effort | Present so UI work outside Tauri is possible; not a supported path |
 *
 * The desktop choice deserves justification, because "why not just use IP
 * geolocation or `navigator.geolocation` on desktop" is the obvious question.
 * Both would send the user's location to a third party, which contradicts the
 * app's central guarantee that no scan data leaves the device. The offline table
 * costs one typed word and no privacy, and the comparison it enables — "is this
 * merchant's city plausible for where I am" — only needs city resolution, not
 * GPS precision.
 */

import { geocodeCity, isTauri } from "./api";
import type { ClientLocation } from "./types";

/** Where a location value came from, so the UI can explain what it used. */
export type LocationSource =
  | "none"
  | "typed_city"
  | "offline_table"
  | "device_gps"
  | "browser";

export interface ResolvedLocation extends ClientLocation {
  source: LocationSource;
  /** Set when a source was attempted and failed, for display. */
  note: string | null;
}

const EMPTY: ResolvedLocation = {
  city: null,
  lat: null,
  lon: null,
  source: "none",
  note: null,
};

/** Whether the OS location plugin is usable here (mobile builds only). */
const isMobile = (): boolean => {
  if (typeof navigator === "undefined") return false;
  return /android|iphone|ipad|ipod/i.test(navigator.userAgent);
};

/**
 * Reads a position from the OS location service via the Tauri plugin.
 *
 * Permission is requested here rather than at launch, so the prompt is
 * triggered by the user actually running a scan with location enabled. A denial
 * is not an error: it is reported as a note and the caller degrades to the city
 * name.
 */
async function readDeviceGps(): Promise<ResolvedLocation | null> {
  try {
    const geo = await import("@tauri-apps/plugin-geolocation");

    let perm = await geo.checkPermissions();
    if (perm.location === "prompt" || perm.location === "prompt-with-rationale") {
      perm = await geo.requestPermissions(["location"]);
    }
    if (perm.location !== "granted") {
      return {
        ...EMPTY,
        source: "none",
        note: "Izin lokasi ditolak; perbandingan jarak dilewati.",
      };
    }

    const pos = await geo.getCurrentPosition({
      // Coarse on purpose: Layer 3 needs a city-scale position, not a precise
      // one, and asking for precision would be collecting more than the feature
      // needs.
      enableHighAccuracy: false,
      timeout: 8000,
      maximumAge: 300_000,
    });

    return {
      city: null,
      lat: pos.coords.latitude,
      lon: pos.coords.longitude,
      source: "device_gps",
      note: null,
    };
  } catch (e) {
    return {
      ...EMPTY,
      source: "none",
      note: `Layanan lokasi perangkat gagal: ${e instanceof Error ? e.message : String(e)}`,
    };
  }
}

/** Browser fallback, for running the UI outside Tauri. */
async function readBrowserLocation(): Promise<ResolvedLocation> {
  if (typeof navigator === "undefined" || !navigator.geolocation) {
    return { ...EMPTY, note: "Peramban tidak menyediakan layanan lokasi." };
  }
  return new Promise((resolve) => {
    navigator.geolocation.getCurrentPosition(
      (pos) =>
        resolve({
          city: null,
          lat: pos.coords.latitude,
          lon: pos.coords.longitude,
          source: "browser",
          note: null,
        }),
      (err) =>
        resolve({
          ...EMPTY,
          note: `Lokasi peramban gagal: ${err.message}`,
        }),
      { enableHighAccuracy: false, timeout: 5000, maximumAge: 300_000 },
    );
  });
}

/**
 * Resolves the location Layer 3 should use.
 *
 * Order of preference, and the reasoning for it:
 *
 * 1. **A device GPS fix**, when the user opted in and the platform has a real
 *    location service. This is the only true measurement.
 * 2. **The offline city table**, from the typed city name. No GPS, no network,
 *    no permission — and it resolves to the same city reference point the Layer 3
 *    table uses, so the resulting distance is meaningful even without a fix.
 * 3. **Nothing.** Layer 3 then reports `NOT RUN` and the UI says the comparison
 *    did not happen, which is the correct outcome when location is genuinely
 *    unknown.
 *
 * @param cityName what the user typed, if anything
 * @param allowDeviceGps whether the user enabled the precise-location toggle
 */
export async function resolveLocation(
  cityName: string | null,
  allowDeviceGps: boolean,
): Promise<ResolvedLocation> {
  const typed = cityName?.trim() || null;
  let note: string | null = null;

  // 1. Device GPS, when available and permitted.
  if (allowDeviceGps) {
    if (isTauri() && isMobile()) {
      const gps = await readDeviceGps();
      if (gps) {
        if (gps.lat !== null) {
          // A fix plus a typed city gives the most informative verdict, because
          // the city name still drives the naming tiers while the fix drives the
          // distance.
          return { ...gps, city: typed };
        }
        note = gps.note;
      }
    } else if (isTauri()) {
      // Desktop: no OS location service. Say so instead of failing quietly.
      note =
        "Desktop tidak punya layanan lokasi sistem; jarak dihitung dari nama kota di bawah.";
    } else {
      const browser = await readBrowserLocation();
      if (browser.lat !== null) return { ...browser, city: typed };
      note = browser.note;
    }
  }

  // 2. Offline table, from the typed city. This is the desktop path, and it also
  //    covers mobile when GPS was denied or unavailable.
  if (typed) {
    try {
      const coords = await geocodeCity(typed);
      if (coords) {
        return { city: typed, lat: coords[0], lon: coords[1], source: "offline_table", note };
      }
      return {
        city: typed,
        lat: null,
        lon: null,
        source: "typed_city",
        note:
          note ??
          `"${typed}" tidak ada di tabel kota offline, jadi jarak tidak dihitung. ` +
            `Perbandingan nama kota tetap dilakukan.`,
      };
    } catch (e) {
      return {
        city: typed,
        lat: null,
        lon: null,
        source: "typed_city",
        note: `Gagal membaca tabel kota: ${e instanceof Error ? e.message : String(e)}`,
      };
    }
  }

  // 3. Nothing available.
  return { ...EMPTY, note };
}

/** Human-readable description of what the resolver used. */
export function describeLocation(loc: ResolvedLocation): string {
  switch (loc.source) {
    case "device_gps":
      return `GPS perangkat${
        loc.lat !== null && loc.lon !== null
          ? ` (${loc.lat.toFixed(3)}, ${loc.lon.toFixed(3)})`
          : ""
      }`;
    case "offline_table":
      return `Tabel kota offline${
        loc.lat !== null && loc.lon !== null
          ? ` (${loc.lat.toFixed(3)}, ${loc.lon.toFixed(3)})`
          : ""
      }`;
    case "typed_city":
      return "Nama kota saja (koordinat tidak diketahui)";
    case "browser":
      return "Lokasi peramban";
    default:
      return "Tidak tersedia";
  }
}
