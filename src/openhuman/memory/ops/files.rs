//! File-based memory RPC handlers (`ai_list_memory_files`,
//! `ai_read_memory_file`, `ai_write_memory_file`).
//!
//! All filesystem I/O here is performed via `tokio::fs` so the handlers stay
//! async-friendly and never block the executor.

use crate::openhuman::memory::{
    ApiEnvelope, ListMemoryFilesRequest, ListMemoryFilesResponse, ReadMemoryFileRequest,
    ReadMemoryFileResponse, WriteMemoryFileRequest, WriteMemoryFileResponse,
};
use crate::rpc::RpcOutcome;

use super::envelope::{envelope, memory_counts};
use super::helpers::{
    resolve_existing_memory_path, resolve_writable_memory_path, validate_memory_relative_path,
};

/// Lists files in a memory directory.
pub async fn ai_list_memory_files(
    request: ListMemoryFilesRequest,
) -> Result<RpcOutcome<ApiEnvelope<ListMemoryFilesResponse>>, String> {
    validate_memory_relative_path(&request.relative_dir)?;
    let directory = resolve_existing_memory_path(&request.relative_dir).await?;
    if !directory.is_dir() {
        return Err(format!(
            "memory directory not found: {}",
            directory.display()
        ));
    }
    let mut files = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&directory)
        .await
        .map_err(|e| format!("read memory directory {}: {e}", directory.display()))?;
    while let Some(entry) = read_dir
        .next_entry()
        .await
        .map_err(|e| format!("read memory directory entry: {e}"))?
    {
        // Skip subdirectories and symlinks — `ai_read_memory_file` only
        // consumes regular file entries, and surfacing other entry kinds
        // here would just produce confusing follow-up read errors.
        let file_type = entry
            .file_type()
            .await
            .map_err(|e| format!("read memory directory entry type: {e}"))?;
        if !file_type.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        if !file_name.is_empty() {
            files.push(file_name.to_string());
        }
    }
    files.sort();
    let count = files.len();
    Ok(envelope(
        ListMemoryFilesResponse {
            relative_dir: request.relative_dir,
            files,
            count,
        },
        Some(memory_counts([("num_files", count)])),
        None,
    ))
}

/// Reads the contents of a memory file.
pub async fn ai_read_memory_file(
    request: ReadMemoryFileRequest,
) -> Result<RpcOutcome<ApiEnvelope<ReadMemoryFileResponse>>, String> {
    let path = resolve_existing_memory_path(&request.relative_path).await?;
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("read memory file {}: {e}", path.display()))?;
    Ok(envelope(
        ReadMemoryFileResponse {
            relative_path: request.relative_path,
            content,
        },
        None,
        None,
    ))
}

/// Writes content to a memory file.
pub async fn ai_write_memory_file(
    request: WriteMemoryFileRequest,
) -> Result<RpcOutcome<ApiEnvelope<WriteMemoryFileResponse>>, String> {
    let path = resolve_writable_memory_path(&request.relative_path).await?;
    tokio::fs::write(&path, request.content.as_bytes())
        .await
        .map_err(|e| format!("write memory file {}: {e}", path.display()))?;
    let bytes_written = request.content.len();
    Ok(envelope(
        WriteMemoryFileResponse {
            relative_path: request.relative_path,
            written: true,
            bytes_written,
        },
        None,
        None,
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use tempfile::TempDir;

    use super::*;

    fn env_mutex() -> &'static Mutex<()> {
        static ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_MUTEX.get_or_init(|| Mutex::new(()))
    }

    fn set_workspace_env(path: &std::path::Path) {
        unsafe {
            std::env::set_var("OPENHUMAN_WORKSPACE", path);
        }
    }

    fn clear_workspace_env() {
        unsafe {
            std::env::remove_var("OPENHUMAN_WORKSPACE");
        }
    }

    #[tokio::test]
    async fn write_read_and_list_memory_files_roundtrip() {
        let _guard = env_mutex().lock().expect("lock env mutex");
        let tmp = TempDir::new().expect("tempdir");
        set_workspace_env(tmp.path());

        let write = ai_write_memory_file(WriteMemoryFileRequest {
            relative_path: "notes/today.md".to_string(),
            content: "remember this".to_string(),
        })
        .await
        .expect("write should succeed");
        let write_data = write.value.data.expect("write data");
        assert_eq!(write_data.relative_path, "notes/today.md");
        assert!(write_data.written);
        assert_eq!(write_data.bytes_written, "remember this".len());

        let read = ai_read_memory_file(ReadMemoryFileRequest {
            relative_path: "notes/today.md".to_string(),
        })
        .await
        .expect("read should succeed");
        let read_data = read.value.data.expect("read data");
        assert_eq!(read_data.relative_path, "notes/today.md");
        assert_eq!(read_data.content, "remember this");

        let memory_root = tmp.path().join("memory");
        tokio::fs::write(memory_root.join("b.md"), "b")
            .await
            .expect("write b");
        tokio::fs::write(memory_root.join("a.md"), "a")
            .await
            .expect("write a");
        tokio::fs::create_dir_all(memory_root.join("nested"))
            .await
            .expect("create nested dir");
        tokio::fs::write(memory_root.join("nested").join("hidden.md"), "hidden")
            .await
            .expect("write nested file");

        let listed = ai_list_memory_files(ListMemoryFilesRequest {
            relative_dir: String::new(),
        })
        .await
        .expect("list should succeed");
        let listed_data = listed.value.data.expect("list data");
        assert_eq!(listed_data.relative_dir, "");
        assert_eq!(listed_data.files, vec!["a.md", "b.md"]);
        assert_eq!(listed_data.count, 2);

        clear_workspace_env();
    }

    #[tokio::test]
    async fn list_memory_files_rejects_non_directory_target() {
        let _guard = env_mutex().lock().expect("lock env mutex");
        let tmp = TempDir::new().expect("tempdir");
        set_workspace_env(tmp.path());

        tokio::fs::create_dir_all(tmp.path().join("memory"))
            .await
            .expect("create memory root");
        tokio::fs::write(tmp.path().join("memory").join("single.md"), "hello")
            .await
            .expect("write file");

        let err = ai_list_memory_files(ListMemoryFilesRequest {
            relative_dir: "single.md".to_string(),
        })
        .await
        .expect_err("listing a file path should fail");
        assert!(err.contains("memory directory not found"));

        clear_workspace_env();
    }

    #[tokio::test]
    async fn read_and_write_memory_files_reject_path_traversal() {
        let _guard = env_mutex().lock().expect("lock env mutex");
        let tmp = TempDir::new().expect("tempdir");
        set_workspace_env(tmp.path());

        let write_err = ai_write_memory_file(WriteMemoryFileRequest {
            relative_path: "../secrets.txt".to_string(),
            content: "nope".to_string(),
        })
        .await
        .expect_err("path traversal should fail for writes");
        assert!(write_err.contains("path traversal is not allowed"));

        let read_err = ai_read_memory_file(ReadMemoryFileRequest {
            relative_path: "../secrets.txt".to_string(),
        })
        .await
        .expect_err("path traversal should fail for reads");
        assert!(read_err.contains("path traversal is not allowed"));

        clear_workspace_env();
    }
}
