//! DV-8: several photographs developed at once, written in plan order, within a memory budget.

#[allow(dead_code)]
mod support;

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use phototools_core::error::Error;
use rollchrome_core::media::develop::{Look, OutputFormat};
use rollchrome_core::media::film::Grain;
use rollchrome_core::tools::develop::{
    estimated_peak, execute_develop, execute_develop_with, physical_memory, plan_develop,
    Concurrency, DevelopItem, DevelopStore, DevelopSummary, NEUTRAL_LOOK,
};
use support::dng::DngBuilder;

const JPEG: OutputFormat = OutputFormat::Jpeg { quality: 90 };

/// Every call the executor makes on the store, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Read(String),
    Create(String),
    Carry(String),
}

#[derive(Default)]
struct LogStore {
    sources: HashMap<String, Vec<u8>>,
    written: BTreeMap<String, Vec<u8>>,
    calls: Vec<Call>,
}

impl DevelopStore for LogStore {
    fn existing_outputs(&self) -> Result<BTreeSet<String>, Error> {
        Ok(BTreeSet::new())
    }
    fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
        // `read` takes `&self`; the log is pushed by the wrapper below.
        self.sources
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Refused(format!("{name} not found")))
    }
    fn look(&self, look: &str) -> Result<Look, Error> {
        Err(Error::Refused(format!("no library look '{look}'")))
    }
    fn create_new(&mut self, output: &str, bytes: &[u8]) -> Result<(), Error> {
        self.calls.push(Call::Create(output.to_string()));
        self.written.insert(output.to_string(), bytes.to_vec());
        Ok(())
    }
    fn carry_metadata(
        &mut self,
        _source: &str,
        output: &str,
        _width: u32,
        _height: u32,
        _format: OutputFormat,
    ) -> Result<(), Error> {
        self.calls.push(Call::Carry(output.to_string()));
        Ok(())
    }
}

/// Logs reads too: `DevelopStore::read` takes `&self`, so the log lives in a `Cell`-backed list.
struct Logged {
    inner: LogStore,
    reads: std::cell::RefCell<Vec<String>>,
    order: std::cell::RefCell<Vec<Call>>,
}

impl Logged {
    fn new(inner: LogStore) -> Self {
        Self {
            inner,
            reads: Default::default(),
            order: Default::default(),
        }
    }
}

impl DevelopStore for Logged {
    fn existing_outputs(&self) -> Result<BTreeSet<String>, Error> {
        self.inner.existing_outputs()
    }
    fn read(&self, name: &str) -> Result<Vec<u8>, Error> {
        self.reads.borrow_mut().push(name.to_string());
        self.order.borrow_mut().push(Call::Read(name.to_string()));
        self.inner.read(name)
    }
    fn look(&self, look: &str) -> Result<Look, Error> {
        self.inner.look(look)
    }
    fn create_new(&mut self, output: &str, bytes: &[u8]) -> Result<(), Error> {
        self.order
            .borrow_mut()
            .push(Call::Create(output.to_string()));
        self.inner.create_new(output, bytes)
    }
    fn carry_metadata(
        &mut self,
        source: &str,
        output: &str,
        width: u32,
        height: u32,
        format: OutputFormat,
    ) -> Result<(), Error> {
        self.order
            .borrow_mut()
            .push(Call::Carry(output.to_string()));
        self.inner
            .carry_metadata(source, output, width, height, format)
    }
}

struct Progress {
    cancel_after_reports: Option<usize>,
    reports: Cell<usize>,
}

// The executor calls progress only from the calling thread; the trait still asks for Sync.
unsafe impl Sync for Progress {}

impl Progress {
    fn new() -> Self {
        Self {
            cancel_after_reports: None,
            reports: Cell::new(0),
        }
    }
}

impl phototools_core::jobs::Progress for Progress {
    fn report(&self, _done: u64, _total: u64, _current: &str) {
        self.reports.set(self.reports.get() + 1);
    }
    fn cancelled(&self) -> bool {
        self.cancel_after_reports
            .is_some_and(|n| self.reports.get() >= n)
    }
}

/// Six small RAWs, each a different grey, so a mix-up between photographs changes the bytes.
fn six_raws() -> LogStore {
    let mut store = LogStore::default();
    for i in 0..6u16 {
        let bytes = DngBuilder::new_bayer(40, 28).build(&vec![1500 + i * 400; 40 * 28]);
        store.sources.insert(format!("p{i}.dng"), bytes);
    }
    store
}

fn items(store: &LogStore, look: &str) -> Vec<DevelopItem> {
    let mut names: Vec<_> = store.sources.keys().cloned().collect();
    names.sort();
    names
        .into_iter()
        .map(|name| DevelopItem {
            name,
            look: Some(look.to_string()),
            exposure_ev: 0.0,
            grain: Grain::Off,
            noise_reduction: 10.0,
            lens_correction: false,
        })
        .collect()
}

fn run(store: &mut dyn DevelopStore, look_items: &[DevelopItem], c: Concurrency) -> DevelopSummary {
    let plan = plan_develop(look_items, JPEG, &BTreeSet::new());
    execute_develop_with(plan, store, &Progress::new(), c)
}

/// The most photographs read but not yet written at any point of the log.
fn most_ahead(order: &[Call]) -> usize {
    let (mut reads, mut created, mut most) = (0usize, BTreeSet::new(), 0usize);
    for call in order {
        match call {
            Call::Read(_) => reads += 1,
            Call::Create(name) => {
                created.insert(name.clone());
            }
            Call::Carry(_) => {}
        }
        most = most.max(reads - created.len());
    }
    most
}

#[test]
fn several_at_once_writes_the_same_bytes_in_the_same_order_as_one_at_a_time() {
    for look in [NEUTRAL_LOOK, "builtin:fujifilm-classic-chrome"] {
        let base = six_raws();
        let list = items(&base, look);

        let mut one = six_raws();
        let s1 = run(&mut one, &list, Concurrency::sequential());
        let mut four = six_raws();
        let s4 = run(&mut four, &list, Concurrency::for_this_machine(4));

        assert_eq!(s1.developed.len(), 6, "{look}: {:?}", s1.failures);
        assert_eq!(s1.developed, s4.developed, "{look}: plan order");
        assert_eq!(one.written, four.written, "{look}: the same bytes");
        assert_eq!(
            one.calls, four.calls,
            "{look}: created and carried in the same order"
        );
    }
}

#[test]
fn one_at_a_time_reads_writes_and_carries_each_photograph_before_the_next() {
    let base = six_raws();
    let list = items(&base, NEUTRAL_LOOK);
    let mut store = Logged::new(six_raws());
    let plan = plan_develop(&list, JPEG, &BTreeSet::new());
    execute_develop(plan, &mut store, &Progress::new());

    let order = store.order.borrow().clone();
    let expected: Vec<Call> = (0..6)
        .flat_map(|i| {
            [
                Call::Read(format!("p{i}.dng")),
                Call::Create(format!("p{i}.jpg")),
                Call::Carry(format!("p{i}.jpg")),
            ]
        })
        .collect();
    assert_eq!(order, expected);
}

#[test]
fn no_more_photographs_are_in_flight_than_the_jobs_allow() {
    let base = six_raws();
    let list = items(&base, NEUTRAL_LOOK);
    let mut store = Logged::new(six_raws());
    let summary = run(&mut store, &list, Concurrency::for_this_machine(3));

    assert_eq!(summary.developed.len(), 6);
    let ahead = most_ahead(&store.order.borrow());
    assert!(
        ahead <= 3,
        "{ahead} photographs were read ahead of the writes"
    );
    assert!(
        ahead >= 2,
        "with three jobs the run must read ahead, got {ahead}"
    );
}

#[test]
fn a_budget_smaller_than_one_photograph_still_develops_them_one_at_a_time() {
    let base = six_raws();
    let list = items(&base, NEUTRAL_LOOK);
    let mut store = Logged::new(six_raws());
    let summary = run(
        &mut store,
        &list,
        Concurrency {
            max_jobs: 4,
            memory_budget: 1,
        },
    );

    assert_eq!(summary.developed.len(), 6, "{:?}", summary.failures);
    // One in flight, and at most one more read and waiting for room.
    let ahead = most_ahead(&store.order.borrow());
    assert!(
        ahead <= 2,
        "{ahead} photographs were read ahead of the writes"
    );
}

#[test]
fn cancelling_starts_nothing_new_and_writes_what_had_started() {
    let base = six_raws();
    let list = items(&base, NEUTRAL_LOOK);
    let mut store = six_raws();
    let plan = plan_develop(&list, JPEG, &BTreeSet::new());
    let progress = Progress {
        cancel_after_reports: Some(2),
        reports: Cell::new(0),
    };
    let summary = execute_develop_with(
        plan,
        &mut store,
        &progress,
        Concurrency::for_this_machine(4),
    );

    assert_eq!(summary.developed, vec!["p0.jpg", "p1.jpg"]);
    assert!(summary.failures.is_empty());
}

#[test]
fn a_photograph_that_fails_does_not_stop_the_others_and_keeps_its_place() {
    let mut base = six_raws();
    base.sources
        .insert("p2.dng".to_string(), b"not a raw file".to_vec());
    let list = items(&base, NEUTRAL_LOOK);
    let mut store = base;
    let summary = run(&mut store, &list, Concurrency::for_this_machine(4));

    assert_eq!(
        summary.developed,
        vec!["p0.jpg", "p1.jpg", "p3.jpg", "p4.jpg", "p5.jpg"]
    );
    assert_eq!(summary.failures.len(), 1);
    assert_eq!(summary.failures[0].file, "p2.dng");
}

#[test]
fn the_estimate_follows_size_film_and_a_second_output() {
    // An XZ-1 ORF of 11 MB: about 2.2 GB neutral and 3.4 GB through the film simulation.
    let orf = 11_000_000;
    assert_eq!(estimated_peak(orf, false, false), 2_200_000_000);
    assert_eq!(estimated_peak(orf, true, false), 3_437_500_000);
    assert!(estimated_peak(orf, true, true) > estimated_peak(orf, true, false));
    assert!(estimated_peak(2 * orf, false, false) == 2 * estimated_peak(orf, false, false));
}

#[cfg(unix)]
#[test]
fn this_machine_reports_its_memory_and_the_budget_is_half() {
    let memory = physical_memory().expect("sysconf reports physical memory");
    assert!(memory > 1 << 30, "{memory} bytes is less than 1 GB");
    let c = Concurrency::for_this_machine(4);
    assert_eq!(c.memory_budget, memory / 2);
    assert_eq!(c.max_jobs, 4);
    assert_eq!(Concurrency::for_this_machine(0).max_jobs, 1);
}
