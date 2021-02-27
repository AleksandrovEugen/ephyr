//! Version 1 of a shareable (exportable and importable) specification of
//! application's [`State`].
//!
//! [`State`]: state::State

use std::collections::HashSet;

use derive_more::{Deref, From, Into, IntoIterator};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

use crate::{serde::is_false, state};

/// Shareable (exportable and importable) specification of a [`State`].
///
/// [`State`]: state::State
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Spec {
    /// [`Restream`]s to be performed.
    #[serde(deserialize_with = "Spec::deserialize_restreams")]
    pub restreams: Vec<Restream>,
}

impl Spec {
    /// Deserializes [`Spec::restreams`] ensuring its invariants preserved.
    fn deserialize_outputs<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<Mixin>, D::Error> {
        let restreams = Vec::deserialize(deserializer)?;

        if !restreams.is_empty() {
            let mut unique = HashSet::with_capacity(restreams.len());
            for r in &restreams {
                if let Some(key) = unique.replace(&r.key) {
                    return Err(D::Error::custom(format!(
                        "Duplicate Restream.key in Spec.restreams: {}",
                        key,
                    )));
                }
            }
        }

        Ok(restreams)
    }
}

/// Shareable (exportable and importable) specification of a
/// [`state::Restream`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Restream {
    /// Unique key of this [`Restream`] identifying it, and used to form its
    /// endpoints URLs.
    pub key: state::RestreamKey,

    /// Optional label of this [`Restream`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<state::Label>,

    /// [`Input`] that a live stream is received from.
    pub input: Input,

    /// [`Output`]s that a live stream is re-streamed to.
    #[serde(
        default,
        deserialize_with = "Output::deserialize_outputs",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub outputs: Vec<Output>,
}

impl Restream {
    /// Deserializes [`Restream::outputs`] ensuring its invariants preserved.
    fn deserialize_outputs<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<Mixin>, D::Error> {
        let outputs = Vec::deserialize(deserializer)?;

        if !outputs.is_empty() {
            let mut unique = HashSet::with_capacity(outputs.len());
            for o in &outputs {
                if let Some(dst) = unique.replace(&o.dst) {
                    return Err(D::Error::custom(format!(
                        "Duplicate Output.dst in Restream.outputs: {}",
                        dst,
                    )));
                }
            }
        }

        Ok(outputs)
    }
}

/// Shareable (exportable and importable) specification of a [`state::Input`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Input {
    /// Key of this [`Input`] to expose its endpoint with for accepting and
    /// serving a live stream.
    pub key: state::InputKey,

    /// Sources to pull a live stream from.
    ///
    /// If empty then a live stream is received (pushed) rather than is pulled.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub srcs: Vec<InputSrc>,

    /// Indicator whether this [`Input`] is enabled, so is allowed to receive a
    /// live stream from its upstream sources.
    #[serde(default, skip_serializing_if = "is_false")]
    pub enabled: bool,
}

impl<'de> Deserialize<'de> for Input {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawInput {
            key: state::InputKey,
            #[serde(default)]
            srcs: Vec<InputSrc>,
            #[serde(default)]
            enabled: bool,
        }

        let raw = RawInput::deserialize(deserializer)?;

        if !srcs.is_empty() {
            fn ensure_srcs_unique(
                src: &InputSrc,
                urls: &mut HashSet<&state::InputSrcUrl>,
                keys: &mut HashSet<&state::InputKey>,
            ) -> Result<(), D::Error> {
                match src {
                    InputSrc::RemoteUrl(url) => {
                        if let Some(url) = unique_urls.replace(url) {
                            return Err(D::Error::custom(format!(
                                "Duplicate RemoteInputSrc.url in Input.srcs: \
                                 {}",
                                url,
                            )));
                        }
                    }
                    InputSrc::LocalInput(i) => {
                        if let Some(key) = unique_keys.replace(&i.key) {
                            return Err(D::Error::custom(format!(
                                "Duplicate Input.key in Input.srcs: {}",
                                key,
                            )));
                        }
                        for s in &i.srcs {
                            ensure_srcs_unique(s, urls, keys)?;
                        }
                    }
                }
                Ok(())
            }

            let mut unique_urls = HashSet::with_capacity(srcs.len());
            let mut unique_keys = HashSet::with_capacity(srcs.len() + 1);
            unique_keys.insert(&raw.key);
            for s in &srcs {
                ensure_srcs_unique(s, &mut unique_urls, &mut unique_keys)?;
            }
        }

        Ok(Self {
            key: raw.key,
            srcs: raw.srcs,
            enabled: raw.enabled,
        })
    }
}

/// Shareable (exportable and importable) specification of a
/// [`state::InputSrc`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InputSrc {
    /// Remote endpoint represented by its URL.
    RemoteUrl(state::InputSrcUrl),

    /// Yet another local [`Input`].
    LocalInput(Input),
}

/// Shareable (exportable and importable) specification of a [`state::Output`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Output {
    /// Downstream URL to re-stream a live stream onto.
    pub dst: state::OutputDstUrl,

    /// Optional label of this [`Output`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<state::Label>,

    /// Volume rate of this [`Output`]'s audio tracks when mixed with
    /// [`Output::mixins`].
    #[serde(default, skip_serializing_if = "state::Volume::is_origin")]
    pub volume: state::Volume,

    /// [`Mixin`]s to mix this [`Output`] with before re-streaming it to its
    /// downstream destination.
    ///
    /// If empty, then no mixing is performed.
    #[serde(
        default,
        deserialize_with = "Output::deserialize_mixins",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub mixins: Vec<Mixin>,

    /// Indicator whether this [`Output`]  is enabled, so is allowed to perform
    /// a live stream re-streaming to its downstream destination.
    #[serde(default, skip_serializing_if = "is_false")]
    pub enabled: bool,
}

impl Output {
    /// Deserializes [`Output::mixins`] ensuring its invariants preserved.
    fn deserialize_mixins<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<Mixin>, D::Error> {
        let mixins = Vec::deserialize(deserializer)?;

        if !mixins.is_empty() {
            let mut unique = HashSet::with_capacity(mixins.len());
            for m in &mixins {
                if let Some(src) = unique.replace(&m.src) {
                    return Err(D::Error::custom(format!(
                        "Duplicate Mixin.src in Output.mixins: {}",
                        src,
                    )));
                }
            }
        }

        Ok(mixins)
    }
}

/// Shareable (exportable and importable) specification of a [`state::Mixin`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Mixin {
    /// URL of the source to be mixed with an [`Output`].
    pub src: state::MixinSrcUrl,

    /// Volume rate of this [`Mixin`]'s audio tracks to mix them with.
    #[serde(default, skip_serializing_if = "Volume::is_origin")]
    pub volume: state::Volume,

    /// Delay that this [`Mixin`] should wait before being mixed with an
    /// [`Output`].
    #[serde(default, skip_serializing_if = "Delay::is_zero")]
    pub delay: state::Delay,
}
