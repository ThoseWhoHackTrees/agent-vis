use crossbeam_channel::{unbounded, Receiver};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum FileSystemEvent {
    Created(PathBuf, bool),  // path, is_dir
    Deleted(PathBuf),
    Modified(PathBuf),
}

pub struct FileWatcherHandle {
    _watcher: notify::RecommendedWatcher,
}

pub fn start_file_watcher(_watch_path: PathBuf) -> (Receiver<FileSystemEvent>, FileWatcherHandle) {
    let (tx, rx) = unbounded::<FileSystemEvent>();

    let watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        match res {
            Ok(event) => {
                match event.kind {
                    EventKind::Create(_) => {
                        for path in event.paths {
                            let is_dir = path.is_dir();
                            let _ = tx.send(FileSystemEvent::Created(path, is_dir));
                        }
                    }
                    EventKind::Remove(_) => {
                        for path in event.paths {
                            let _ = tx.send(FileSystemEvent::Deleted(path));
                        }
                    }
                    EventKind::Modify(_) => {
                        for path in event.paths {
                            let _ = tx.send(FileSystemEvent::Modified(path));
                        }
                    }
                    _ => {}
                }
            }
            Err(e) => eprintln!("Watch error: {:?}", e),
        }
    })
    .expect("Failed to create file watcher");

    let handle = FileWatcherHandle { _watcher: watcher };

    (rx, handle)
}

pub fn watch_directory(
    mut watcher: FileWatcherHandle,
    watch_path: PathBuf,
) -> FileWatcherHandle {
    watcher
        ._watcher
        .watch(&watch_path, RecursiveMode::Recursive)
        .expect("Failed to watch directory");
    watcher
}
