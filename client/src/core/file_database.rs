use std::path::{Path, PathBuf};

use async_trait::async_trait;

#[async_trait]
pub trait FileDatabaseTrait {
    fn add_path(&mut self, path: PathBuf);
    fn del_path(&mut self, path: &Path);
    fn clear_paths(&mut self);
    fn get_paths(&self) -> Vec<PathBuf>;
    fn start_update(&mut self);
    fn stop_update(&mut self);
    fn update_status(&self) -> f32;
    fn find_file(&self, filename: &str) -> Option<PathBuf>;
    fn all_files(&self) -> Vec<PathBuf>;
    async fn update_completed(&mut self);
}
