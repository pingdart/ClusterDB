use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs;

#[allow(async_fn_in_trait)]
pub trait ObjectStore: Send + Sync {
    async fn put_object(&self, key: &str, data: &[u8]) -> Result<()>;
    async fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>>;
    async fn list_objects(&self, prefix: &str) -> Result<Vec<String>>;
    async fn delete_object(&self, key: &str) -> Result<()>;
}

/// Local Filesystem Object Store Emulator for testing and on-premise zero-cloud setups
pub struct LocalFsStore {
    root_dir: PathBuf,
}

impl LocalFsStore {
    pub fn new<P: AsRef<Path>>(root: P) -> Result<Self> {
        let root_dir = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root_dir).context("Failed to create object store root directory")?;
        Ok(Self { root_dir })
    }

    fn resolve_path(&self, key: &str) -> PathBuf {
        let safe_key = key.trim_start_matches('/').replace("..", "_");
        self.root_dir.join(safe_key)
    }
}

impl ObjectStore for LocalFsStore {
    async fn put_object(&self, key: &str, data: &[u8]) -> Result<()> {
        let path = self.resolve_path(key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, data).await?;
        Ok(())
    }

    async fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.resolve_path(key);
        if path.exists() {
            let data = fs::read(path).await?;
            Ok(Some(data))
        } else {
            Ok(None)
        }
    }

    async fn list_objects(&self, prefix: &str) -> Result<Vec<String>> {
        let mut results = Vec::new();
        let mut stack = vec![self.root_dir.clone()];

        while let Some(dir) = stack.pop() {
            if let Ok(mut entries) = fs::read_dir(dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if let Ok(rel) = path.strip_prefix(&self.root_dir) {
                        let key = rel.to_string_lossy().replace('\\', "/");
                        let norm_prefix = prefix.replace('\\', "/");
                        if key.starts_with(&norm_prefix) {
                            results.push(key);
                        }
                    }
                }
            }
        }
        results.sort();
        Ok(results)
    }

    async fn delete_object(&self, key: &str) -> Result<()> {
        let path = self.resolve_path(key);
        if path.exists() {
            fs::remove_file(path).await?;
        }
        Ok(())
    }
}

/// S3 / MinIO REST-Compatible Client
pub struct S3CompatibleStore {
    endpoint: String,
    bucket: String,
    http: reqwest::Client,
}

impl S3CompatibleStore {
    pub fn new(endpoint: &str, bucket: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            bucket: bucket.to_string(),
            http: reqwest::Client::new(),
        }
    }
}

impl ObjectStore for S3CompatibleStore {
    async fn put_object(&self, key: &str, data: &[u8]) -> Result<()> {
        let url = format!("{}/{}/{}", self.endpoint, self.bucket, key.trim_start_matches('/'));
        let res = self.http.put(&url).body(data.to_vec()).send().await?;
        if res.status().is_success() {
            Ok(())
        } else {
            anyhow::bail!("S3 PutObject failed: {}", res.status())
        }
    }

    async fn get_object(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let url = format!("{}/{}/{}", self.endpoint, self.bucket, key.trim_start_matches('/'));
        let res = self.http.get(&url).send().await?;
        if res.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(None)
        } else if res.status().is_success() {
            let bytes = res.bytes().await?;
            Ok(Some(bytes.to_vec()))
        } else {
            anyhow::bail!("S3 GetObject failed: {}", res.status())
        }
    }

    async fn list_objects(&self, _prefix: &str) -> Result<Vec<String>> {
        // Fallback or basic bucket query
        Ok(Vec::new())
    }

    async fn delete_object(&self, key: &str) -> Result<()> {
        let url = format!("{}/{}/{}", self.endpoint, self.bucket, key.trim_start_matches('/'));
        let res = self.http.delete(&url).send().await?;
        if res.status().is_success() || res.status() == reqwest::StatusCode::NOT_FOUND {
            Ok(())
        } else {
            anyhow::bail!("S3 DeleteObject failed: {}", res.status())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_fs_store() {
        let test_dir = "test_obj_store_dir";
        let store = LocalFsStore::new(test_dir).unwrap();

        let data = b"Hello, PingDart Object Storage!";
        store.put_object("snapshots/snap_1.bin", data).await.unwrap();

        let retrieved = store.get_object("snapshots/snap_1.bin").await.unwrap();
        assert_eq!(retrieved, Some(data.to_vec()));

        let list = store.list_objects("snapshots/").await.unwrap();
        assert_eq!(list, vec!["snapshots/snap_1.bin"]);

        store.delete_object("snapshots/snap_1.bin").await.unwrap();
        assert_eq!(store.get_object("snapshots/snap_1.bin").await.unwrap(), None);

        let _ = std::fs::remove_dir_all(test_dir);
    }
}
