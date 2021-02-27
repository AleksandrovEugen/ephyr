//! Shareable (exportable and importable) specification of application's
//! [`State`].
//!
//! [`State`]: crate::state::State

pub mod v1;

use derive_more::From;
use serde::{Deserialize, Serialize};

/// All supported versions of shareable (exportable and importable)
/// specifications of application's [`State`].
///
/// [`State`]: crate::state::State
#[derive(Clone, Debug, Default, Deserialize, From, Serialize)]
#[serde(tag = "version", rename_all = "lowercase")]
pub enum Spec {
    /// Version 1 of this [`Spec`].
    V1(v1::Spec),
}

impl Spec {
    /// Converts this [`Spec`] into a [`v1::Spec`].
    #[inline]
    #[must_use]
    pub fn into_v1(self) -> v1::Spec {
        match self {
            Self::V1(s) => s,
        }
    }
}
