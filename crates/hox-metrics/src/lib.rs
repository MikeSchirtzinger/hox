//! # hox-metrics
//!
//! Observability and telemetry for Hox orchestration.
//!
//! This crate provides:
//! - Agent telemetry collection
//! - Metrics storage (JJ-native or external)
//! - Evaluation hooks at status transitions

#![allow(dead_code)]

mod collector;
mod storage;

pub use collector::{MetricsCollector, TelemetryEvent};
pub use storage::{MetricsStorage, StorageMode};
