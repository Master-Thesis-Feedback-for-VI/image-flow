# Image Flow

Image Flow is a Rust library for object detection, monocular depth estimation,
and geographic context classification using images and OpenStreetMap data. It
also supports video/GPS processing and UniFFI bindings for mobile applications.
The [Platypus app](https://github.com/Master-Thesis-Feedback-for-VI/Platypus-Apple)
uses it to help people with vision impairment navigate their environment.

The workspace contains the `image_flow_lib` package (imported as `image_flow`)
and `uniffi-bindgen-runner` for generating bindings.

**Start with the [library guide](image_flow_lib/README.md)** for Rust examples,
models, feature flags, map configuration, detection/depth results, GPS input,
frame ownership, JSON formats, errors, and mobile bindings.

Generate the API reference, including the guide and public-item documentation:

```sh
cargo doc -p image_flow_lib --lib --no-default-features --no-deps --open
```

You should activate devenv. Omit `--no-default-features` to include video support.

## Pipeline Script

The pipeline accepts one ZIP archive or a directory of ZIP archives. Each input
must contain exactly one `.mov` video and one `gps_timestamps.csv` in the same
recording folder. The CSV uses Unix-second timestamps and the columns
`timestamp,latitude,longitude`.

```sh
cargo run -p image_flow_lib --bin pipeline --release --features "video,cli" -- \
  --input sample_data \
  --output pipeline_out/work \
  --serialized-output pipeline_out/results \
  --error-log pipeline_out/errors.log
```

It samples frames every 400 ms, pairs them with GPS, and evaluates each frame
using large object/depth models. Each recording produces `content.json`, a
`recording.mov` with reduced audio volume, and a ZIP of the output folder for
playback in the Platypus app. With directory input, outputs are grouped by input
filename stem. Model files are reused in `--output` when supplied; otherwise
they are downloaded into temporary storage. Frame failures are logged and
skipped; recording failures are logged while remaining archives continue.

`just pipeline_all` runs this on `sample_data/` after removing the existing
`out/` and `pipeline_out/` directories.

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
