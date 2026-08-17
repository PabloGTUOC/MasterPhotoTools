//! The archive operations (F1–F9)

pub mod f1_dates;
pub mod f2_takeout;
pub mod f3_rename;
pub mod f4_split;
pub mod f5_contact;
pub mod f6_transform;
pub mod f7_border;
pub mod f8_tiff;
pub mod f9_browser;

use crate::jobs::{Progress, ToolResult};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skip {
    pub file: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan<T> {
    pub actions: Vec<T>,
    pub skipped: Vec<Skip>,
}

pub trait Tool {
    type Params;
    type Action;
    type Summary;

    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>>;
    fn apply(&self, plan: Plan<Self::Action>, progress: &dyn Progress)
        -> ToolResult<Self::Summary>;
}
