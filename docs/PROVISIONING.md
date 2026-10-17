# BLE Wi-Fi provisioning — protocol v1

How a phone gives a new frame its Wi-Fi credentials. Implemented by
`crates/daemon/src/provision/` (device) and `app/android/.../provision/` (phone).
No earlier GATT spec was found in the repo, so this one starts at v1.

## When the device provisions

| Trigger | Advertises | Setup code on panel | Ends |
|---|---|---|---|
| No Wi-Fi profile in NetworkManager (first boot) | immediately | immediately | Wi-Fi configured **and** online |
| Offline ≥ `offline_start_mins` (default 30) | yes | only once a phone says hello | back online |
| Button held `button_hold_secs` (optional GPIO, `button_pin`) | yes | on hello | `idle_timeout_mins` without a phone |
| `POST /api/v1/provision/start` (debug page / USB) | yes | on hello | same |

All runs also end when the phone sends `done`. A connected phone is never cut off. While a run
is active the scheduler does not paint over the code, and the network task does not
restart NetworkManager or reboot. When a run that painted the code ends, the schedule is redrawn.

## GATT

| | UUID | Properties |
|---|---|---|
| Service | `6f1a0001-8b5e-4c1f-9d3a-1e5f0a7c2b90` | primary, in the advertisement |
| Info | `6f1a0002-…` | read: `{"proto":1,"id":"3fa2","name":"inkframe-3fa2","fw":"0.1.0"}` |
| RX | `6f1a0003-…` | write **with** response (phone → device) |
| TX | `6f1a0004-…` | notify (device → phone) |

The advertised local name is `inkframe-<id>`, where `id` is 4 hex chars of SHA-256(`/etc/machine-id`).
The phone must subscribe to TX before writing RX; a session begins at subscription and ends on
unsubscribe or disconnect. One phone at a time.

## Framing

Every RX write and TX notification is one fragment: a header byte, then payload. Header bit 0
(`0x01`) means more fragments follow. Other bits are reserved and must be 0. Concatenated payloads
form one UTF-8 JSON message of at most 4096 bytes. Fragment size is ATT MTU − 3 (the app requests
an MTU of 247).

## Messages

Plaintext envelope, discriminated by `"t"`:

```
phone  → {"t":"hello","v":1,"pub":"<b64 X25519 public key>"}
device → {"t":"hello","v":1,"pub":"<b64>","salt":"<b64 16 random bytes>"}
phone  → {"t":"auth","mac":"<b64 HMAC>"}
device → {"t":"auth","mac":"<b64 HMAC>"}        or {"t":"error","code":"bad_code",…}
either → {"t":"sealed","c":"<b64 AES-GCM ciphertext+tag>"}
device → {"t":"error","code":"…","msg":"…"}      (before authentication)
```

Sealed requests (phone → device) and replies:

| Request | Reply |
|---|---|
| `{"t":"scan"}` | `{"t":"networks","list":[{"ssid":"Home","signal":82,"secure":true}]}` (strongest first, ≤ 25) |
| `{"t":"connect","ssid":"…","psk":"…","hidden":false}` | `state: connecting`, then `connected` or `failed` (≤ 75 s) |
| `{"t":"status"}` | the last `state`, or `{"state":"idle","ssid":…,"ip":…}` |
| `{"t":"done"}` | `{"t":"bye"}`; the device leaves provisioning |

`state` replies look like `{"t":"state","state":"connected","ssid":"Home","ip":"192.168.1.23","host":"inkframe-3fa2.local","port":8080}`.
`failed` carries `reason` (`auth_failed`, `not_found`, `timeout`, `error`) and `detail`.
Error codes: `bad_message`, `unsupported_version`, `bad_code`, `not_authenticated`, `busy`, `not_connected`.

## Security

The Wi-Fi password must not cross the air in clear, and a neighbour must not be able to
provision your frame. Both sides know a 12-character Crockford-base32 **setup code** (60 bits)
that only the panel shows, as `XXXX-XXXX-XXXX`. Input is normalised: case-insensitive, `-` and
spaces ignored, O→0, I/L→1.

```
shared   = X25519(own secret, peer public)          reject all-zero (low-order) results
ikm      = shared ‖ SHA-256(code)
okm      = HKDF-SHA256(salt, ikm, info = "inkframe/prov/v1" ‖ phone_pub ‖ device_pub), 96 bytes
confirm, k_phone→device, k_device→phone = okm[0..32], okm[32..64], okm[64..96]
phone mac  = HMAC-SHA256(confirm, "phone")    device verifies (constant time) before replying
device mac = HMAC-SHA256(confirm, "device")   phone verifies, so a fake frame is detected
sealed   = AES-256-GCM(k_dir, nonce = 0x00000000 ‖ u64 BE counter, no AAD)
```

Counters start at 0 per direction and per handshake. Any decryption failure closes the link,
which also rejects replays and dropped messages. A new `hello` restarts the handshake.

After `max_auth_failures` (default 5) wrong codes the device draws a new code, so online guessing
costs a panel refresh (~25 s) per batch. An active man-in-the-middle who captures one phone `auth`
could brute-force the code offline (60 bits, salted per session), which is impractical within the
provisioning window. A PAKE (SPAKE2/SRP) would remove that at the cost of a less common
Kotlin dependency. Revisit if the code is ever shortened.

The cross-language test vector lives in `crates/daemon/src/provision/testdata/vector.txt`.
It is written by `session::tests::cross_language_vector` and replayed by `ProtocolTest.kt`.

## Wi-Fi on the device

`nmcli` with a timeout on every call. Profiles are named `inkframe-<ssid>`, so only our own are
replaced. A failed attempt deletes its profile, so NetworkManager never retries a wrong password.
A successful one gets `autoconnect-priority 10`. The password appears only in nmcli's argv, never
in logs.

## Development without a radio

`config/dev.toml` sets `transport = "sim"` and `wifi = "mock"`. The simulator carries the same
fragments over TCP (`kind u8, len u16 BE, payload`; kind 1 = info, 2 = RX write, 3 = TX
notify) on `sim_listen`. From the Android emulator it is `10.0.2.2:8090`. Mock networks accept the
password `inkframe123`. With debug endpoints on, `GET /api/v1/debug/provision-code` returns the code
for automated tests.
