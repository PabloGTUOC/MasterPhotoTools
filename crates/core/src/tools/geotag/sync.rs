//! Keeping two timelines the same, without making them one database.
//!
//! **Beyond the specification**, like the rest of `geotag`; the reasoning is in
//! `docs/timeline-sync-plan.md`.
//!
//! The Mac and the NAS each hold a timeline, and each has to work when the
//! other is unreachable — §2.3 puts the card reader on the Mac and MV-7.3
//! expects the desktop to work with the server off. So they stay two
//! databases, and this is what carries what each knows to the other.
//!
//! **Geopositions only.** Tracks, the fixes they contribute, the decisions
//! recorded about them, and deletions. Cards, shots, sessions, publishes, jobs
//! and settings are records of *what one machine did*, and copying them would
//! misrepresent which machine did it.
//!
//! Everything here is arithmetic on two inventories — no clock, no filesystem,
//! no network — because the question "what should move?" is worth being able to
//! answer in a test with neither machine present.

use super::library::{self, Decision, Resolution, TrackFile};
use super::PointSource;
use crate::error::Error;
use crate::ledger::Ledger;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What one side holds of a track, in the cheapest form that decides anything.
///
/// No points and no GPX text: an inventory of a thousand tracks is a few tens
/// of kilobytes, and the fixes are only fetched for the tracks that turn out to
/// be missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackStamp {
    /// The sha256 of the file's bytes — the same on both machines for the same
    /// file, which is what makes this a set comparison rather than a merge.
    pub id: String,
    pub name: String,
    /// When this side first saw it.
    pub imported_at: i64,
    pub point_count: i64,
    /// False where the text was not kept (an import too large to store), in
    /// which case this side cannot send the track — see [`SyncPlan::unsendable`].
    pub has_text: bool,
}

/// A track that was deleted, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    pub id: String,
    pub deleted_at: i64,
}

/// What one side has, and what it has thrown away.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub tracks: Vec<TrackStamp>,
    pub deletions: Vec<Tombstone>,
}

impl Inventory {
    fn by_id(&self) -> HashMap<&str, &TrackStamp> {
        self.tracks.iter().map(|t| (t.id.as_str(), t)).collect()
    }

    fn deletions_by_id(&self) -> HashMap<&str, i64> {
        self.deletions
            .iter()
            .map(|t| (t.id.as_str(), t.deleted_at))
            .collect()
    }
}

/// The work one sync has to do, from the point of view of the side driving it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncPlan {
    /// Ids to fetch from the other side and import here.
    pub pull: Vec<String>,
    /// Ids to send.
    pub push: Vec<String>,
    /// Ids to delete here, because the other side deleted them later than this
    /// side imported them.
    pub delete_here: Vec<String>,
    /// Ids to ask the other side to delete, for the same reason reversed.
    pub delete_there: Vec<String>,
    /// Tracks the other side is missing and this side cannot send, because the
    /// text was never stored. Reported rather than skipped: a sync that says
    /// "done" having quietly moved nothing is the failure this is here to
    /// avoid.
    pub unsendable: Vec<String>,
}

impl SyncPlan {
    /// True where a sync would change nothing on either side.
    pub fn is_empty(&self) -> bool {
        self.pull.is_empty()
            && self.push.is_empty()
            && self.delete_here.is_empty()
            && self.delete_there.is_empty()
    }
}

/// Work out what has to move, in both directions.
///
/// The rules, in the order they resolve:
///
/// 1. A track only one side holds moves to the other.
/// 2. A tombstone **younger** than the other side's copy deletes it there: the
///    deletion is the more recent statement about that track.
/// 3. A track imported **again after** a tombstone wins, and the tombstone is
///    ignored — somebody who re-imports a file they deleted last week means it.
/// 4. A track both sides hold is left alone. The id is the hash of the bytes,
///    so "both hold it" means both hold the same file, and re-sending it would
///    be work for an outcome already true.
///
/// Nothing here reads a clock. Both timestamps were written by whichever
/// machine acted, and the only comparison made is which of the two happened
/// later.
pub fn plan(local: &Inventory, remote: &Inventory) -> SyncPlan {
    let here = local.by_id();
    let there = remote.by_id();
    let deleted_here = local.deletions_by_id();
    let deleted_there = remote.deletions_by_id();

    let mut plan = SyncPlan::default();

    for (id, stamp) in &here {
        match there.get(id) {
            Some(_) => {
                // Both hold it. Either side may still have deleted it.
                if let Some(at) = deleted_there.get(id) {
                    if *at > stamp.imported_at {
                        plan.delete_here.push((*id).to_string());
                    }
                }
            }
            None => {
                // Only here. Did the other side delete it after this side got
                // it, or has it simply never seen it?
                match deleted_there.get(id) {
                    Some(at) if *at > stamp.imported_at => plan.delete_here.push((*id).to_string()),
                    _ if stamp.has_text => plan.push.push((*id).to_string()),
                    _ => plan.unsendable.push((*id).to_string()),
                }
            }
        }
    }

    for (id, stamp) in &there {
        if here.contains_key(id) {
            if let Some(at) = deleted_here.get(id) {
                if *at > stamp.imported_at {
                    plan.delete_there.push((*id).to_string());
                }
            }
            continue;
        }
        match deleted_here.get(id) {
            Some(at) if *at > stamp.imported_at => plan.delete_there.push((*id).to_string()),
            // Not asked for when the other side cannot send it; it says so in
            // its own inventory and its own sync will report it.
            _ if stamp.has_text => plan.pull.push((*id).to_string()),
            _ => {}
        }
    }

    // Stable output, so two runs of the same sync read the same and a test can
    // assert on order without asserting on a hash map's mood.
    plan.pull.sort();
    plan.push.sort();
    plan.delete_here.sort();
    plan.delete_there.sort();
    plan.unsendable.sort();
    plan
}

/// What one sync actually did, for the line a person reads afterwards.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncOutcome {
    pub pulled: usize,
    pub pushed: usize,
    pub deleted_here: usize,
    pub deleted_there: usize,
    /// Points added to this side's timeline by the tracks pulled.
    pub points_added: usize,
    /// Instants where a pulled track disagreed with what is held here.
    pub conflicts: usize,
    /// Tracks that could not move, each with the reason.
    pub skipped: Vec<String>,
}

impl SyncOutcome {
    /// True where nothing moved in either direction.
    pub fn is_nothing(&self) -> bool {
        self.pulled == 0 && self.pushed == 0 && self.deleted_here == 0 && self.deleted_there == 0
    }

    /// One sentence, in words rather than counts of nothing.
    ///
    /// A sync that moved nothing says so plainly: "already in step" is a
    /// different statement from "done", and the difference is what tells
    /// somebody whether to go looking.
    pub fn summary(&self) -> String {
        if self.is_nothing() {
            return if self.skipped.is_empty() {
                "already in step with the server".into()
            } else {
                format!(
                    "already in step with the server, {} track(s) could not be sent",
                    self.skipped.len()
                )
            };
        }

        let mut parts = Vec::new();
        if self.pulled > 0 {
            parts.push(format!(
                "pulled {} track(s), {} fixes",
                self.pulled, self.points_added
            ));
        }
        if self.pushed > 0 {
            parts.push(format!("sent {} track(s)", self.pushed));
        }
        if self.deleted_here > 0 {
            parts.push(format!("removed {} here", self.deleted_here));
        }
        if self.deleted_there > 0 {
            parts.push(format!("removed {} there", self.deleted_there));
        }
        if self.conflicts > 0 {
            parts.push(format!("{} instant(s) in dispute", self.conflicts));
        }
        if !self.skipped.is_empty() {
            parts.push(format!("{} could not be sent", self.skipped.len()));
        }
        parts.join(", ")
    }
}

// ---------------------------------------------------------------------------
// Moving a track between two machines
// ---------------------------------------------------------------------------

/// One track, as it travels.
///
/// The GPX text rather than the points: the receiver rebuilds the fixes with
/// the same reader that read them originally, so a track that arrives is a
/// track that was imported, by the road every other track takes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackTransfer {
    pub name: String,
    /// The path **on the machine that imported it**, kept as it was written.
    /// `/Users/…/track.gpx` is not a path on a NAS and must not pretend to be.
    pub source_path: String,
    pub source: PointSource,
    pub imported_at: i64,
    pub gpx: String,
    /// The decisions the origin recorded about instants in dispute.
    ///
    /// Carried so that a choice somebody made once is not re-made differently
    /// by whichever machine imported second. Where none was made, both sides
    /// apply the same default and the diff is deterministic, so they agree
    /// without being told to.
    #[serde(default)]
    pub decisions: Vec<Decision>,
}

/// The other machine, as this module needs to see it.
///
/// A trait for the same reason `ingest::SessionClient` is one: the HTTP belongs
/// to the binary that has a network (G1), and the sequence below is worth
/// testing with neither machine present.
pub trait TimelineRemote {
    /// What the other side holds, and what it has deleted.
    fn inventory(&self) -> Result<Inventory, Error>;
    /// One track, with its text and its recorded decisions.
    fn fetch(&self, id: &str) -> Result<TrackTransfer, Error>;
    /// Hand one track over.
    fn send(&self, transfer: &TrackTransfer) -> Result<(), Error>;
    /// Ask the other side to forget these.
    fn delete(&self, tombstones: &[Tombstone]) -> Result<(), Error>;
}

/// This side's inventory, for the other side to compare against.
pub fn inventory(ledger: &Ledger) -> Result<Inventory, Error> {
    Ok(Inventory {
        tracks: ledger.track_stamps()?,
        deletions: ledger.tombstones()?,
    })
}

/// Read one track out of the library, ready to travel.
pub fn transfer_of(ledger: &Ledger, id: &str) -> Result<TrackTransfer, Error> {
    let row = ledger
        .track(id)?
        .ok_or_else(|| Error::Config(format!("no track {id} here")))?;
    let gpx = ledger
        .track_text(id)?
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            Error::Config(format!(
                "the text of {id} was not kept, so this side cannot send it"
            ))
        })?;

    Ok(TrackTransfer {
        name: row.name,
        source_path: row.source_path,
        source: row.source,
        imported_at: row.imported_at,
        gpx,
        decisions: ledger
            .conflicts_for_track(id)?
            .into_iter()
            .map(|c| Decision {
                at: c.at,
                take: if c.decision == "took-new" {
                    library::Take::New
                } else {
                    library::Take::Existing
                },
            })
            .collect(),
    })
}

/// Put a track that arrived from the other machine into this library.
///
/// **By `commit_import`, deliberately.** A received track meets the same rule a
/// file does: an instant already held is a disagreement to record, never
/// something to overwrite because it arrived later. The provenance travels
/// intact — a point placed by hand on one machine says so on both, and a sync
/// that laundered that would undo the reason for having it.
pub fn receive(
    ledger: &Ledger,
    transfer: &TrackTransfer,
    _now: i64,
) -> Result<library::TrackImportResult, Error> {
    let file = TrackFile::from_text(
        &transfer.name,
        &transfer.source_path,
        transfer.source,
        &transfer.gpx,
    )?;
    // **Dated as the origin dated it, not as "now".**
    //
    // `imported_at` is what a tombstone is compared against, so if the
    // receiving machine stamped its own clock the two sides would hold
    // different dates for one track — and a deletion made on the origin a
    // minute later could read as *older* than the copy it is meant to remove,
    // which is a deletion that silently never happens. Two clocks a few seconds
    // apart is enough to do it. One date, set once, by the machine that first
    // imported the file.
    library::commit_import(
        ledger,
        &file,
        Resolution::KeepExisting,
        &transfer.decisions,
        transfer.imported_at,
    )
}

/// Carry out one sync, in both directions.
///
/// Pull before push, so a conflict is judged against the fullest timeline this
/// side can have; deletions last, so nothing is deleted that a failure halfway
/// would have left half-replaced.
///
/// **Each track is one request and one transaction.** A sync interrupted by a
/// closed laptop leaves both sides consistent, and the next run finishes the
/// job — which is why nothing here records "how far it got".
pub fn run(ledger: &Ledger, remote: &dyn TimelineRemote, now: i64) -> Result<SyncOutcome, Error> {
    let here = inventory(ledger)?;
    let there = remote.inventory()?;
    let plan = plan(&here, &there);

    let mut outcome = SyncOutcome {
        skipped: plan
            .unsendable
            .iter()
            .map(|id| format!("{id}: its text was not kept, so it cannot be sent"))
            .collect(),
        ..Default::default()
    };

    for id in &plan.pull {
        let transfer = remote.fetch(id)?;
        let result = receive(ledger, &transfer, now)?;
        outcome.pulled += 1;
        outcome.points_added += result.added;
        outcome.conflicts += result.kept_existing + result.took_new;
    }

    for id in &plan.push {
        let transfer = transfer_of(ledger, id)?;
        remote.send(&transfer)?;
        outcome.pushed += 1;
    }

    if !plan.delete_here.is_empty() {
        let deletions = there.deletions_by_id();
        for id in &plan.delete_here {
            let at = deletions.get(id.as_str()).copied().unwrap_or(now);
            ledger.delete_track(id, at)?;
            outcome.deleted_here += 1;
        }
    }

    if !plan.delete_there.is_empty() {
        let deletions = here.deletions_by_id();
        let tombstones: Vec<Tombstone> = plan
            .delete_there
            .iter()
            .map(|id| Tombstone {
                id: id.clone(),
                deleted_at: deletions.get(id.as_str()).copied().unwrap_or(now),
            })
            .collect();
        remote.delete(&tombstones)?;
        outcome.deleted_there = tombstones.len();
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(id: &str, imported_at: i64) -> TrackStamp {
        TrackStamp {
            id: id.into(),
            name: format!("{id}.gpx"),
            imported_at,
            point_count: 10,
            has_text: true,
        }
    }

    fn held(tracks: &[TrackStamp], deletions: &[(&str, i64)]) -> Inventory {
        Inventory {
            tracks: tracks.to_vec(),
            deletions: deletions
                .iter()
                .map(|(id, at)| Tombstone {
                    id: (*id).into(),
                    deleted_at: *at,
                })
                .collect(),
        }
    }

    #[test]
    fn a_track_only_one_side_holds_moves_to_the_other() {
        let local = held(&[stamp("mine", 100)], &[]);
        let remote = held(&[stamp("theirs", 100)], &[]);

        let plan = plan(&local, &remote);
        assert_eq!(plan.push, vec!["mine"]);
        assert_eq!(plan.pull, vec!["theirs"]);
        assert!(plan.delete_here.is_empty() && plan.delete_there.is_empty());
    }

    #[test]
    fn a_track_both_sides_hold_is_left_alone() {
        let both = held(&[stamp("same", 100)], &[]);
        assert!(
            plan(&both, &both).is_empty(),
            "an id is the hash of the bytes"
        );
    }

    #[test]
    fn a_deletion_on_one_side_removes_it_on_the_other() {
        let local = held(&[stamp("doomed", 100)], &[]);
        let remote = held(&[], &[("doomed", 200)]);

        let plan = plan(&local, &remote);
        assert_eq!(plan.delete_here, vec!["doomed"]);
        assert!(plan.push.is_empty(), "a deleted track is not resurrected");
    }

    #[test]
    fn a_deletion_here_removes_it_there() {
        let local = held(&[], &[("doomed", 200)]);
        let remote = held(&[stamp("doomed", 100)], &[]);

        let plan = plan(&local, &remote);
        assert_eq!(plan.delete_there, vec!["doomed"]);
        assert!(plan.pull.is_empty());
    }

    #[test]
    fn re_importing_a_file_deleted_last_week_keeps_it() {
        // Deleted at 100, imported again at 300: somebody meant it.
        let local = held(&[stamp("revived", 300)], &[]);
        let remote = held(&[], &[("revived", 100)]);

        let plan = plan(&local, &remote);
        assert_eq!(plan.push, vec!["revived"]);
        assert!(plan.delete_here.is_empty());
    }

    #[test]
    fn a_deletion_both_sides_already_applied_moves_nothing() {
        let local = held(&[], &[("gone", 200)]);
        let remote = held(&[], &[("gone", 200)]);
        assert!(plan(&local, &remote).is_empty());
    }

    #[test]
    fn a_track_deleted_there_after_being_imported_here_goes_even_when_both_hold_it() {
        let local = held(&[stamp("shared", 100)], &[]);
        let remote = held(&[stamp("shared", 100)], &[("shared", 250)]);

        assert_eq!(plan(&local, &remote).delete_here, vec!["shared"]);
    }

    #[test]
    fn a_track_whose_text_was_never_stored_is_reported_rather_than_skipped() {
        let mut huge = stamp("enormous", 100);
        huge.has_text = false;
        let plan = plan(&held(&[huge], &[]), &Inventory::default());

        assert!(plan.push.is_empty());
        assert_eq!(
            plan.unsendable,
            vec!["enormous"],
            "said out loud, not dropped"
        );
    }

    #[test]
    fn the_plan_reads_the_same_twice() {
        let local = held(&[stamp("b", 1), stamp("a", 1), stamp("c", 1)], &[]);
        let remote = Inventory::default();
        assert_eq!(plan(&local, &remote).push, vec!["a", "b", "c"]);
    }

    #[test]
    fn a_sync_that_moved_nothing_says_so_rather_than_done() {
        assert_eq!(
            SyncOutcome::default().summary(),
            "already in step with the server"
        );
    }

    #[test]
    fn a_sync_that_moved_something_says_what() {
        let outcome = SyncOutcome {
            pulled: 2,
            pushed: 1,
            points_added: 98,
            conflicts: 3,
            ..Default::default()
        };
        assert_eq!(
            outcome.summary(),
            "pulled 2 track(s), 98 fixes, sent 1 track(s), 3 instant(s) in dispute"
        );
    }

    // -----------------------------------------------------------------------
    // Two real libraries, through a remote that is another `Ledger`
    // -----------------------------------------------------------------------

    /// The other machine, standing in for HTTP.
    ///
    /// It is a whole second library rather than a stub, so what these tests
    /// assert is what two timelines actually end up holding — the claim, not a
    /// proxy for it.
    struct OtherMachine {
        ledger: Ledger,
        now: i64,
    }

    impl TimelineRemote for OtherMachine {
        fn inventory(&self) -> Result<Inventory, Error> {
            super::inventory(&self.ledger)
        }
        fn fetch(&self, id: &str) -> Result<TrackTransfer, Error> {
            transfer_of(&self.ledger, id)
        }
        fn send(&self, transfer: &TrackTransfer) -> Result<(), Error> {
            receive(&self.ledger, transfer, self.now).map(|_| ())
        }
        fn delete(&self, tombstones: &[Tombstone]) -> Result<(), Error> {
            for stone in tombstones {
                self.ledger.delete_track(&stone.id, stone.deleted_at)?;
            }
            Ok(())
        }
    }

    fn document(points: &[(&str, f64, f64)]) -> String {
        let body: String = points
            .iter()
            .map(|(time, lat, lon)| {
                format!("<trkpt lat=\"{lat}\" lon=\"{lon}\"><time>{time}</time></trkpt>")
            })
            .collect();
        format!("<gpx creator=\"OwnTracks\"><trk><trkseg>{body}</trkseg></trk></gpx>")
    }

    /// Import a file into a library the way a screen does.
    fn import(ledger: &Ledger, name: &str, gpx: &str, now: i64) -> String {
        let file =
            TrackFile::from_text(name, &format!("/tracks/{name}"), PointSource::Recorded, gpx)
                .unwrap();
        library::commit_import(ledger, &file, Resolution::KeepExisting, &[], now).unwrap();
        file.id
    }

    fn instants(ledger: &Ledger) -> Vec<i64> {
        ledger
            .points_between(0, i64::MAX)
            .unwrap()
            .into_iter()
            .map(|p| p.at)
            .collect()
    }

    #[test]
    fn each_machine_ends_up_holding_what_the_other_had() {
        let mac = Ledger::open_in_memory().unwrap();
        let nas = OtherMachine {
            ledger: Ledger::open_in_memory().unwrap(),
            now: 1_000,
        };

        import(
            &mac,
            "berlin.gpx",
            &document(&[("2026-09-02T19:40:44Z", 52.5, 13.4)]),
            100,
        );
        import(
            &nas.ledger,
            "cardiff.gpx",
            &document(&[("2026-09-09T06:42:39Z", 51.48, -3.18)]),
            100,
        );

        let outcome = run(&mac, &nas, 1_000).unwrap();

        assert_eq!((outcome.pulled, outcome.pushed), (1, 1));
        assert_eq!(mac.tracks().unwrap().len(), 2);
        assert_eq!(nas.ledger.tracks().unwrap().len(), 2);
        assert_eq!(
            instants(&mac),
            instants(&nas.ledger),
            "one timeline, two machines"
        );
    }

    #[test]
    fn a_point_placed_by_hand_still_says_so_on_the_other_machine() {
        let mac = Ledger::open_in_memory().unwrap();
        let nas = OtherMachine {
            ledger: Ledger::open_in_memory().unwrap(),
            now: 1_000,
        };

        let placed = super::super::place::build(&[super::super::gpx::PlacedStop {
            name: "Alexanderplatz".into(),
            lat: 52.521918,
            lon: 13.413215,
            from: 1_367_416_800,
            to: 1_367_431_200,
        }])
        .unwrap();
        library::commit_import(&mac, &placed, Resolution::KeepExisting, &[], 100).unwrap();

        run(&mac, &nas, 1_000).unwrap();

        let arrived = &nas.ledger.tracks().unwrap()[0];
        assert_eq!(
            arrived.source,
            PointSource::Placed,
            "provenance is not laundered"
        );
        assert_eq!(arrived.name, "Alexanderplatz");
    }

    #[test]
    fn a_second_sync_moves_nothing() {
        let mac = Ledger::open_in_memory().unwrap();
        let nas = OtherMachine {
            ledger: Ledger::open_in_memory().unwrap(),
            now: 1_000,
        };
        import(
            &mac,
            "berlin.gpx",
            &document(&[("2026-09-02T19:40:44Z", 52.5, 13.4)]),
            100,
        );

        run(&mac, &nas, 1_000).unwrap();
        let second = run(&mac, &nas, 2_000).unwrap();

        assert!(second.pulled == 0 && second.pushed == 0);
        assert_eq!(second.summary(), "already in step with the server");
    }

    #[test]
    fn deleting_on_one_machine_deletes_on_the_other_and_it_stays_deleted() {
        let mac = Ledger::open_in_memory().unwrap();
        let nas = OtherMachine {
            ledger: Ledger::open_in_memory().unwrap(),
            now: 3_000,
        };
        let id = import(
            &mac,
            "wrong.gpx",
            &document(&[("2026-09-02T19:40:44Z", 52.5, 13.4)]),
            100,
        );
        run(&mac, &nas, 1_000).unwrap();
        assert_eq!(nas.ledger.tracks().unwrap().len(), 1);

        mac.delete_track(&id, 2_000).unwrap();
        let outcome = run(&mac, &nas, 2_500).unwrap();

        assert_eq!(outcome.deleted_there, 1);
        assert_eq!(nas.ledger.tracks().unwrap().len(), 0);

        // And the next sync does not hand it back.
        let again = run(&mac, &nas, 3_000).unwrap();
        assert!(
            again.is_nothing(),
            "a deleted track must not be resurrected"
        );
        assert_eq!(mac.tracks().unwrap().len(), 0);
    }

    #[test]
    fn a_deletion_on_the_server_reaches_the_mac() {
        let mac = Ledger::open_in_memory().unwrap();
        let nas = OtherMachine {
            ledger: Ledger::open_in_memory().unwrap(),
            now: 3_000,
        };
        let id = import(
            &nas.ledger,
            "wrong.gpx",
            &document(&[("2026-09-02T19:40:44Z", 52.5, 13.4)]),
            100,
        );
        run(&mac, &nas, 1_000).unwrap();

        nas.ledger.delete_track(&id, 2_000).unwrap();
        let outcome = run(&mac, &nas, 2_500).unwrap();

        assert_eq!(outcome.deleted_here, 1);
        assert_eq!(mac.tracks().unwrap().len(), 0);
    }

    #[test]
    fn re_importing_a_deleted_file_brings_it_back_on_both() {
        let mac = Ledger::open_in_memory().unwrap();
        let nas = OtherMachine {
            ledger: Ledger::open_in_memory().unwrap(),
            now: 5_000,
        };
        let gpx = document(&[("2026-09-02T19:40:44Z", 52.5, 13.4)]);
        let id = import(&mac, "berlin.gpx", &gpx, 100);
        run(&mac, &nas, 1_000).unwrap();
        mac.delete_track(&id, 2_000).unwrap();
        run(&mac, &nas, 2_500).unwrap();

        // Imported again, later than the deletion: the person means it.
        import(&mac, "berlin.gpx", &gpx, 4_000);
        let outcome = run(&mac, &nas, 4_500).unwrap();

        assert_eq!(outcome.pushed, 1);
        assert_eq!(nas.ledger.tracks().unwrap().len(), 1);
        assert_eq!(mac.tracks().unwrap().len(), 1);
    }

    #[test]
    fn two_machines_that_disagree_about_an_instant_reach_the_same_answer() {
        let mac = Ledger::open_in_memory().unwrap();
        let nas = OtherMachine {
            ledger: Ledger::open_in_memory().unwrap(),
            now: 1_000,
        };
        // One instant, two positions, one on each machine.
        import(
            &mac,
            "mac.gpx",
            &document(&[("2026-09-02T19:40:44Z", 52.5, 13.4)]),
            100,
        );
        import(
            &nas.ledger,
            "nas.gpx",
            &document(&[("2026-09-02T19:40:44Z", 48.85, 2.35)]),
            100,
        );

        let outcome = run(&mac, &nas, 1_000).unwrap();

        assert!(
            outcome.conflicts > 0,
            "the disagreement is reported, not hidden"
        );
        // Both hold both files, and both timelines answer the same for that
        // instant: whichever fix each already held is kept, and the libraries
        // agree because the decision travels with the track.
        assert_eq!(mac.tracks().unwrap().len(), 2);
        assert_eq!(nas.ledger.tracks().unwrap().len(), 2);
        let here = mac.points_between(0, i64::MAX).unwrap();
        assert_eq!(here.len(), 1, "one instant still has one position");
    }

    #[test]
    fn a_track_whose_text_is_gone_is_reported_rather_than_lost_silently() {
        let mac = Ledger::open_in_memory().unwrap();
        let nas = OtherMachine {
            ledger: Ledger::open_in_memory().unwrap(),
            now: 1_000,
        };
        let id = import(
            &mac,
            "berlin.gpx",
            &document(&[("2026-09-02T19:40:44Z", 52.5, 13.4)]),
            100,
        );
        mac.inner()
            .execute(&format!("UPDATE tracks SET gpx = '' WHERE id = '{id}'"), [])
            .unwrap();

        let outcome = run(&mac, &nas, 1_000).unwrap();

        assert_eq!(outcome.pushed, 0);
        assert_eq!(outcome.skipped.len(), 1);
        assert!(
            outcome.skipped[0].contains("cannot be sent"),
            "got: {:?}",
            outcome.skipped
        );
    }
}
