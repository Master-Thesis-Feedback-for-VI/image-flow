use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{LazyLock, OnceLock},
};

pub struct CSVVideoCombi {
    archive_path: PathBuf,
    video_entry: String,
    pub csv_embed: String,
    video: OnceLock<(tempfile::TempDir, PathBuf)>,
}

impl CSVVideoCombi {
    pub fn video_path(&self) -> &str {
        let (_, path) = self
            .video
            .get_or_init(|| {
                let directory = tempfile::tempdir().expect("create sample video directory");
                let path = directory
                    .path()
                    .join("video.mov");
                let mut archive = zip::ZipArchive::new(
                    File::open(&self.archive_path).expect("open sample archive"),
                )
                .expect("read sample archive");
                let mut video = archive
                    .by_name(&self.video_entry)
                    .expect("find sample video");
                let mut output = File::create(&path).expect("create sample video");
                io::copy(&mut video, &mut output).expect("extract sample video");
                (directory, path)
            });
        path.to_str()
            .expect("sample video path is UTF-8")
    }
}

pub static SAMPLE_VIDEO_SOURCES: LazyLock<Vec<CSVVideoCombi>> = LazyLock::new(|| {
    let sample_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sample_data");
    let mut archives: Vec<_> = fs::read_dir(sample_dir)
        .expect("read sample directory")
        .map(|entry| {
            entry
                .expect("read sample entry")
                .path()
        })
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "zip")
        })
        .collect();
    archives.sort();
    assert!(!archives.is_empty(), "no sample ZIP archives found");

    archives
        .into_iter()
        .map(|archive_path| {
            let mut archive =
                zip::ZipArchive::new(File::open(&archive_path).expect("open sample archive"))
                    .expect("read sample archive");
            let videos: Vec<_> = archive
                .file_names()
                .filter(|name| {
                    Path::new(name)
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("mov"))
                })
                .map(str::to_owned)
                .collect();
            assert_eq!(videos.len(), 1, "expected one video in {archive_path:?}");
            let video_entry = videos
                .into_iter()
                .next()
                .expect("sample video entry");
            let csv_entry = PathBuf::from(&video_entry).with_file_name("gps_timestamps.csv");
            let mut csv_embed = String::new();
            archive
                .by_name(
                    csv_entry
                        .to_str()
                        .expect("sample CSV path is UTF-8"),
                )
                .expect("find matching sample GPS CSV")
                .read_to_string(&mut csv_embed)
                .expect("read sample GPS CSV");

            CSVVideoCombi {
                archive_path,
                video_entry,
                csv_embed,
                video: OnceLock::new(),
            }
        })
        .collect()
});

#[test]
fn check_files_exists() {
    for src in SAMPLE_VIDEO_SOURCES.iter() {
        assert!(
            src.archive_path
                .is_file()
        );
        assert!(
            !src.csv_embed
                .is_empty(),
            "empty GPS CSV in {:?}",
            src.archive_path
        );
    }
}
