# `image_flow_lib`

Image Flow combines object detection, monocular depth estimation, and geographic
context for an image. It can return detected objects with estimated distances,
classify a location using OpenStreetMap data and image colors, and pair video
frames with recorded GPS samples. `UniFFI` exports support generating Swift and
Kotlin bindings.

The Cargo package is named **`image_flow_lib`**; the Rust crate and native library
are named **`image_flow`**.

## Evaluate an image

Create the model directory before constructing an `ImageRecognizer`. The
constructor loads **both** the object and depth models, downloading missing files.
Reuse the recognizer across images to avoid repeatedly loading the models.

```rust,no_run
use std::sync::Arc;

use image_flow::{
    context::image_based_context::{ImageRecognizer, ModelVariant},
    image_flow_evaluation,
    location::coordinate_system::Coordinate,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::fs::create_dir_all("models").await?;
    let recognizer = Arc::new(
        ImageRecognizer::new(ModelVariant::Nano, "models".to_owned()).await?,
    );

    let evaluation = image_flow_evaluation(
        Coordinate { lat: 52.3759, lon: 9.7320 },
        None, // Use the default map-service configuration.
        "frame.jpg",
        Arc::clone(&recognizer),
        4,   // Number of dominant-color clusters, not a limit on detections.
        0.3, // Confidence threshold, clamped to 0.0..=1.0.
    )
    .await?;

    println!("{}", evaluation.location_tags);
    println!("{}", evaluation.image_detections);
    evaluation.save_as_json("evaluation.json").await?;
    Ok(())
}
```

`ImageFlowEvaluation` exposes `location_tags` and `image_detections` as public
fields in Rust. Access them directly to borrow or modify results; the accessor
methods return clones.

This combines object/depth inference with dominant-color analysis and map
lookups. Object and depth predictions run on blocking worker threads. Each model
has its own mutex, so calls sharing a recognizer serialize access to each model.
All async entry points need a Tokio runtime when called from Rust; exported async
functions use `UniFFI`'s Tokio integration for foreign callers.

`amount` is the k-means cluster count; use a small positive value such as `2` or
`4`, this represents the amount of colors extracted from a given image.

`confidence_limit` is clamped to `0.0..=1.0` before filtering: values below `0.0`
become `0.0`, and values above `1.0` become `1.0`. Detections at or above the
clamped threshold are retained.

### Choose an entry point

| Function | Work performed | Recognizer needed? |
| --- | --- | --- |
| `image_flow_evaluation` | Object detection, depth, map context, and image heuristics | Yes |
| `recognize_objects_in_image` | Object detection and depth for each detection | Yes |
| `recognize_depth_in_image` | Per-pixel depth only | Yes |
| `recognize_location_tags` | Map context and dominant-color hints | No |
| `context::location_based_context::evaluate_location_as_loaction_tags` | Map context only | No |

### Model files

The constructor uses these exact filenames in the supplied directory:

| `ModelVariant` | Object model | Depth model |
| --- | --- | --- |
| `Nano` | `yolo26n.onnx` | `yolo26n-depth.onnx` |
| `Small` | `yolo26s.onnx` | `yolo26s-depth.onnx` |
| `Medium` | `yolo26m.onnx` | `yolo26m-depth.onnx` |
| `Large` | `yolo26l.onnx` | `yolo26l-depth.onnx` |
| `XLarge` | `yolo26x.onnx` | `yolo26x-depth.onnx` |

Missing files are requested from
`https://github.com/ultralytics/assets/releases/download/v8.4.0/`.
These are the names and release path configured in this library; initialization
fails if a requested asset is unavailable or a file cannot be loaded.

For inference without network access, place compatible object **and** depth ONNX
models at those paths first. Existing files are loaded without downloading or
refreshing them. 

## Interpret detections and depth

`ImageDetections` supports `.iter()`, `.len()`, `.is_empty()`, owned iteration,
`into_inner()`, and `get_by_detected_object(DetectionType::Person)` for filtering.
Each `Detection` serializes as:

```json
{
  "name": "Person",
  "xyxy": { "x1": 10.0, "x2": 30.0, "y1": 20.0, "y2": 60.0 },
  "distance": 2.5,
  "confidence": 0.9
}
```

Bounding boxes use image pixel coordinates. `distance` is the depth sampled at
the floored center of the box. Values are passed through from the depth model
(the supported YOLO depth models report meters); the library adds no calibration.
`DepthMap::depth_at(x, y)` returns `None` outside the stored array.

Postprocessing first merges overlapping person/bicycle pairs into `Cyclist`
detections, with a combined box and averaged confidence/distance. It then removes
later detections of the same type whose intersection-over-union is at least
`0.9` with a retained box. Unmapped class labels become
`DetectionType::Unknown(String)`.

## Location tags and map services

The supported tags are `Railway`, `Parking`, `Street`, `Pedestrian`,
`Intersection`, `Forest`, `Outside`, `Inside`, and `Elevator`. They are heuristic
context labels; an empty set means no tag could be established.

`UniqueLocationTags` deduplicates tags and expands implications on insertion:

- `Forest` and `Intersection` also insert `Outside`.
- `Parking` also inserts `Street`.
- `Outside` removes `Inside`, including when both are inserted together.
- Other tags do not add an indoor/outdoor classification automatically.

```rust
use image_flow::location::tag::{LocationTag, UniqueLocationTags, sort_location_tags};

let mut tags = UniqueLocationTags::new_singular(LocationTag::Forest);
assert!(tags.contains(LocationTag::Outside));
tags.insert(LocationTag::Inside);
assert!(!tags.contains(LocationTag::Inside));

let sorted = sort_location_tags(&tags.iter().copied().collect::<Vec<_>>());
assert_eq!(sorted, vec![LocationTag::Forest, LocationTag::Outside]);
```

Iteration and JSON array order are unspecified because tags use a `HashSet`.
`sort_location_tags` sorts in enum declaration order, preserving duplicates.
`UniqueLocationTags::categories(&mut self)` **drains** the set; use `iter()` to
inspect it without removing its contents. Removing a tag does not remove its
previously implied tags. Serde deserialization does not reapply insertion rules.

Geographic evaluation uses Nominatim reverse geocoding and Overpass queries for
building containment, nearby intersections, and road distance. Nearby points of
interest alone do not establish building containment. Map classification
preserves known `Inside` results against weaker outdoor image/context hints;
a road-surface distance of at most two meters can override containment to account
for GPS drift. Image heuristics recognize foliage-like dominant colors as
`Forest`, and more than eight detected traffic lights as `Intersection`.

Map-request failures are logged and treated as missing evidence by the tagging
pipeline, so a successful evaluation may contain partial or empty tags. In debug
builds an unrecognized Nominatim class/type can panic in direct location-tagging
calls; combined evaluation reports that spawned-task failure as an I/O error.
Release builds log the classification error and continue without its tags.

To use custom services, pass `Some(AuthToHost { ... })` instead of `None`:

```rust
use image_flow::requesters::AuthToHost;

let auth = AuthToHost {
    overpass_url: "https://maps.example.org/api/interpreter".to_owned(),
    nominatim_url: "https://geocoder.example.org".to_owned(),
    user: "username".to_owned(),
    passwd: "password".to_owned(),
};
```

`nominatim_url` is a base URL without a trailing slash; `/reverse?...` is appended.
`overpass_url` is the complete interpreter endpoint. Nominatim uses HTTP Basic
authentication with the supplied credentials. Overpass first tries the custom
endpoint with Basic authentication, then tries public endpoints on failure;
the current retry list also repeats the custom URL between public candidates.
Passing custom services therefore does not disable public Overpass fallback.

With `None`, Nominatim uses the build-time `NOMINATIM_URL`, defaulting to
`https://nominatim.openstreetmap.org`, and Overpass uses its built-in public
endpoints. `IMAGE_FLOW_AGENT` and `IMAGE_FLOW_REFERER` override map-request
headers **at build time**. The built-in request throttle currently has a zero
minimum interval; callers must schedule requests to suit their service.

## GPS input

`DataPointsGPS::from_csv` reads a CSV with `timestamp`, `latitude`, and
`longitude` columns. Timestamps are Unix seconds and may be fractional:

```csv
timestamp,latitude,longitude
1730000000.0,52.3759,9.7320
1730000000.4,52.3760,9.7321
```

The first row defines offset zero. Rows retain input order, so supply them in
timestamp order. Coordinates are stored as supplied; there is no GPS
interpolation or general latitude/longitude validation.

```rust
use chrono::Duration;
use image_flow::location::tabular_gps::DataPointsGPS;

let csv = b"timestamp,latitude,longitude\n1730000000.0,52.3759,9.7320\n1730000000.4,52.3760,9.7321\n";
let gps = DataPointsGPS::from_csv(&csv[..])?;
let sample = gps.fetch_best_match(Duration::milliseconds(350), Duration::milliseconds(100));
assert_eq!(sample.expect("nearby GPS sample").coordinates().lat, 52.3760);
# Ok::<(), image_flow::Error>(())
```

`fetch` includes both ends of the search window and treats a negative tolerance
as its absolute value. `fetch_best_match` chooses the nearest row, keeping the
first row on a tie. `fetch_within_bounds` uses inclusive bounds. `fetch_every`
requires a positive interval, uses a strict tolerance, and tests grid offsets
from zero up to (excluding) the last row's offset; it can return multiple rows
near a grid point.

## Video and frame ownership

With `video` enabled, `ImageGPSZip::from_video_and_gps` exports sampled PNG
frames and matches each to the nearest GPS row within **±400 milliseconds**.
Unmatched frames are dropped. Video offsets are relative to the first decoded
timestamp, which must align with the first GPS row for meaningful matching.

```rust,no_run
# #[cfg(feature = "video")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use chrono::Duration;
    use image_flow::{location::tabular_gps::DataPointsGPS, video_slicer::ImageGPSZip};

    let gps = DataPointsGPS::from_csv(std::fs::File::open("gps_timestamps.csv")?)?;
    let mut frames = ImageGPSZip::from_video_and_gps(
        "recording.mov", &gps, "frames", Duration::milliseconds(400),
    )
    .await?;

    for (coordinate, frame) in frames.iter() {
        println!("{coordinate}: {}", frame.as_ref().display());
    }
    frames.do_not_remove_frames(); // Keep the PNGs after this collection is dropped.
    Ok(())
}
# #[cfg(not(feature = "video"))]
# fn main() {}
```

For incremental processing, `export_images_from_video` returns a stream of
`Result<FramePath>` with a bounded channel. Use a positive capture interval;
frames are selected by timestamps rather than an exact frame rate. Rotation and
reflection metadata are applied to exported pixels. Decode/directory errors are
stream items; individual PNG save failures are logged and can still yield a path.

**A `FramePath` deletes its file when dropped by default**, including paths passed
to its constructors. Set its public `should_drop` field to `false` to retain a
single frame. Cloning a `FramePath` copies this cleanup flag and the path; clones
do not coordinate ownership, so dropping any owning clone can remove the file.
`ImageGPSZip::do_not_remove_frames()` disables cleanup for all retained frames.

`VideoMetadata::open` reports the file name and duration rounded to whole seconds.

## JSON persistence

`ImageFlowEvaluation::save_as_json` writes **one evaluation object**, overwriting
an existing file. It does not create parent directories. To load it in Rust,
deserialize it as `ImageFlowEvaluation` with Serde.

Recording playback uses a different format: a JSON **array** of
`ImageFlowEvaluationForRecording`, each with `evaluations`, `time_diff`, and
`location`. `time_diff` is a `std::time::Duration` serialized as `secs` and `nanos`.
`image_flow_evaluation_read_from_json` reads this array and preserves its order;
it cannot read a single-object file produced by `save_as_json`.

```rust,no_run
use std::{sync::Arc, time::Duration};
use image_flow::{
    ImageFlowEvaluation, ImageFlowEvaluationForRecording,
    image::detections::ImageDetections,
    image_flow_evaluation_read_from_json,
    location::{coordinate_system::Coordinate, tag::UniqueLocationTags},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let evaluation = ImageFlowEvaluation {
        location_tags: UniqueLocationTags::new(),
        image_detections: ImageDetections::new(),
    };
    let records = vec![ImageFlowEvaluationForRecording::new(
        Arc::new(evaluation),
        Duration::from_millis(400),
        Coordinate { lat: 52.3759, lon: 9.7320 },
    )];
    tokio::fs::write("content.json", serde_json::to_vec_pretty(&records)?).await?;
    let restored = image_flow_evaluation_read_from_json("content.json").await?;
    assert_eq!(restored[0].time_diff(), Duration::from_millis(400));
    Ok(())
}
```

Examples using GPS or JSON directly also require `chrono = "0.4"` or
`serde_json = "1"` in the consuming application's dependencies.

## Errors and logging

Rust `_impl` methods return `image_flow::Error`, which supports `Display`,
`Debug`, and `std::error::Error::source()`. `UniFFI`-facing methods return
`ImageFlowError` with `Request`, `Serialization`, `Io`, `Image`, and, when `video`
is enabled, `Ffmpeg` variants. Each variant contains a message string. Missing
image paths map to `Io`; JSON save/load operations also classify file-access
failures as `Serialization`.

Call `enable_tracing(Some(FilterLevel::Info))` to install a global tracing
subscriber. It returns whether this library successfully installed one (or had
already installed one). Passing `None` selects `Off`. If the application already
owns the global subscriber, the call returns `false`; configure that subscriber
to receive the library's tracing events instead.

## Mobile bindings

The crate builds `rlib`, `cdylib`, and `staticlib` artifacts. `UniFFI` exports the
top-level wrappers and selected objects, records, and enums; Rust-only methods
such as `_impl` helpers and the video module are not exported automatically.

The workspace includes `uniffi-bindgen-runner`. Existing `just` recipes build and
generate Swift bindings (`just create-bindings-swift`, on macOS) and Kotlin
bindings (`just create-bindings-kotlin`, with the Android NDK and `cargo-ndk`).
`just swift-build-xcode-bindings` assembles the iOS `XCFramework` on macOS. These
recipes replace generated output directories and require the relevant target
toolchains; mobile integration remains the responsibility of the calling app.

## Validation

Run these from the workspace root:

```sh
cargo test -p image_flow_lib --no-default-features
cargo test -p image_flow_lib
cargo test -p image_flow_lib --all-features
```

Default tests use local fixtures and do not download models or call live map
services. Examples marked `no_run` are compiled without loading models, contacting
services, or writing output. Live inference/service/sample-recording tests are
ignored unless selected explicitly:

```sh
cargo test -p image_flow_lib --lib -- --ignored --test-threads=1
```

Those integration tests require their sample assets and compatible models and
may write to `out/`. The pipeline and integration-test helpers read
`USER_AGENT_NAME`, `PASSWORD`, `OVERPASS_URL`, and `NOMINATIM_URL` at **build time**;
library callers can supply `AuthToHost` directly at runtime.
