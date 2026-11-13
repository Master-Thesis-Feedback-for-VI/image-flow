# Image Flow

Image Flow is a library for detecting objects and inferring distance in cross
platform way. The only requirement is that ONNX can run on the given architecture.
Additionally, this library provides features for categorizing a coordinate and/or an
image into a set of predefined categories.

As an example usage of this library can be found in
[this](https://github.com/Master-Thesis-Feedback-for-VI/Platypus-Apple) app.
It uses the features for helping individuals with vision impairment to better
navigate their environment.

The core API is this function:
```rust
#[uniffi::export(async_runtime = "tokio")]
pub async fn image_flow_evaluation(
    location: Coordinate,
    auth: Option<AuthToHost>,
    img_path: &str,
    image_recognizer: Arc<ImageRecognizer>,
    amount: u32,
    confidence_limit: f32,
) -> DefaultResult<ImageFlowEvaluation, ImageFlowError> {
  ...
}
```

This creates a struct which contain the location tags and the detections of that
image.

## Pipeline Script

By running `just pipeline_all` a CLI is started which processes all zip files in
`sample_data/*.zip`. A zip file consists of a video file and a JSON file with
a lot of GPS tags. It then gets processed into another JSON which describes which
objects are detected where and what location tag this system assigns to each frame.
We can use that outputted file in the
[Platypus-App](https://github.com/Master-Thesis-Feedback-for-VI/Platypus-Apple)
as a simulation.

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
