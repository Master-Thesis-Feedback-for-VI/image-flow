use std::path::Path;

pub const SAMPLE_VIDEO_SOURCES: &[&str] = &[
    "../sample_data/recording-2026-08-12-13-24-46/6FC95631-8DE8-43F1-AEB4-DC4B6AF6CAC5.mov",
    "../sample_data/recording-2026-08-12-13-41-36/E510E1EE-D294-4EAF-8652-B95B9D72AC21.mov",
];

#[test]
fn check_files_exists() {
    for &src in SAMPLE_VIDEO_SOURCES {
        assert!(
            Path::new(src).exists(),
            "missing sample video fixture: {src}",
        );
    }
}
