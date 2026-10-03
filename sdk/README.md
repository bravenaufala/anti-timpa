# Anti Timpa SDK scaffolding

This directory holds the integration path for embedding the Anti Timpa core in a
bank / PSP application. The status table below separates working code from
*scaffolding* that has not been built in this environment.

| Piece | Status |
|---|---|
| C-ABI functions (`antitimpa_version`, `antitimpa_analyze_payload`, `antitimpa_free_string`) | Implemented and unit-tested (see `src-tauri/src/sdk.rs`) |
| One shared analyzer for app + SDK | Done; both call `analyze_payload_snapshot` |
| `.so` / `.a` cross-compilation | Requires Android NDK / Xcode; script provided, not run here |
| `.aar` (Android) / `.framework` (iOS) packaging | Scaffolding; layout scripted, artifacts unverified |
| Kotlin / Swift binding generation | Manual; the ABI is JSON-in/JSON-out, no codegen needed |

## The ABI

Three functions, all `extern "C"`. JSON in, JSON out, so no language-specific
type graph has to be maintained.

```c
const char *antitimpa_version(void);

/* Returns a heap string the caller must free, or NULL for invalid input. */
char *antitimpa_analyze_payload(const char *payload, const char *client_city);

void antitimpa_free_string(char *ptr);
```

`client_city` may be `NULL`; the geofence layer then reports `NOT RUN` rather
than a false "safe". Layer 1 does not run on this path (there is no frame) and
the returned JSON states that explicitly via `coverage.optical_ran`.

Example (JSON reply, trimmed):

```json
{
  "combined_risk_level": "HIGH RISK",
  "combined_score": 1.0,
  "scannable": true,
  "coverage": { "optical_ran": false, "payload_ran": true, "geofence_ran": true, "complete": false },
  "findings": [{ "code": "L2_CRC_MISMATCH", "severity": "HIGH" }]
}
```

## Building the native libraries

```bash
./generate-bindings.sh android   # needs ANDROID_NDK_HOME + android targets
./generate-bindings.sh ios       # needs macOS + Xcode
```

The script fails loudly when a toolchain is missing rather than producing a
silently broken artifact.

### Android prerequisites

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi
export ANDROID_NDK_HOME=/path/to/ndk
```

### iOS prerequisites

On macOS:

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios
```

## Why not UniFFI?

UniFFI is a reasonable alternative and was considered. It is not used here
because the surface is three functions over a string, and UniFFI would add a
build-time code generator plus a runtime dependency for no benefit at this size.
If the surface grows into structured types, UniFFI becomes the better choice.
The analyzer behind `antitimpa_analyze_payload` is the single transfer point, so
switching later is a change in this directory only, not in the core.

## What is not here

- No key material and no network calls. The SDK has no attack surface beyond
  the strings it is handed.
- No account/merchant verification against a server. Anti Timpa parses and
  reports the merchant identifier (`merchant_id` in the snapshot); confirming it
  against a registry is the host bank's job.
