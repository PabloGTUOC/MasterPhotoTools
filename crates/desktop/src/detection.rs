//! Card detection (F10).
//!
//! > Watch `/Volumes` for newly mounted filesystems. Debounce, then test for a
//! > `DCIM` directory. On a match, raise a native notification.
//!
//! This is a platform integration concern and lives in the binary for that
//! reason. It is deliberately thin: **it produces a path and hands it to
//! `core`** (build plan §6.3). Everything it triggers — deciding what a card is,
//! counting its shots, recognising one already seen — is `core`'s, and is
//! testable without a card reader. What is here is the watcher, the debounce and
//! the notification.

use phototools_core::error::Error;
use phototools_core::ingest::{summarise_card, Card, CardSummary};
use phototools_core::ledger::Ledger;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Where macOS mounts removable volumes.
pub const MOUNT_ROOT: &str = "/Volumes";

/// How long to wait after a mount event before looking at the volume.
///
/// A mount is not one event. The kernel publishes the mount point, then the
/// filesystem driver populates it, and a card reader can emit several events a
/// few milliseconds apart. Looking immediately finds an empty directory and
/// concludes there is no `DCIM`.
pub const DEBOUNCE: Duration = Duration::from_millis(1500);

/// What detection tells the rest of the application.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CardDetected {
    pub path: PathBuf,
    pub card_id: String,
    pub label: Option<String>,
    pub shots: usize,
    pub new_shots: usize,
    pub seen_before: bool,
}

impl CardDetected {
    /// F10's notification text: `EOS_DIGITAL — 412 new shots. Review?`
    pub fn notification(&self) -> String {
        let label = self.label.as_deref().unwrap_or("Card");
        let n = self.new_shots;
        let shots = if n == 1 { "shot" } else { "shots" };
        format!("{label} — {n} new {shots}. Review?")
    }
}

/// Collapses a burst of mount events into one look at the volume.
///
/// Separated from the watcher so the timing rule is testable without mounting
/// anything: the watcher supplies events and a clock, this decides when a path
/// has settled.
pub struct Debounce {
    window: Duration,
    pending: HashMap<PathBuf, Instant>,
}

impl Debounce {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            pending: HashMap::new(),
        }
    }

    /// Record an event for `path`. Restarts the window if one is already open.
    pub fn touch(&mut self, path: impl Into<PathBuf>, now: Instant) {
        self.pending.insert(path.into(), now);
    }

    /// Paths whose window has closed, removed from the pending set.
    pub fn ready(&mut self, now: Instant) -> Vec<PathBuf> {
        let due: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, seen)| now.duration_since(**seen) >= self.window)
            .map(|(path, _)| path.clone())
            .collect();

        for path in &due {
            self.pending.remove(path);
        }

        // Stable order so a burst of mounts is reported predictably.
        let mut due = due;
        due.sort();
        due
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Decide whether a settled path is a card worth announcing (F10).
///
/// Returns `None` for anything that is not a card, and for a card that has
/// nothing new on it — a reinserted card must not announce the same 400 shots
/// again, which is what recognising cards is for.
pub fn inspect(path: &Path, ledger: &Ledger) -> Result<Option<CardDetected>, Error> {
    let card = match Card::at(path) {
        Ok(card) => card,
        // A volume that vanished between the event and this look is not an
        // error; someone pulled it out.
        Err(_) => return Ok(None),
    };

    if !card.looks_like_a_card() {
        return Ok(None);
    }

    let summary: CardSummary = summarise_card(&card, ledger)?;
    if !summary.worth_announcing() {
        return Ok(None);
    }

    Ok(Some(CardDetected {
        path: card.root().to_path_buf(),
        card_id: summary.card_id,
        label: summary.label,
        shots: summary.shots,
        new_shots: summary.new_shots,
        seen_before: summary.seen_before,
    }))
}

/// Watches a directory for newly mounted volumes.
///
/// Held by the application for as long as it runs. Dropping it stops the watch.
pub struct VolumeWatcher {
    _watcher: notify::RecommendedWatcher,
    /// Kept so the polling loop can be stopped when the watcher is dropped.
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for VolumeWatcher {
    fn drop(&mut self) {
        self.running
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

impl VolumeWatcher {
    /// Start watching `mount_root`, calling `on_card` for each card detected.
    ///
    /// Non-blocking: the watch and the debounce timer run on their own threads,
    /// because F10 must not make the application wait on a card reader.
    pub fn start<F>(
        mount_root: impl AsRef<Path>,
        ledger: Arc<Mutex<Ledger>>,
        on_card: F,
    ) -> Result<Self, Error>
    where
        F: Fn(CardDetected) + Send + 'static,
    {
        use notify::{RecursiveMode, Watcher};

        let mount_root = mount_root.as_ref().to_path_buf();
        let debounce = Arc::new(Mutex::new(Debounce::new(DEBOUNCE)));
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));

        let queue = Arc::clone(&debounce);
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let Ok(event) = event else { return };
                // Only creations matter: a mount appears as a new directory under
                // the mount root. Writes inside a mounted card are not our business.
                if !matches!(event.kind, notify::EventKind::Create(_)) {
                    return;
                }
                if let Ok(mut debounce) = queue.lock() {
                    for path in event.paths {
                        debounce.touch(path, Instant::now());
                    }
                }
            })
            .map_err(|e| {
                Error::Internal(format!("could not watch {}: {e}", mount_root.display()))
            })?;

        watcher
            .watch(&mount_root, RecursiveMode::NonRecursive)
            .map_err(|e| {
                Error::Internal(format!("could not watch {}: {e}", mount_root.display()))
            })?;

        let timer_running = Arc::clone(&running);
        let timer_debounce = Arc::clone(&debounce);
        std::thread::Builder::new()
            .name("card-detection".into())
            .spawn(move || {
                while timer_running.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(250));

                    let due = match timer_debounce.lock() {
                        Ok(mut d) => d.ready(Instant::now()),
                        Err(_) => break,
                    };

                    for path in due {
                        let inspected = match ledger.lock() {
                            Ok(ledger) => inspect(&path, &ledger),
                            Err(_) => continue,
                        };
                        // A card that cannot be inspected is not worth crashing
                        // the watcher over; the next mount still gets a look.
                        if let Ok(Some(detected)) = inspected {
                            on_card(detected);
                        }
                    }
                }
            })
            .map_err(|e| Error::Internal(format!("could not start card detection: {e}")))?;

        Ok(Self {
            _watcher: watcher,
            running,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> Ledger {
        Ledger::open_in_memory().unwrap()
    }

    fn card_dir(temp: &Path, name: &str, files: &[&str]) -> PathBuf {
        let root = temp.join(name);
        let dcim = root.join("DCIM").join("100CANON");
        std::fs::create_dir_all(&dcim).unwrap();
        for file in files {
            std::fs::write(dcim.join(file), file.as_bytes()).unwrap();
        }
        root
    }

    // ------------------------------------------------------------- debounce

    #[test]
    fn a_burst_of_events_produces_one_look() {
        // A card reader emits several events milliseconds apart. Looking at the
        // volume once per event would scan a half-populated mount.
        let mut d = Debounce::new(Duration::from_millis(100));
        let start = Instant::now();

        d.touch("/Volumes/EOS", start);
        d.touch("/Volumes/EOS", start + Duration::from_millis(10));
        d.touch("/Volumes/EOS", start + Duration::from_millis(20));

        assert!(d.ready(start + Duration::from_millis(50)).is_empty());

        let due = d.ready(start + Duration::from_millis(200));
        assert_eq!(due, vec![PathBuf::from("/Volumes/EOS")]);
        assert!(d.is_empty(), "a path is reported once, not repeatedly");
    }

    #[test]
    fn a_later_event_restarts_the_window() {
        let mut d = Debounce::new(Duration::from_millis(100));
        let start = Instant::now();

        d.touch("/Volumes/EOS", start);
        d.touch("/Volumes/EOS", start + Duration::from_millis(90));

        assert!(
            d.ready(start + Duration::from_millis(120)).is_empty(),
            "the window runs from the most recent event"
        );
        assert_eq!(d.ready(start + Duration::from_millis(200)).len(), 1);
    }

    #[test]
    fn two_volumes_are_tracked_separately() {
        let mut d = Debounce::new(Duration::from_millis(100));
        let start = Instant::now();

        d.touch("/Volumes/EOS", start);
        d.touch("/Volumes/BACKUP", start + Duration::from_millis(150));

        assert_eq!(
            d.ready(start + Duration::from_millis(200)),
            vec![PathBuf::from("/Volumes/EOS")]
        );
        assert_eq!(
            d.ready(start + Duration::from_millis(300)),
            vec![PathBuf::from("/Volumes/BACKUP")]
        );
    }

    // -------------------------------------------------------------- inspect

    #[test]
    fn a_volume_without_dcim_is_not_a_card() {
        // F10: "test for a DCIM directory". Someone's backup drive must not
        // raise a card notification.
        let temp = tempfile::tempdir().unwrap();
        let drive = temp.path().join("BACKUP");
        std::fs::create_dir_all(drive.join("Documents")).unwrap();

        assert_eq!(inspect(&drive, &ledger()).unwrap(), None);
    }

    #[test]
    fn a_card_with_shots_is_announced() {
        let temp = tempfile::tempdir().unwrap();
        let card = card_dir(
            temp.path(),
            "EOS_DIGITAL",
            &["IMG_0001.JPG", "IMG_0002.JPG"],
        );

        let detected = inspect(&card, &ledger()).unwrap().expect("a card");

        assert_eq!(detected.label.as_deref(), Some("EOS_DIGITAL"));
        assert_eq!(detected.shots, 2);
        assert_eq!(detected.new_shots, 2);
        assert!(!detected.seen_before);
        assert_eq!(
            detected.notification(),
            "EOS_DIGITAL — 2 new shots. Review?"
        );
    }

    #[test]
    fn a_raw_plus_jpeg_pair_is_announced_as_one_shot() {
        // The notification counts photographs, not files.
        let temp = tempfile::tempdir().unwrap();
        let card = card_dir(
            temp.path(),
            "EOS_DIGITAL",
            &["IMG_0001.JPG", "IMG_0001.CR2"],
        );

        let detected = inspect(&card, &ledger()).unwrap().expect("a card");

        assert_eq!(detected.shots, 1);
        assert_eq!(detected.notification(), "EOS_DIGITAL — 1 new shot. Review?");
    }

    #[test]
    fn a_card_already_ingested_is_not_announced_again() {
        // F10 recognises reinserted cards precisely so this does not happen.
        let temp = tempfile::tempdir().unwrap();
        let card = card_dir(temp.path(), "EOS_DIGITAL", &["IMG_0001.JPG"]);
        let ledger = ledger();

        let scan = phototools_core::ingest::scan_card(
            &Card::at(&card).unwrap(),
            &phototools_core::jobs::InMemoryProgress::new(),
        )
        .unwrap();
        phototools_core::ingest::record_scan(&scan, &ledger).unwrap();

        assert_eq!(
            inspect(&card, &ledger).unwrap(),
            None,
            "nothing new means nothing to interrupt anyone about"
        );
    }

    #[test]
    fn more_frames_on_a_known_card_are_announced_as_new() {
        let temp = tempfile::tempdir().unwrap();
        let card = card_dir(temp.path(), "EOS_DIGITAL", &["IMG_0001.JPG"]);
        let ledger = ledger();

        let scan = phototools_core::ingest::scan_card(
            &Card::at(&card).unwrap(),
            &phototools_core::jobs::InMemoryProgress::new(),
        )
        .unwrap();
        phototools_core::ingest::record_scan(&scan, &ledger).unwrap();

        std::fs::write(
            card.join("DCIM").join("100CANON").join("IMG_0002.JPG"),
            b"another",
        )
        .unwrap();

        let detected = inspect(&card, &ledger).unwrap().expect("new frames");
        assert_eq!(detected.shots, 2);
        assert_eq!(detected.new_shots, 1, "only the new frame is new");
        assert!(detected.seen_before);
    }

    #[test]
    fn a_volume_that_vanished_is_not_an_error() {
        // Someone pulled the card out between the event and this look.
        let temp = tempfile::tempdir().unwrap();
        let gone = temp.path().join("EOS_DIGITAL");

        assert_eq!(inspect(&gone, &ledger()).unwrap(), None);
    }

    #[test]
    fn an_empty_card_raises_no_notification() {
        let temp = tempfile::tempdir().unwrap();
        let card = card_dir(temp.path(), "EOS_DIGITAL", &[]);

        assert_eq!(inspect(&card, &ledger()).unwrap(), None);
    }

    // ---------------------------------------------------------- the watcher

    #[test]
    fn the_watcher_reports_a_card_that_appears_under_the_mount_root() {
        // Exercises the whole path — notify event, debounce, inspect, callback —
        // against a stand-in mount root, which is all `/Volumes` is on macOS.
        let temp = tempfile::tempdir().unwrap();
        let mounts = temp.path().join("Volumes");
        std::fs::create_dir_all(&mounts).unwrap();

        let seen = Arc::new(Mutex::new(Vec::<CardDetected>::new()));
        let recorder = Arc::clone(&seen);

        let _watcher = VolumeWatcher::start(&mounts, Arc::new(Mutex::new(ledger())), move |card| {
            recorder.lock().unwrap().push(card)
        })
        .unwrap();

        card_dir(&mounts, "EOS_DIGITAL", &["IMG_0001.JPG", "IMG_0002.JPG"]);

        // The debounce window plus the timer's own tick.
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline && seen.lock().unwrap().is_empty() {
            std::thread::sleep(Duration::from_millis(100));
        }

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "one mount, one notification");
        assert_eq!(seen[0].label.as_deref(), Some("EOS_DIGITAL"));
        assert_eq!(seen[0].shots, 2);
    }
}
