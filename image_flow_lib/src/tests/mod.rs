use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use tokio::sync::Mutex;

pub mod location_data;
pub mod video_data;

static TESTING_FILE_MANAGER: Mutex<TestingFileManger> = Mutex::const_new(TestingFileManger::new());

const TEST_REMOVE_FILES_AFTER_RERUN: bool = match option_env!("TEST_REMOVE_FILES_AFTER_RERUN") {
    Some("false" | "FALSE" | "no" | "NO") => false,
    Some(_) | None => true,
};

pub struct TestingFileManger {
    out_dir: Option<PathBuf>,
}

impl TestingFileManger {
    const fn new() -> Self {
        Self { out_dir: None }
    }

    pub fn out_dir(&mut self) -> &Path {
        self.out_dir
            .get_or_insert_with(|| {
                let output = Command::new("cargo")
                    .args(["locate-project", "--workspace", "--message-format", "plain"])
                    .output()
                    .unwrap();

                assert!(
                    output
                        .status
                        .success()
                );

                let manifest = PathBuf::from(
                    String::from_utf8(output.stdout)
                        .unwrap()
                        .trim(),
                )
                .parent()
                .unwrap()
                .to_owned()
                .join("out");

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
