# Cloud voice backend

This module implements the transport-free `tinyvoice-bus::provider` interface
for the TinyHumans backend. Host composition installs its factory through embed's
explicit cloud voice seam. Each call resolves its current configuration and
credential, so token rotation and different user scopes do not reuse a credential
captured at process installation.

`transcription.rs` validates the multipart input and dispatches through the native
SDK bounded response helper. It preserves the 32 MiB encoded-input limit, 64 KiB
response limit, default `whisper-v1` model and endpoint-specific Bearer handling
for both API keys and session JWTs. HTTPS and loopback restrictions apply before
dispatch; API keys also require the managed backend or loopback.

`requests.rs` owns reply-speech request JSON and backend session-ticket parsing.
Reply speech and realtime bootstrap use the shared backend client, preserving
session-Bearer and API-key-header behavior and typed authentication failures.
Provider errors preserve categories and remove active credential echoes before
returning safe details to core.

The core voice domain owns provider selection, local fallbacks, RPC outcomes and
its established response normalization. Native TinyVoice processing runs through
the compiled module; this backend provider does not replace its behavior.

Sibling unit tests cover validation, endpoint policy, request shapes and typed
failure projection without network access. `tests/cloud_voice_backend_e2e.rs` covers
the installed composition against local HTTP mocks, including credential
rotation, multipart headers, response limits, and realtime and synthesis routes.
