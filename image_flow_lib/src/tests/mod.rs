use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tokio::sync::Mutex;

use crate::context::ContextCategory;

pub mod location_data;
pub mod video_data;

pub static TESTING_FILE_MANAGER: Mutex<TestingFileManger> =
    Mutex::const_new(TestingFileManger::new());

const TEST_REMOVE_FILES_AFTER_RERUN: bool = match option_env!("TEST_REMOVE_FILES_AFTER_RERUN") {
    Some("false" | "FALSE" | "no" | "NO") => false,
    Some(_) | None => true,
};

#[derive(Clone, Debug)]
pub struct TestingImage {
    pub img_path: PathBuf,
    pub categories: Vec<ContextCategory>,
}

pub struct TestingFileManger {
    out_dir: Option<PathBuf>,
    sample_dir: Option<PathBuf>,

    all_images: Option<Vec<TestingImage>>,
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

    pub fn all_images(&mut self) -> Vec<TestingImage> {
        let image_dir = self
            .image_dir()
            .to_path_buf();

        let images = self
            .all_images
            .get_or_insert_with(move || {
                let folders = [
                    (image_dir.join("outside"), ContextCategory::Outside),
                    (image_dir.join("forest"), ContextCategory::Forest),
                ];

                folders
                    .iter()
                    .flat_map(|(dir_path, ctx_cat)| {
                        let entries = fs::read_dir(dir_path).unwrap();
                        let mut files: Vec<TestingImage> = entries
                            .map(|val| val.unwrap())
                            .filter(|pred| {
                                let is_file = pred
                                    .file_type()
                                    .unwrap()
                                    .is_file();

                                let valid_ext = pred
                                    .path()
                                    .extension()
                                    .is_some_and(|val| val == "png" || val == "jpg");

                                is_file && valid_ext
                            })
                            .map(|val| TestingImage {
                                img_path: val.path(),
                                categories: ctx_cat
                                    .implied_additional_categories()
                                    .to_vec(),
                            })
                            .collect();

                        files.sort_by(|a, b| {
                            a.img_path
                                .cmp(&b.img_path)
                        });
                        files
                    })
                    .collect()
            });
        images.clone()
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

const AMOUNT_OF_IMAGES: usize = 8;

#[allow(clippy::significant_drop_tightening)]
#[tokio::test]
async fn instantiate_file_manager() {
    let mut lock = TESTING_FILE_MANAGER
        .lock()
        .await;
    let fm = &mut *lock;

    fm.out_dir();
}

#[allow(clippy::significant_drop_tightening)]
#[tokio::test]
async fn correct_amount_of_files() {
    let mut binding = TESTING_FILE_MANAGER
        .lock()
        .await;
    let image_files = binding.all_images();

    assert_eq!(
        image_files.len(),
        AMOUNT_OF_IMAGES,
        "These files where found: {image_files:?}"
    );
}
