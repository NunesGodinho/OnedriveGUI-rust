//! The list of recent uploads, downloads and deletions shown under a profile's status
//! (`event_update_progress` and `TaskList` in the Python app).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::parser::Transfer;
use crate::utils::{humanize_file_size, shorten_path};

/// The list keeps at most this many entries (issue #208 of the Python app).
pub const MAX_ITEMS: usize = 10_000;
/// An update replaces an unfinished entry for the same file among the newest this many.
const MATCH_WINDOW: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Upload,
    Download,
    Delete,
    /// Deleted online and moved to the local recycle bin.
    Trash,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransferItem {
    pub file_name: String,
    pub kind: Kind,
    /// The operation word the client used (`Uploading`, …) for unfinished entries.
    pub operation: String,
    /// 0–100; the progress bar shows until `complete`.
    pub progress: u8,
    pub complete: bool,
    pub completed_at: Option<SystemTime>,
    /// First line: what happened (`Uploaded from`, `Deleted from Documents`, `Downloading`).
    pub headline: String,
    /// The folder linked after the headline: (shortened name, absolute folder).
    pub folder_link: Option<(String, PathBuf)>,
    /// Second line: sizes (`1.0MiB of 4.0MiB`).
    pub detail: String,
}

/// A file's size, or its `.partial` download's while it is still empty.
fn file_size(path: &Path) -> u64 {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if size > 0 {
        return size;
    }
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    std::fs::metadata(PathBuf::from(partial)).map(|m| m.len()).unwrap_or(0)
}

impl TransferItem {
    /// Build the list entry for `t`, a path relative to `sync_dir`. Reads the file's size.
    pub fn new(t: &Transfer, sync_dir: &Path) -> TransferItem {
        let path = sync_dir.join(&t.file_path);
        TransferItem::with_size(t, sync_dir, file_size(&path))
    }

    fn with_size(t: &Transfer, sync_dir: &Path, size: u64) -> TransferItem {
        let path = sync_dir.join(&t.file_path);
        let file_name = path.file_name().map_or_else(|| t.file_path.clone(), |n| n.to_string_lossy().into_owned());
        let folder = path.parent().map_or_else(|| sync_dir.to_path_buf(), Path::to_path_buf);
        let parent_name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| folder.display().to_string());
        let relative = folder.strip_prefix(sync_dir).map(|p| p.display().to_string()).unwrap_or_default();
        let folder_display = if relative.is_empty() { parent_name.clone() } else { shorten_path(&relative, 32) };
        let kind = match t.operation.as_str() {
            "Uploading" => Kind::Upload,
            "Downloading" => Kind::Download,
            "Deleting" => Kind::Delete,
            "Moving" => Kind::Trash,
            _ => Kind::Other,
        };
        let size_f = size as f64;
        let (headline, folder_link, detail) = match kind {
            Kind::Delete => (format!("Deleted from {parent_name}"), None, String::new()),
            Kind::Trash => (format!("Trashed from {parent_name}"), None, String::new()),
            Kind::Upload if t.complete => ("Uploaded from".into(), Some((folder_display, folder)), humanize_file_size(size_f)),
            Kind::Download if t.complete => ("Downloaded from".into(), Some((folder_display, folder)), humanize_file_size(size_f)),
            Kind::Download => {
                // The client reports download progress about 5% behind.
                let estimate = size_f / (f64::from(t.progress) + 5.0) * 100.0;
                (t.operation.clone(), None, format!("{} of ~{}", humanize_file_size(size_f), humanize_file_size(estimate)))
            }
            _ => {
                let done = size_f / 100.0 * f64::from(t.progress);
                (t.operation.clone(), None, format!("{} of {}", humanize_file_size(done), humanize_file_size(size_f)))
            }
        };
        TransferItem {
            file_name,
            kind,
            operation: t.operation.clone(),
            progress: t.progress.min(100),
            complete: t.complete,
            completed_at: if t.complete { t.timestamp } else { None },
            headline,
            folder_link,
            detail,
        }
    }
}

/// Newest first.
#[derive(Debug, Default)]
pub struct TransferList {
    pub items: VecDeque<TransferItem>,
}

impl TransferList {
    /// Update the unfinished entry for the same file, or add a new entry at the top.
    pub fn apply(&mut self, item: TransferItem) {
        let existing = self.items.iter_mut().take(MATCH_WINDOW).find(|i| i.file_name == item.file_name && i.completed_at.is_none());
        match existing {
            Some(slot) => *slot = item,
            None => {
                self.items.push_front(item);
                self.items.truncate(MAX_ITEMS);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transfer(op: &str, path: &str, progress: u8, complete: bool) -> Transfer {
        Transfer {
            operation: op.into(),
            file_path: path.into(),
            progress,
            complete,
            timestamp: complete.then(SystemTime::now),
        }
    }

    #[test]
    fn entries_read_like_the_python_app() {
        let sync = Path::new("/home/u/OneDrive");
        let up = TransferItem::with_size(&transfer("Uploading", "Docs/Work/a.pdf", 0, true), sync, 2048);
        assert_eq!(up.headline, "Uploaded from");
        assert_eq!(up.folder_link, Some(("Docs/Work".into(), PathBuf::from("/home/u/OneDrive/Docs/Work"))));
        assert_eq!(up.detail, "2.0KiB");
        assert!(up.completed_at.is_some());

        let root = TransferItem::with_size(&transfer("Downloading", "b.txt", 0, true), sync, 10);
        assert_eq!(root.folder_link.map(|l| l.0), Some("OneDrive".to_string()));

        let del = TransferItem::with_size(&transfer("Deleting", "Old/c.txt", 0, true), sync, 0);
        assert_eq!((del.headline.as_str(), del.kind), ("Deleted from Old", Kind::Delete));

        let partial = TransferItem::with_size(&transfer("Downloading", "big.iso", 45, false), sync, 1024 * 1024);
        assert_eq!(partial.detail, "1.0MiB of ~2.0MiB");
        assert!(partial.completed_at.is_none());

        let uploading = TransferItem::with_size(&transfer("Uploading", "big.iso", 50, false), sync, 4096);
        assert_eq!(uploading.detail, "2.0KiB of 4.0KiB");
    }

    #[test]
    fn progress_updates_replace_the_unfinished_entry() {
        let sync = Path::new("/s");
        let mut list = TransferList::default();
        list.apply(TransferItem::with_size(&transfer("Uploading", "a.bin", 10, false), sync, 100));
        list.apply(TransferItem::with_size(&transfer("Uploading", "other.bin", 10, false), sync, 100));
        list.apply(TransferItem::with_size(&transfer("Uploading", "a.bin", 60, false), sync, 100));
        assert_eq!(list.items.len(), 2);
        list.apply(TransferItem::with_size(&transfer("Uploading", "a.bin", 0, true), sync, 100));
        assert_eq!(list.items.len(), 2);
        // A finished entry isn't reused: the next transfer of the same file is a new entry.
        list.apply(TransferItem::with_size(&transfer("Uploading", "a.bin", 5, false), sync, 100));
        assert_eq!(list.items.len(), 3);
        assert_eq!(list.items.front().map(|i| i.progress), Some(5));
    }

    #[test]
    fn the_list_is_capped() {
        let mut list = TransferList::default();
        for i in 0..MAX_ITEMS + 5 {
            list.apply(TransferItem::with_size(&transfer("Deleting", &format!("f{i}"), 0, true), Path::new("/s"), 0));
        }
        assert_eq!(list.items.len(), MAX_ITEMS);
    }
}
