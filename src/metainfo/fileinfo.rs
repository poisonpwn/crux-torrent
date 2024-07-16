use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct FileInfo {
    pub path: Vec<String>,
    pub length: usize,

    #[serde(default)]
    pub md5sum: Option<String>,
}
