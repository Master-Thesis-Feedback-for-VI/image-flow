use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tokio::sync::Mutex;

pub mod location_data;
pub mod video_data;

pub static TESTING_FILE_MANAGER: Mutex<TestingFileManger> =
    Mutex::const_new(TestingFileManger::new());

const TEST_REMOVE_FILES_AFTER_RERUN: bool = match option_env!("TEST_REMOVE_FILES_AFTER_RERUN") {
    Some("false" | "FALSE" | "no" | "NO") => false,
    Some(_) | None => true,
};

pub struct TestingFileManger {
    out_dir: Option<PathBuf>,
    sample_dir: Option<PathBuf>,

    all_images: Option<Vec<PathBuf>>,
}

impl TestingFileManger {
    const fn new() -> Self {
        Self {
            out_dir: None,
            sample_dir: None,
            all_images: None,
        }
    }

    pub fn out_dir(&mut self) -> &Path {
        self.out_dir
            .get_or_insert_with(|| {
                let manifest = Self::cargo_root().join("out");

                if manifest.exists() && TEST_REMOVE_FILES_AFTER_RERUN {
                    fs::remove_dir_all(&manifest).unwrap();
                }
                fs::create_dir_all(&manifest).unwrap();

                manifest
            })
    }

    pub fn test_out(&mut self, path: impl AsRef<Path>) -> PathBuf {
        let p = self.out_dir();
        let dir = p.join(path);

        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();

        dir
    }

    pub fn all_images(&mut self) -> &[PathBuf] {
        let image_dir = self
            .image_dir()
            .to_path_buf();

        self.all_images
            .get_or_insert_with(move || {
                let entries = fs::read_dir(image_dir).unwrap();
                let mut files: Vec<PathBuf> = entries
                    .map(|val| val.unwrap())
                    .filter(|pred| {
                        let is_file = pred
                            .file_type()
                            .unwrap()
                            .is_file();

                        let valid_ext = pred
                            .path()
                            .extension()
                            .map(|val| val == "png" || val == "jpg")
                            .unwrap();

                        is_file && valid_ext
                    })
                    .map(|val| val.path())
                    .collect();

                files.sort();
                files
            })
    }

    pub fn image_dir(&mut self) -> &Path {
        self.sample_dir
            .get_or_insert_with(|| {
                let img_path = Self::cargo_root()
                    .join("sample_data")
                    .join("img");
                assert!(img_path.exists());

                img_path
            })
    }

    fn cargo_root() -> PathBuf {
        let output = Command::new("cargo")
            .args(["locate-project", "--workspace", "--message-format", "plain"])
            .output()
            .unwrap();

        assert!(
            output
                .status
                .success()
        );

        PathBuf::from(
            String::from_utf8(output.stdout)
                .unwrap()
                .trim(),
        )
        .parent()
        .unwrap()
        .to_owned()
    }
}

#[allow(clippy::significant_drop_tightening)]
#[tokio::test]
async fn instantiate_file_manager() {
    let mut lock = TESTING_FILE_MANAGER
        .lock()
        .await;
    let fm = &mut *lock;

    fm.out_dir();
}

const AMOUNT_OF_IMAGES: usize = 9;

#[allow(clippy::significant_drop_tightening)]
#[tokio::test]
async fn correct_amount_of_files() {
    let mut binding = TESTING_FILE_MANAGER
        .lock()
        .await;
    let image_files = binding.all_images();

    assert_eq!(image_files.len(), AMOUNT_OF_IMAGES);
}
