# Image Flow

## Tests

Use the nightly Rust toolchain from `rust-toolchain.toml` (including Clippy,
which is configured as the workspace compiler wrapper).

```sh
cargo test --workspace --no-default-features  # core library, no FFmpeg needed
cargo test --workspace                       # also tests video decoding
cargo test --workspace --all-features        # also tests CLI parsing and ZIP output
```

These are also available as `just test-core`, `just test`, and `just test-all`.
The video-enabled builds require FFmpeg development libraries and `pkg-config`;
the provided devenv includes these. ONNX Runtime is still a build dependency,
but the default tests do not load or download models.

The default suite uses temporary files, generated images and a tiny generated
video, plus loopback HTTP fixtures. It does not call external services, require
credentials, or modify `sample_data/` or shared output directories. It covers
location-tag invariants, GPS parsing/matching, bounding-box geometry, detection
postprocessing and depth, image-color heuristics, model URLs/download failures,
map response parsing/retries, JSON persistence, frame ownership, video/GPS
pairing, and CLI/archive behavior. Generated mobile bindings and the bindgen
launcher are not independently unit-tested.

### Opt-in integration tests

Existing tests that use live Nominatim/Overpass services, ONNX models, sample
images, or large recordings are marked `#[ignore]` with prerequisite messages:

```sh
cargo test -p image_flow_lib --lib -- --ignored --test-threads=1
```

Or run `just test-integration`. The full sample collection must be present
(fetch Git LFS assets if applicable). Inference tests need compatible ONNX models
and may download missing models. Private map services can be configured with
`USER_AGENT_NAME`, `PASSWORD`, `OVERPASS_URL`, and `NOMINATIM_URL` **at build time**
because the current configuration uses `option_env!`. These tests may write to
`out/` and live-data snapshots may change; run them explicitly, not as a default
CI suite. To select only one, add its test name before `--`.
