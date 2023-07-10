use std::fs::File;
use std::time::Duration;

use anyhow::Result;
use tempfile::{tempdir, TempDir};

use super::FileDatabase;

fn generate_test_dir() -> Result<TempDir> {
    let tempdir = tempdir()?;
    let subdir = tempdir.path().join("TestFolder");
    std::fs::create_dir(&subdir)?;
    for i in 0..100 {
        File::create(tempdir.path().join(format!("File_{i}")))?;
        File::create(subdir.join(format!("SubFile_{i}")))?;
    }
    Ok(tempdir)
}

#[tokio::test]
async fn find_all_files() -> Result<()> {
    let dir = generate_test_dir()?;
    let db = FileDatabase::new(&[dir.path().to_path_buf()]);
    db.sender().update().await;

    while db.sender().is_updating() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(db.sender().data.database.len(), 200);

    Ok(())
}
