//! Airglow core: hardware drivers, lighting engine, pure-Rust Studio layer compositor,
//! real-time screen/audio inputs, fan curve safety controller, LHM fan control, cooling recorder,
//! Python Airglow migration, Windows system integration, automation rules, and sensor telemetry.

pub mod automations;
pub mod config;
pub mod cooling;
pub mod devices;
pub mod effects;
pub mod engine;
pub mod fans;
pub mod geometry;
pub mod inputs;
pub mod lhm_control;
pub mod migrate;
pub mod openrgb;
pub mod sensors;
pub mod system;
pub mod updates;
