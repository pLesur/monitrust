use std::fmt::Debug;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use anyhow::Result;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::{info, warn};

use crate::alert_reporter::AlertReporter;

pub mod disk_space;
pub mod memory;

#[derive(Debug, Clone)]
pub struct ActiveAlert {
    pub message: String,
}

pub trait Checker {
    type CheckResult;
    type Configuration: DeserializeOwned + Debug;
    fn check(&self) -> Result<Self::CheckResult>;
    fn period(&self) -> Duration;

    fn new(configuration: Self::Configuration) -> Self;
}

pub trait Alert {
    type Checker: Checker;
    fn is_triggered(&self, check_result: &<Self::Checker as Checker>::CheckResult) -> Option<ActiveAlert>;
}

pub trait Watcher {
    fn run<A: AlertReporter>(&self, alert_reporter: &A) -> Result<()>;
}

pub struct MultiWatcher<A: Alert> {
    checker: A::Checker,
    alerts: Vec<A>,
}

impl<A: Alert + DeserializeOwned + Clone + Debug> MultiWatcher<A> {
    pub fn new(serialized_configuration: SerializedMultiWatcher<A>) -> Self {
        MultiWatcher { checker: A::Checker::new(serialized_configuration.configuration), alerts: serialized_configuration.alerts }
    }

    pub fn period(&self) -> Duration {
        self.checker.period()
    }
}

impl<A: Alert> Watcher for MultiWatcher<A> {
    fn run<R: AlertReporter>(&self, alert_reporter: &R) -> Result<()> {
        let check_result = self.checker.check()?;
        self.alerts
            .iter()
            .filter_map(|a| a.is_triggered(&check_result))
            .inspect(|a| info!(firing_alert = ?a))
            .filter_map(|alert| match alert_reporter.report(&alert) {
                Ok(_) => None,
                Err(e) => Some(e),
            }).for_each(|e| {
            warn!(alert_reporter = ?e);
        });
        Ok(())
    }
}

#[derive(Deserialize, Debug)]
pub struct SerializedMultiWatcher<A: Clone + Debug + Alert> {
    configuration: <A::Checker as Checker>::Configuration,
    alerts: Vec<A>,
}

pub enum WatcherEnum {
    DiskSpace(MultiWatcher<disk_space::Alert>),
    Memory(MultiWatcher<memory::Alert>),
}

impl WatcherEnum {
    pub fn period(&self) -> Duration {
        match self {
            WatcherEnum::DiskSpace(d) => d.period(),
            WatcherEnum::Memory(m) => m.period(),
        }
    }
}

impl Watcher for WatcherEnum {
    fn run<A: AlertReporter>(&self, alert_reporter: &A) -> Result<()> {
        match self {
            WatcherEnum::DiskSpace(d) => d.run(alert_reporter),
            WatcherEnum::Memory(m) => m.run(alert_reporter),
        }
    }
}

#[derive(Deserialize, Debug)]
pub enum WatcherConfiguration {
    DiskSpace(SerializedMultiWatcher<disk_space::Alert>),
    Memory(SerializedMultiWatcher<memory::Alert>),
}

impl Into<WatcherEnum> for WatcherConfiguration {
    fn into(self) -> WatcherEnum {
        match self {
            WatcherConfiguration::DiskSpace(d) => WatcherEnum::DiskSpace(MultiWatcher::new(d)),
            WatcherConfiguration::Memory(m) => WatcherEnum::Memory(MultiWatcher::new(m))
        }
    }
}

impl PartialEq<Self> for WatcherConfiguration {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Hash for WatcherConfiguration {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state)
    }
}

impl Eq for WatcherConfiguration {}