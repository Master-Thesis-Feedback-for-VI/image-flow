use std::path::Path;

pub struct CSVVideoCombi {
    pub video_path: &'static str,
    pub csv_embed: &'static str,
}

pub const SAMPLE_VIDEO_SOURCES: &[CSVVideoCombi] = &[
    CSVVideoCombi {
        video_path: "../sample_data/recording-2026-08-12-13-24-46/6FC95631-8DE8-43F1-AEB4-DC4B6AF6CAC5.mov",
        csv_embed: include_str!(
            "../../../sample_data/recording-2026-08-12-13-41-36/gps_timestamps.csv"
        ),
    },
    CSVVideoCombi {
        video_path: "../sample_data/recording-2026-08-12-13-41-36/E510E1EE-D294-4EAF-8652-B95B9D72AC21.mov",
        csv_embed: include_str!(
            "../../../sample_data/recording-2026-08-12-13-41-36/gps_timestamps.csv"
        ),
    },
];

#[test]
fn check_files_exists() {
    for src in SAMPLE_VIDEO_SOURCES {
        let video_path = src.video_path;
        assert!(
            Path::new(video_path).exists(),
            "missing sample video fixture: {video_path}",
        );
    }
}
