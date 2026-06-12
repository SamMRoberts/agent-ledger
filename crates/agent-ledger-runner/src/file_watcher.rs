use std::{path::Path, sync::mpsc::Sender};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

pub struct FileWatcher {
    pub tx: Sender<notify::Event>,
    watcher: RecommendedWatcher,
}

impl FileWatcher {
    pub fn new(watch_dir: &Path, event_sender: Sender<notify::Event>) -> anyhow::Result<Self> {
        let tx = event_sender.clone();
        let mut watcher =
            notify::recommended_watcher(move |result: Result<notify::Event, notify::Error>| {
                if let Ok(event) = result {
                    let _ = event_sender.send(event);
                }
            })?;
        watcher.watch(watch_dir, RecursiveMode::Recursive)?;
        Ok(Self { tx, watcher })
    }

    pub fn stop(self) {
        let _ = self.watcher;
    }
}
