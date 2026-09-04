//! Uçtan uca iş akışı motoru (Faz 0: iskelet).
//!
//! Denetleyici süpervizör, model/görev spec'i, sağlık kontrolleri ve
//! indirme yönetimi. Faz 1+.
pub mod download;
pub mod health;
pub mod spec;
pub mod supervisor;