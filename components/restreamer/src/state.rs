//! Application state.

use std::{
    borrow::Cow, convert::TryInto, future::Future, mem,
    panic::AssertUnwindSafe, path::Path, time::Duration,
};

use anyhow::anyhow;
use derive_more::{Deref, Display, From, Into};
use ephyr_log::log;
use futures::{
    future::TryFutureExt as _,
    sink,
    stream::{StreamExt as _, TryStreamExt as _},
};
use futures_signals::signal::{Mutable, SignalExt as _};
use juniper::{
    graphql_scalar, GraphQLEnum, GraphQLObject, GraphQLScalarValue,
    GraphQLUnion, ParseScalarResult, ParseScalarValue, ScalarValue, Value,
};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use smart_default::SmartDefault;
use tokio::{fs, io::AsyncReadExt as _};
use url::Url;
use uuid::Uuid;

use crate::{display_panic, serde::is_false, spec, srs, Spec};

impl State {
    /// Adds new [`Output`] to the specified [`Restream`] of this [`State`].
    ///
    /// Returns [`None`] if no [`Restream`] with `input_id` exists.
    #[must_use]
    pub fn add_new_output(
        &self,
        input_id: InputId,
        output_dst: OutputDstUrl,
        label: Option<Label>,
        mix_with: Option<MixinSrcUrl>,
    ) -> Option<bool> {
        let mut restreams = self.restreams.lock_mut();
        let outputs =
            &mut restreams.iter_mut().find(|r| r.id == input_id)?.outputs;

        if outputs.iter_mut().any(|o| o.dst == output_dst) {
            return Some(false);
        }

        outputs.push(Output {
            id: OutputId::random(),
            dst: output_dst,
            label,
            volume: Volume::ORIGIN,
            mixins: mix_with
                .map(|url| {
                    let delay = (url.scheme() == "ts")
                        .then(|| Delay::from_millis(3500))
                        .flatten()
                        .unwrap_or_default();
                    vec![Mixin {
                        id: MixinId::random(),
                        src: url,
                        volume: Volume::ORIGIN,
                        delay,
                    }]
                })
                .unwrap_or_default(),
            enabled: false,
            status: Status::Offline,
        });
        Some(true)
    }

    /// Removes [`Output`] from the specified [`Restream`] of this [`State`].
    ///
    /// Returns `true` if it has been removed, or `false` if doesn't exist.
    /// Returns [`None`] if no [`Restream`] with `input_id` exists.
    #[must_use]
    pub fn remove_output(
        &self,
        input_id: InputId,
        output_id: OutputId,
    ) -> Option<bool> {
        let mut restreams = self.restreams.lock_mut();
        let outputs =
            &mut restreams.iter_mut().find(|r| r.id == input_id)?.outputs;

        let prev_len = outputs.len();
        outputs.retain(|o| o.id != output_id);
        Some(outputs.len() != prev_len)
    }

    /// Enables [`Output`] in the specified [`Restream`] of this [`State`].
    ///
    /// Returns `true` if it has been enabled, or `false` if it already has been
    /// enabled.
    /// Returns [`None`] if no [`Restream`] with `input_id` exists.
    #[must_use]
    pub fn enable_output(
        &self,
        input_id: InputId,
        output_id: OutputId,
    ) -> Option<bool> {
        let mut restreams = self.restreams.lock_mut();
        let output = restreams
            .iter_mut()
            .find(|r| r.id == input_id)?
            .outputs
            .iter_mut()
            .find(|o| o.id == output_id)?;

        if output.enabled {
            return Some(false);
        }

        output.enabled = true;
        Some(true)
    }

    /// Disables [`Output`] in the specified [`Restream`] of this [`State`].
    ///
    /// Returns `true` if it has been disabled, or `false` if it already has
    /// been disabled.
    /// Returns [`None`] if no [`Restream`] with `input_id` exists.
    #[must_use]
    pub fn disable_output(
        &self,
        input_id: InputId,
        output_id: OutputId,
    ) -> Option<bool> {
        let mut restreams = self.restreams.lock_mut();
        let output = restreams
            .iter_mut()
            .find(|r| r.id == input_id)?
            .outputs
            .iter_mut()
            .find(|o| o.id == output_id)?;

        if !output.enabled {
            return Some(false);
        }

        output.enabled = false;
        Some(true)
    }

    /// Enables all [`Output`]s in the specified [`Restream`] of this [`State`].
    ///
    /// Returns `true` if at least one has been enabled, or `false` if all of
    /// them already have been enabled.
    /// Returns [`None`] if no [`Restream`] with `input_id` exists.
    #[must_use]
    pub fn enable_all_outputs(&self, input_id: InputId) -> Option<bool> {
        let mut restreams = self.restreams.lock_mut();
        Some(
            restreams
                .iter_mut()
                .find(|r| r.id == input_id)?
                .outputs
                .iter_mut()
                .filter(|o| !o.enabled)
                .fold(false, |_, o| {
                    o.enabled = true;
                    true
                }),
        )
    }

    /// Disables all [`Output`]s in the specified [`Restream`] of this
    /// [`State`].
    ///
    /// Returns `true` if at least one has been disabled, or `false` if all of
    /// them already have been disabled.
    /// Returns [`None`] if no [`Restream`] with `input_id` exists.
    #[must_use]
    pub fn disable_all_outputs(&self, input_id: InputId) -> Option<bool> {
        let mut restreams = self.restreams.lock_mut();
        Some(
            restreams
                .iter_mut()
                .find(|r| r.id == input_id)?
                .outputs
                .iter_mut()
                .filter(|o| o.enabled)
                .fold(false, |_, o| {
                    o.enabled = false;
                    true
                }),
        )
    }

    /// Tunes a [`Volume`] rate of the specified [`Output`] or its [`Mixin`] in
    /// this [`State`].
    ///
    /// Returns `true` if a [`Volume`] rate has been changed, or `false` if it
    /// has the same value already.
    /// Returns [`None`] if no [`Restream`] with `input_id` exists, or no
    /// [`Output`] with `output_id` exist, or no [`Mixin`] with `mixin_id`
    /// exists.
    #[must_use]
    pub fn tune_volume(
        &self,
        input_id: InputId,
        output_id: OutputId,
        mixin_id: Option<MixinId>,
        volume: Volume,
    ) -> Option<bool> {
        let mut restreams = self.restreams.lock_mut();
        let output = restreams
            .iter_mut()
            .find(|r| r.id == input_id)?
            .outputs
            .iter_mut()
            .find(|o| o.id == output_id)?;

        let curr_volume = if let Some(id) = mixin_id {
            &mut output.mixins.iter_mut().find(|m| m.id == id)?.volume
        } else {
            &mut output.volume
        };

        if *curr_volume == volume {
            return Some(false);
        }

        *curr_volume = volume;
        Some(true)
    }

    /// Tunes a [`Delay`] of the specified [`Mixin`] in this [`State`].
    ///
    /// Returns `true` if a [`Delay`] has been changed, or `false` if it has the
    /// same value already.
    /// Returns [`None`] if no [`Restream`] with `input_id` exists, or no
    /// [`Output`] with `output_id` exist, or no [`Mixin`] with `mixin_id`
    /// exists.
    #[must_use]
    pub fn tune_delay(
        &self,
        input_id: InputId,
        output_id: OutputId,
        mixin_id: MixinId,
        delay: Delay,
    ) -> Option<bool> {
        let mut restreams = self.restreams.lock_mut();
        let mixin = restreams
            .iter_mut()
            .find(|r| r.id == input_id)?
            .outputs
            .iter_mut()
            .find(|o| o.id == output_id)?
            .mixins
            .iter_mut()
            .find(|m| m.id == mixin_id)?;

        if mixin.delay == delay {
            return Some(false);
        }

        mixin.delay = delay;
        Some(true)
    }
}

impl Restream {
    /// Returns an URL of the remote server that this [`Restream`] receives a
    /// live stream from, if any.
    #[inline]
    #[must_use]
    pub fn upstream_url(&self) -> Option<&InputSrcUrl> {
        if let Input::Pull(i) = &self.input {
            Some(&i.src)
        } else {
            None
        }
    }

    /// Returns an URL of the local [SRS] server that the received live stream
    /// by this [`Restream`] may be pulled from.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    #[must_use]
    pub fn srs_url(&self) -> Url {
        Url::parse(&match &self.input {
            Input::Push(i) => format!("rtmp://127.0.0.1:1935/{}/in", i.name),
            Input::FailoverPush(i) => {
                format!("rtmp://127.0.0.1:1935/{}/in", i.name)
            }
            Input::Pull(_) => {
                format!("rtmp://127.0.0.1:1935/pull_{}/in", self.id)
            }
        })
        .unwrap()
    }

    /// Checks whether the given `app` parameter of a [SRS] media stream is
    /// related to this [`Restream::input`].
    ///
    /// [SRS]: https://github.com/ossrs/srs
    #[inline]
    #[must_use]
    pub fn uses_srs_app(&self, app: &str) -> bool {
        match &self.input {
            Input::Push(i) => i.name == *app,
            Input::FailoverPush(i) => i.name == *app,
            Input::Pull(_) => {
                app.starts_with("pull_") && app[5..].parse() == Ok(self.id.0)
            }
        }
    }
}

impl Input {
    /// Indicates whether this [`Input`] has [`Status::Online`].
    #[must_use]
    pub fn is_online(&self) -> bool {
        match self {
            Self::Push(i) => i.status == Status::Online,
            Self::FailoverPush(i) => {
                // If `/in` endpoint goes offline, but there is still `/main`
                // or `/backup` endpoint is online, then failover switch is
                // happening at the moment, so we consider tha the whole `Input`
                // is still online.
                i.status == Status::Online
                    || i.main_status == Status::Online
                    || i.backup_status == Status::Online
            }
            Self::Pull(i) => i.status == Status::Online,
        }
    }

    /// Sets a `new` [`Status`] of this [`Input`].
    #[inline]
    pub fn set_status(&mut self, new: Status) {
        match self {
            Self::Push(i) => i.status = new,
            Self::FailoverPush(i) => i.status = new,
            Self::Pull(i) => i.status = new,
        }
    }

    /// Checks whether this [`Input`] is the same as `other` one.
    #[inline]
    #[must_use]
    pub fn is(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Push(a), Self::Push(b)) => a.is(b),
            (Self::FailoverPush(a), Self::FailoverPush(b)) => a.is(b),
            (Self::Pull(a), Self::Pull(b)) => a.is(b),
            _ => false,
        }
    }
}

/// `Input` pulling a live RTMP stream from a remote server.
#[derive(
    Clone, Debug, Deserialize, Eq, GraphQLObject, PartialEq, Serialize,
)]
pub struct PullInput {
    /// URL of a live stream to be pulled from.
    ///
    /// At the moment only [RTMP] is supported.
    ///
    /// [RTMP]: https://en.wikipedia.org/wiki/Real-Time_Messaging_Protocol
    pub src: InputSrcUrl,

    /// `Status` of this `PullInput` indicating whether it performs pulling.
    #[serde(skip)]
    pub status: Status,
}

/// `Input` receiving a live RTMP stream from a remote client on an `/in`
/// endpoint.
#[derive(
    Clone, Debug, Deserialize, Eq, GraphQLObject, PartialEq, Serialize,
)]
pub struct PushInput {
    /// Name of a live stream to expose its endpoints with for receiving and
    /// re-streaming media traffic.
    pub name: InputName,

    /// `Status` of this `PushInput` indicating whether it receives media
    /// traffic.
    #[serde(skip)]
    pub status: Status,
}

/// `Input` receiving a live RTMP stream from a remote client on two endpoints:
/// - `/main` for receiving a main live RTMP stream, once it goes offline the
///   `Input` switches to `/backup`;
/// - `/backup` for receiving a backup live RTMP stream, once `/main` is
///   restored the `Input` switches back to `/main`.
#[derive(
    Clone, Debug, Deserialize, Eq, GraphQLObject, PartialEq, Serialize,
)]
pub struct FailoverPushInput {
    /// Name of a live RTMP stream to expose it with for receiving and
    /// re-streaming media traffic.
    pub name: InputName,

    /// `Status` of a `/main` endpoint indicating whether it receives media
    /// traffic.
    #[serde(skip)]
    pub main_status: Status,

    /// ID of [SRS] client who publishes the ongoing live RTMP stream to
    /// a `/main` endpoint of this [`FailoverPushInput`].
    ///
    /// [SRS]: https://github.com/ossrs/srs
    #[graphql(skip)]
    #[serde(skip)]
    pub main_srs_publisher_id: Option<srs::ClientId>,

    /// `Status` of a `/backup` endpoint indicating whether it receives media
    /// traffic.
    #[serde(skip)]
    pub backup_status: Status,

    /// ID of [SRS] client who publishes the ongoing live RTMP stream to
    /// a `/backup` endpoint of this [`FailoverPushInput`].
    ///
    /// [SRS]: https://github.com/ossrs/srs
    #[graphql(skip)]
    #[serde(skip)]
    pub backup_srs_publisher_id: Option<srs::ClientId>,

    /// Overall `Status` of this `FailoverPushInput` indicating whether a
    /// received media traffic is available.
    #[serde(skip)]
    pub status: Status,
}

impl FailoverPushInput {
    /// Returns a local [SRS] URL of the currently active `/main` or `/backup`
    /// endpoint.
    ///
    /// It returns `/backup` if it's online while `/main` is not, otherwise
    /// returns `/main`.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    #[must_use]
    pub fn from_url(&self) -> Url {
        Url::parse(
            &(if self.backup_status == Status::Online
                && self.main_status != Status::Online
            {
                format!("rtmp://127.0.0.1:1935/{}/backup", self.name)
            } else {
                format!("rtmp://127.0.0.1:1935/{}/main", self.name)
            }),
        )
        .unwrap()
    }

    /// Indicates whether this [`FailoverPushInput`] receives any media traffic
    /// on its `/main` or `/backup` endpoint.
    #[inline]
    #[must_use]
    pub fn has_traffic(&self) -> bool {
        self.main_status == Status::Online
            || self.backup_status == Status::Online
    }
}

////////////////////////////////////////////////////////////////////////////////

/// Reactive application's state.
///
/// Any changes to it automatically propagate to the appropriate subscribers.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct State {
    /// [`argon2`] hash of password which protects access to this application's
    /// public APIs.
    pub password_hash: Mutable<Option<String>>,

    /// All [`Restream`]s performed by this application.
    pub restreams: Mutable<Vec<Restream>>,
}

impl State {
    /// Instantiates a new [`State`] reading it from a `file` (if any) and
    /// performing all the required inner subscriptions.
    ///
    /// # Errors
    ///
    /// If [`State`] file exists, but fails to be parsed.
    pub async fn try_new<P: AsRef<Path>>(
        file: P,
    ) -> Result<Self, anyhow::Error> {
        let file = file.as_ref();

        let mut contents = vec![];
        let _ = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .read(true)
            .open(&file)
            .await
            .map_err(|e| {
                anyhow!("Failed to open '{}' file: {}", file.display(), e)
            })?
            .read_to_end(&mut contents)
            .await
            .map_err(|e| {
                anyhow!("Failed to read '{}' file: {}", file.display(), e)
            })?;

        let state = if contents.is_empty() {
            State::default()
        } else {
            serde_json::from_slice(&contents).map_err(|e| {
                anyhow!(
                    "Failed to deserialize state from '{}' file: {}",
                    file.display(),
                    e,
                )
            })?
        };

        let (file, persisted_state) = (file.to_owned(), state.clone());
        let persist_state1 = move || {
            fs::write(
                file.clone(),
                serde_json::to_vec(&persisted_state)
                    .expect("Failed to serialize server state"),
            )
            .map_err(|e| log::error!("Failed to persist server state: {}", e))
        };
        let persist_state2 = persist_state1.clone();
        Self::on_change("persist_restreams", &state.restreams, move |_| {
            persist_state1()
        });
        Self::on_change(
            "persist_password_hash",
            &state.password_hash,
            move |_| persist_state2(),
        );

        Ok(state)
    }

    /// Applies the given [`Spec`] to this [`State`].
    ///
    /// If `replace` is `true` then all the [`Restream`]s, [`Restream::outputs`]
    /// and [`Output::mixins`] will be replaced with new ones, otherwise new
    /// ones will be merged with already existing ones.
    pub fn apply(&self, new: Spec, replace: bool) {
        let new = new.into_v1();
        let mut restreams = self.restreams.lock_mut();
        if replace {
            let mut olds = mem::replace(
                &mut *restreams,
                Vec::with_capacity(new.restreams.len()),
            );
            for new in new.restreams {
                if let Some(mut old) = olds
                    .iter()
                    .enumerate()
                    .find_map(|(n, o)| (o.key == new.key).then(|| n))
                    .map(|n| olds.swap_remove(n))
                {
                    old.apply(new, replace);
                    restreams.push(old);
                } else {
                    restreams.push(Restream::new(new));
                }
            }
        } else {
            for new in new.restreams {
                if let Some(old) =
                    restreams.iter_mut().find(|o| o.key == new.key)
                {
                    old.apply(new, replace);
                } else {
                    restreams.push(Restream::new(new));
                }
            }
        }
    }

    /// Exports this [`State`] as a [`spec::v1::Spec`].
    #[inline]
    #[must_use]
    pub fn export(&self) -> Spec {
        spec::v1::Spec {
            restreams: self
                .restreams
                .get_cloned()
                .iter()
                .map(Restream::export)
                .collect(),
        }
        .into()
    }

    /// Subscribes the specified `hook` to changes of the [`Mutable`] `val`ue.
    ///
    /// `name` is just a convenience for describing the `hook` in logs.
    pub fn on_change<F, Fut, T>(name: &'static str, val: &Mutable<T>, hook: F)
    where
        F: FnMut(T) -> Fut + Send + 'static,
        Fut: Future + Send + 'static,
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        drop(tokio::spawn(
            AssertUnwindSafe(
                val.signal_cloned().dedupe_cloned().to_stream().then(hook),
            )
            .catch_unwind()
            .map_err(move |p| {
                log::crit!(
                    "Panicked executing `{}` hook of state: {}",
                    name,
                    display_panic(&p),
                )
            })
            .map(|_| Ok(()))
            .forward(sink::drain()),
        ));
    }

    /// Adds a new [`Restream`] by the given `spec` to this [`State`].
    ///
    /// # Errors
    ///
    /// If this [`State`] has a [`Restream`] with such `key` already.
    pub fn add_restream(&self, spec: spec::v1::Restream) -> anyhow::Result<()> {
        let mut restreams = self.restreams.lock_mut();

        if restreams.iter().find(|r| r.key == spec.key).is_some() {
            return Err(anyhow!("Restream.key '{}' is used already", spec.key));
        }

        restreams.push(Restream::new(spec));
        OK(())
    }

    /// Edits a [`Restream`] this [`State`] identified by the given `spec`.
    ///
    /// Returns [`None`] if there is no [`Restream`] with such `key` in this
    /// [`State`].
    ///
    /// # Errors
    ///
    /// If this [`State`] has a [`Restream`] with such `key` already.
    pub fn edit_restream(
        &self,
        id: RestreamId,
        spec: spec::v1::Restream,
    ) -> anyhow::Result<Option<()>> {
        let mut restreams = self.restreams.lock_mut();

        if restreams
            .iter()
            .find(|r| r.key == spec.key && r.id != id)
            .is_some()
        {
            return Err(anyhow!("Restream.key '{}' is used already", spec.key));
        }

        Ok(restreams
            .iter_mut()
            .find(|r| r.id == id)
            .map(|r| r.apply(spec, false)))
    }

    /// Removes a [`Restream`] with the given `id` from this [`State`].
    ///
    /// Returns [`None`] if there is no [`Restream`] with such `key` in this
    /// [`State`].
    pub fn remove_restream(&self, id: RestreamId) -> Option<()> {
        let mut restreams = self.restreams.lock_mut();
        let prev_len = restreams.len();
        restreams.retain(|r| r.id != id);
        (restreams.len() != prev_len).then(|| ())
    }

    /// Enables a [`Restream`] with the given `id` in this [`State`].
    ///
    /// Returns `true` if it has been enabled, or `false` if it already has been
    /// enabled, or [`None`] if it doesn't exist.
    #[must_use]
    pub fn enable_restream(&self, id: RestreamId) -> Option<bool> {
        self.restreams
            .lock_mut()
            .iter_mut()
            .find(|r| r.id == id)
            .map(|r| r.input.enable())
    }

    /// Disables a [`Restream`] with the given `id` in this [`State`].
    ///
    /// Returns `true` if it has been disabled, or `false` if it already has
    /// been disabled, or [`None`] if it doesn't exist.
    #[must_use]
    pub fn disable_restream(&self, id: RestreamId) -> Option<bool> {
        self.restreams
            .lock_mut()
            .iter_mut()
            .find(|r| r.id == id)
            .map(|r| r.input.disable())
    }
}

/// Re-stream of a live stream from one `Input` to many `Output`s.
#[derive(
    Clone, Debug, Deserialize, Eq, GraphQLObject, PartialEq, Serialize,
)]
pub struct Restream {
    /// Unique ID of this `Input`.
    ///
    /// Once assigned, it never changes.
    pub id: RestreamId,

    /// Unique key of this `Restream` identifying it, and used to form its
    /// endpoints URLs.
    pub key: RestreamKey,

    /// Optional label of this `Restream`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,

    /// `Input` that a live stream is received from.
    pub input: Input,

    /// `Output`s that a live stream is re-streamed to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Output>,
}

impl Restream {
    /// Creates a new [`Restream`] out of the given [`spec::v1::Restream`].
    #[inline]
    #[must_use]
    pub fn new(spec: spec::v1::Restream) -> Self {
        Self {
            id: RestreamId::random(),
            key: spec.key,
            label: spec.label,
            input: Input::new(spec.input),
            outputs: spec.outputs.into_iter().map(Output::new).collect(),
        }
    }

    /// Applies the given [`spec::v1::Restream`] to this [`Restream`].
    ///
    /// If `replace` is `true` then all the [`Restream::outputs`] will be
    /// replaced with new ones, otherwise new ones will be merged with already
    /// existing [`Restream::outputs`].
    pub fn apply(&mut self, new: spec::v1::Restream, replace: bool) {
        self.key = new.key;
        self.label = new.label;
        self.input.apply(new.input);
        if replace {
            let mut olds = mem::replace(
                &mut self.outputs,
                Vec::with_capacity(new.outputs.len()),
            );
            for new in new.outputs {
                if let Some(mut old) = olds
                    .iter()
                    .enumerate()
                    .find_map(|(n, o)| (o.dst == new.dst).then(|| n))
                    .map(|n| olds.swap_remove(n))
                {
                    old.apply(new, replace);
                    self.outputs.push(old);
                } else {
                    self.outputs.push(Output::new(new));
                }
            }
        } else {
            for new in new.outputs {
                if let Some(old) =
                    self.outputs.iter_mut().find(|o| o.dst == new.dst)
                {
                    old.apply(new, replace);
                } else {
                    self.outputs.push(Output::new(new));
                }
            }
        }
    }

    /// Exports this [`Restream`] as a [`spec::v1::Restream`].
    #[inline]
    #[must_use]
    pub fn export(&self) -> spec::v1::Restream {
        spec::v1::Restream {
            key: self.key.clone(),
            label: self.label.clone(),
            input: self.input.export(),
            outputs: self.outputs.iter().map(Output::export).collect(),
        }
    }
}

/// ID of a `Restream`.
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Display,
    Eq,
    From,
    GraphQLScalarValue,
    Into,
    PartialEq,
    Serialize,
)]
pub struct RestreamId(Uuid);

impl RestreamId {
    /// Generates a new random [`RestreamId`].
    #[inline]
    #[must_use]
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Key of a [`Restream`] identifying it, and used to form its endpoints URLs.
#[derive(Clone, Debug, Deref, Display, Eq, Into, PartialEq, Serialize)]
pub struct RestreamKey(String);

impl RestreamKey {
    /// Creates a new [`RestreamKey`] if the given value meets its invariants.
    #[must_use]
    pub fn new<'s, S: Into<Cow<'s, str>>>(val: S) -> Option<Self> {
        static REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new("^[a-z0-9_-]{1,20}$").unwrap());

        let val = val.into();
        (!val.is_empty() && REGEX.is_match(&val))
            .then(|| Self(val.into_owned()))
    }
}

impl<'de> Deserialize<'de> for RestreamKey {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(<Cow<'_, str>>::deserialize(deserializer)?)
            .ok_or_else(|| D::Error::custom("Not a valid Restream.key"))
    }
}

/// Type of `Restream`'s `key` identifying it, and used to form its endpoints
/// URLs.
///
/// It should meet `[a-z0-9_-]{1,20}` format.
#[graphql_scalar]
impl<S> GraphQLScalar for RestreamKey
where
    S: ScalarValue,
{
    fn resolve(&self) -> Value {
        Value::scalar(self.0.as_str().to_owned())
    }

    fn from_input_value(v: &InputValue) -> Option<Self> {
        v.as_scalar()
            .and_then(ScalarValue::as_str)
            .and_then(Self::new)
    }

    fn from_str(value: ScalarToken<'_>) -> ParseScalarResult<'_, S> {
        <String as ParseScalarValue<S>>::from_str(value)
    }
}

impl PartialEq<str> for RestreamKey {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

/// Upstream source that a `Restream` receives a live stream from.
#[derive(
    Clone, Debug, Deserialize, Eq, GraphQLObject, PartialEq, Serialize,
)]
pub struct Input {
    /// Unique ID of this `Input`.
    ///
    /// Once assigned, it never changes.
    pub id: InputId,

    /// Key of this `Input` to expose its endpoint with for accepting and
    /// serving a live stream.
    pub key: InputKey,

    /// Sources to pull a live stream from.
    ///
    /// If specified, then this `Input` will pull a live stream from it (pull
    /// kind), otherwise this `Input` will await a live stream to be pushed
    /// (push kind).
    ///
    /// If multiple sources are specified, then the first one will be attempted
    /// falling back to the second one, and so on. Once the first source is
    /// restored, this `Input` switches back to it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub srcs: Vec<InputSrc>,

    /// Indicator whether this `Input` is enabled, so is allowed to receive a
    /// live stream from its upstream sources.
    #[serde(default, skip_serializing_if = "is_false")]
    pub enabled: bool,

    /// `Status` of this `Input` indicating whether it actually serves a live
    /// stream ready to be consumed by `Output`s.
    #[serde(skip)]
    pub status: Status,

    /// ID of a [SRS] client who publishes a live stream to this [`Input`]
    /// (either an external client or a local process).
    ///
    /// [SRS]: https://github.com/ossrs/srs
    #[graphql(skip)]
    #[serde(skip)]
    pub srs_publisher_id: Option<srs::ClientId>,

    /// IDs of [SRS]s client who plays a live stream from this [`Input`] (either
    /// an external client or a local process).
    ///
    /// [SRS]: https://github.com/ossrs/srs
    #[graphql(skip)]
    #[serde(skip)]
    pub srs_player_ids: Vec<srs::ClientId>,
}

impl Input {
    /// Creates a new [`Input`] out of the given [`spec::v1::Input`].
    #[inline]
    #[must_use]
    pub fn new(spec: spec::v1::Input) -> Self {
        Self {
            id: InputId::random(),
            key: spec.key,
            srcs: spec.srcs.into_iter().map(InputSrc::new).collect(),
            enabled: spec.enabled,
            status: Status::Offline,
            srs_publisher_id: None,
            srs_player_ids: vec![]
        }
    }

    /// Applies the given [`spec::v1::Input`] to this [`Input`].
    ///
    /// Replaces all the [`Input::srcs`] with new ones.
    pub fn apply(&mut self, new: spec::v1::Input) {
        if self.key != new.key
            || (self.srcs.is_empty() && !new.srcs.is_empty())
            || (!self.srcs.is_empty() && new.srcs.is_empty())
        {
            // SRS endpoint has changed, or push/pull type has been switched, so
            // we should kick the publisher.
            self.srs_publisher_id = None;

        }
        if self.key != new.key {
            // SRS endpoint has changed, so we should kick all the players.
            self.srs_player_ids = vec![];
        }

        self.key = new.key;
        self.enabled = new.enabled;

        let mut olds =
            mem::replace(&mut self.srcs, Vec::with_capacity(new.srcs.len()));
        for new in new.srcs {
            if let Some(mut old) = olds
                .iter()
                .enumerate()
                .find_map(|(n, s)| s.identified_by(&new).then(|| n))
                .map(|n| olds.swap_remove(n))
            {
                old.apply(new);
                self.srcs.push(old);
            } else {
                self.srcs.push(InputSrc::new(new));
            }
        }
    }

    /// Exports this [`Input`] as a [`spec::v1::Input`].
    #[inline]
    #[must_use]
    pub fn export(&self) -> spec::v1::Input {
        spec::v1::Input {
            key: self.key.clone(),
            srcs: self.srcs.iter().map(InputSrc::export).collect(),
            enabled: self.enabled,
        }
    }

    /// Enables this [`Input`].
    ///
    /// Returns `false` if it has been enabled already.
    #[must_use]
    pub fn enable(&mut self) -> bool {
        let mut changed = !self.enabled;

        self.enabled = true;

        for src in &mut self.srcs {
            if let Some(InputSrc::Local(input)) = src {
                changed |= input.enable();
            }
        }

        changed
    }

    /// Disables this [`Input`].
    ///
    /// Returns `false` if it has been disabled already.
    #[must_use]
    pub fn disable(&mut self) -> bool {
        let mut changed = self.enabled;

        self.enabled = false;
        self.srs_publisher_id = None;
        self.srs_player_ids = vec![];

        for src in &mut self.srcs {
            if let Some(InputSrc::Local(input)) = src {
                changed |= input.disable();
            }
        }

        changed
    }
}

/// Source to pull a live stream by an `Input` from.
#[derive(
    Clone, Debug, Deserialize, Eq, From, GraphQLUnion, PartialEq, Serialize,
)]
#[serde(rename_all = "lowercase")]
pub enum InputSrc {
    /// Remote endpoint.
    Remote(RemoteInputSrc),

    /// Yet another local `Input`.
    Local(Input),
}

impl InputSrc {
    /// Creates a new [`InputSrc`] out of the given [`spec::v1::InputSrc`].
    #[inline]
    #[must_use]
    pub fn new(spec: spec::v1::InputSrc) -> Self {
        match spec {
            spec::v1::InputSrc::RemoteUrl(url) => {
                Self::Remote(RemoteInputSrc { url })
            }
            spec::v1::InputSrc::LocalInput(i) => Self::Local(Input::new(i)),
        }
    }

    /// Applies the given [`spec::v1::InputSrc`] to this [`InputSrc`].
    #[inline]
    pub fn apply(&mut self, new: spec::v1::InputSrc) {
        match (self, new) {
            (Self::Remote(old), spec::v1::InputSrc::RemoteUrl(new_url)) => {
                old.url = new_url
            }
            (Self::Local(old), spec::v1::InputSrc::LocalInput(new)) => {
                old.apply(new)
            }
            (old, new) => *old = Self::new(new),
        }
    }

    /// Exports this [`InputSrc`] as a [`spec::v1::InputSrc`].
    #[inline]
    #[must_use]
    pub fn export(&self) -> spec::v1::InputSrc {
        match self {
            Self::Remote(i) => spec::v1::InputSrc::RemoteUrl(i.url.clone()),
            Self::Local(i) => spec::v1::InputSrc::LocalInput(i.export()),
        }
    }

    /// Checks whether this [`InputSrc`] is identified by the provided
    /// [`spec::v1::InputSrc`].
    #[inline]
    #[must_use]
    pub fn identified_by(&self, spec: &spec::v1::InputSrc) -> bool {
        match (self, spec) {
            (Self::Remote(a), spec::v1::InputSrc::RemoteUrl(b_url)) => {
                a.url == b_url
            }
            (Self::Local(a), spec::v1::InputSrc::LocalInput(b)) => {
                a.key == b.key
            }
            _ => false,
        }
    }
}

/// Remote upstream source to pull a live stream by an `Input` from.
#[derive(
    Clone, Debug, Deserialize, Eq, GraphQLObject, PartialEq, Serialize,
)]
pub struct RemoteInputSrc {
    /// URL of this `RemoteInputSrc`.
    pub url: InputSrcUrl,
}

/// ID of an `Input`.
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Display,
    Eq,
    From,
    GraphQLScalarValue,
    Into,
    PartialEq,
    Serialize,
)]
pub struct InputId(Uuid);

impl InputId {
    /// Generates a new random [`InputId`].
    #[inline]
    #[must_use]
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }
}

/// Key of an [`Input`] used to form its endpoint URL.
#[derive(Clone, Debug, Deref, Display, Eq, Into, PartialEq, Serialize)]
pub struct InputKey(String);

impl InputKey {
    /// Creates a new [`InputKey`] if the given value meets its invariants.
    #[must_use]
    pub fn new<'s, S: Into<Cow<'s, str>>>(val: S) -> Option<Self> {
        static REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new("^[a-z0-9_-]{1,20}$").unwrap());

        let val = val.into();
        (!val.is_empty() && REGEX.is_match(&val))
            .then(|| Self(val.into_owned()))
    }
}

impl<'de> Deserialize<'de> for InputKey {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(<Cow<'_, str>>::deserialize(deserializer)?)
            .ok_or_else(|| D::Error::custom("Not a valid Input.key"))
    }
}

/// Type of `Input`'s `key` used to form its endpoint URL.
///
/// It should meet `[a-z0-9_-]{1,20}` format.
#[graphql_scalar]
impl<S> GraphQLScalar for InputKey
where
    S: ScalarValue,
{
    fn resolve(&self) -> Value {
        Value::scalar(self.0.as_str().to_owned())
    }

    fn from_input_value(v: &InputValue) -> Option<Self> {
        v.as_scalar()
            .and_then(ScalarValue::as_str)
            .and_then(Self::new)
    }

    fn from_str(value: ScalarToken<'_>) -> ParseScalarResult<'_, S> {
        <String as ParseScalarValue<S>>::from_str(value)
    }
}

impl PartialEq<str> for InputKey {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

/// [`Url`] of a [`RemoteInputSrc`].
///
/// Only [RTMP] URLs are allowed at the moment.
///
/// [RTMP]: https://en.wikipedia.org/wiki/Real-Time_Messaging_Protocol
#[derive(Clone, Debug, Deref, Display, Eq, Into, PartialEq, Serialize)]
pub struct InputSrcUrl(Url);

impl InputSrcUrl {
    /// Creates a new [`InputSrcUrl`] if the given [`Url`] is suitable for that.
    #[inline]
    #[must_use]
    pub fn new(url: Url) -> Option<Self> {
        (matches!(url.scheme(), "rtmp" | "rtmps") && url.host().is_some())
            .then(|| Self(url))
    }
}

impl<'de> Deserialize<'de> for InputSrcUrl {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Url::deserialize(deserializer)?)
            .ok_or_else(|| D::Error::custom("Not a valid RemoteInputSrc.url"))
    }
}

/// Type of a `RemoteInputSrc.url`.
///
/// Only [RTMP] URLs are allowed at the moment.
///
/// [RTMP]: https://en.wikipedia.org/wiki/Real-Time_Messaging_Protocol
#[graphql_scalar]
impl<S> GraphQLScalar for InputSrcUrl
where
    S: ScalarValue,
{
    fn resolve(&self) -> Value {
        Value::scalar(self.0.as_str().to_owned())
    }

    fn from_input_value(v: &InputValue) -> Option<Self> {
        v.as_scalar()
            .and_then(ScalarValue::as_str)
            .and_then(|s| Url::parse(s).ok())
            .and_then(Self::new)
    }

    fn from_str(value: ScalarToken<'_>) -> ParseScalarResult<'_, S> {
        <String as ParseScalarValue<S>>::from_str(value)
    }
}

/// Downstream destination that a `Restream` re-streams a live stream to.
#[derive(
    Clone, Debug, Deserialize, Eq, GraphQLObject, PartialEq, Serialize,
)]
pub struct Output {
    /// Unique ID of this `Output`.
    ///
    /// Once assigned, it never changes.
    pub id: OutputId,

    /// Downstream URL to re-stream a live stream onto.
    ///
    /// At the moment only [RTMP] and [Icecast] are supported.
    ///
    /// [Icecast]: https://icecast.org
    /// [RTMP]: https://en.wikipedia.org/wiki/Real-Time_Messaging_Protocol
    pub dst: OutputDstUrl,

    /// Optional label of this `Output`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,

    /// Volume rate of this `Output`'s audio tracks when mixed with
    /// `Output.mixins`.
    ///
    /// Has no effect when there is no `Output.mixins`.
    #[serde(default, skip_serializing_if = "Volume::is_origin")]
    pub volume: Volume,

    /// `Mixin`s to mix this `Output` with before re-streaming it to its
    /// downstream destination.
    ///
    /// If empty, then no mixing is performed and re-streaming is as cheap as
    /// possible (just copies bytes "as is").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixins: Vec<Mixin>,

    /// Indicator whether this `Output` is enabled, so is allowed to perform a
    /// live stream re-streaming to its downstream destination.
    #[serde(default, skip_serializing_if = "is_false")]
    pub enabled: bool,

    /// `Status` of this `Output` indicating whether it actually re-streams a
    /// live stream to its downstream destination.
    #[serde(skip)]
    pub status: Status,
}

impl Output {
    /// Creates a new [`Output`] out of the given [`spec::v1::Output`].
    #[inline]
    #[must_use]
    pub fn new(spec: spec::v1::Output) -> Self {
        Self {
            id: OutputId::random(),
            dst: spec.dst,
            label: spec.label,
            volume: spec.volume,
            mixins: spec.mixins.into_iter().map(Mixin::new).collect(),
            enabled: spec.enabled,
            status: Status::Offline,
        }
    }

    /// Applies the given [`spec::v1::Output`] to this [`Output`].
    ///
    /// If `replace` is `true` then all the [`Output::mixins`] will be replaced
    /// with new ones, otherwise new ones will be merged with already existing
    /// [`Output::mixins`].
    pub fn apply(&mut self, new: spec::v1::Output, replace: bool) {
        self.dst = new.dst;
        self.label = new.label;
        self.volume = new.volume;
        self.enabled = new.enabled;
        if replace {
            let mut olds = mem::replace(
                &mut self.mixins,
                Vec::with_capacity(new.mixins.len()),
            );
            for new in new.mixins {
                if let Some(mut old) = olds
                    .iter()
                    .enumerate()
                    .find_map(|(n, o)| (o.src == new.src).then(|| n))
                    .map(|n| olds.swap_remove(n))
                {
                    old.apply(new);
                    self.mixins.push(old);
                } else {
                    self.mixins.push(Mixin::new(new));
                }
            }
        } else {
            for new in new.mixins {
                if let Some(old) =
                    self.mixins.iter_mut().find(|o| o.src == new.src)
                {
                    old.apply(new);
                } else {
                    self.mixins.push(Mixin::new(new));
                }
            }
        }
    }

    /// Exports this [`Output`] as a [`spec::v1::Output`].
    #[inline]
    #[must_use]
    pub fn export(&self) -> spec::v1::Output {
        spec::v1::Output {
            dst: self.dst.clone(),
            label: self.label.clone(),
            volume: self.volume,
            mixins: self.mixins.iter().map(Mixin::export).collect(),
            enabled: self.enabled,
        }
    }
}

/// ID of an `Output`.
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Display,
    Eq,
    From,
    GraphQLScalarValue,
    Into,
    PartialEq,
    Serialize,
)]
pub struct OutputId(Uuid);

impl OutputId {
    /// Generates a new random [`OutputId`].
    #[inline]
    #[must_use]
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }
}

/// [`Url`] of an [`Output::dst`].
///
/// Only [RTMP] and [Icecast] URLs are allowed at the moment.
///
/// [Icecast]: https://icecast.org
/// [RTMP]: https://en.wikipedia.org/wiki/Real-Time_Messaging_Protocol
#[derive(Clone, Debug, Deref, Display, Eq, Into, PartialEq, Serialize)]
pub struct OutputDstUrl(Url);

impl OutputDstUrl {
    /// Creates a new [`OutputDstUrl`] if the given [`Url`] is suitable for
    /// that.
    #[inline]
    #[must_use]
    pub fn new(url: Url) -> Option<Self> {
        (matches!(url.scheme(), "icecast" | "rtmp" | "rtmps")
            && url.host().is_some())
        .then(|| Self(url))
    }
}

impl<'de> Deserialize<'de> for OutputDstUrl {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Url::deserialize(deserializer)?)
            .ok_or_else(|| D::Error::custom("Not a valid Output.dst URL"))
    }
}

/// Type of an `Output.dst` URL.
///
/// Only [RTMP] and [Icecast] URLs are allowed at the moment.
///
/// [Icecast]: https://icecast.org
/// [RTMP]: https://en.wikipedia.org/wiki/Real-Time_Messaging_Protocol
#[graphql_scalar]
impl<S> GraphQLScalar for OutputDstUrl
where
    S: ScalarValue,
{
    fn resolve(&self) -> Value {
        Value::scalar(self.0.as_str().to_owned())
    }

    fn from_input_value(v: &InputValue) -> Option<Self> {
        v.as_scalar()
            .and_then(ScalarValue::as_str)
            .and_then(|s| Url::parse(s).ok())
            .and_then(Self::new)
    }

    fn from_str(value: ScalarToken<'_>) -> ParseScalarResult<'_, S> {
        <String as ParseScalarValue<S>>::from_str(value)
    }
}

/// Additional source for an `Output` to be mixed with before re-streaming to
/// the destination.
#[derive(
    Clone, Debug, Deserialize, Eq, GraphQLObject, PartialEq, Serialize,
)]
pub struct Mixin {
    /// Unique ID of this `Mixin`.
    ///
    /// Once assigned, it never changes.
    pub id: MixinId,

    /// URL of the source to be mixed with an `Output`.
    ///
    /// At the moment, only [TeamSpeak] is supported.
    ///
    /// [TeamSpeak]: https://teamspeak.com
    pub src: MixinSrcUrl,

    /// Volume rate of this `Mixin`'s audio tracks to mix them with.
    #[serde(default, skip_serializing_if = "Volume::is_origin")]
    pub volume: Volume,

    /// Delay that this `Mixin` should wait before being mixed with an `Output`.
    ///
    /// Very useful to fix de-synchronization issues and correct timings between
    /// a `Mixin` and its `Output`.
    #[serde(default, skip_serializing_if = "Delay::is_zero")]
    pub delay: Delay,

    /// `Status` of this `Mixin` indicating whether it provides an actual media
    /// stream to be mixed with its `Output`.
    #[serde(skip)]
    pub status: Status,
}

impl Mixin {
    /// Creates a new [`Mixin`] out of the given [`spec::v1::Mixin`].
    #[inline]
    #[must_use]
    pub fn new(spec: spec::v1::Mixin) -> Self {
        Self {
            id: MixinId::random(),
            src: spec.src,
            volume: spec.volume,
            delay: spec.delay,
            status: Status::Offline,
        }
    }

    /// Applies the given [`spec::v1::Mixin`] to this [`Mixin`].
    #[inline]
    pub fn apply(&mut self, new: spec::v1::Mixin) {
        self.src = new.src;
        self.volume = new.volume;
        self.delay = new.delay;
    }

    /// Exports this [`Mixin`] as a [`spec::v1::Mixin`].
    #[inline]
    #[must_use]
    pub fn export(&self) -> spec::v1::Mixin {
        spec::v1::Mixin {
            src: self.src.clone(),
            volume: self.volume,
            delay: self.delay,
        }
    }
}

/// ID of a `Mixin`.
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Display,
    Eq,
    From,
    GraphQLScalarValue,
    Into,
    PartialEq,
    Serialize,
)]
pub struct MixinId(Uuid);

impl MixinId {
    /// Generates a new random [`MixinId`].
    #[inline]
    #[must_use]
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }
}

/// [`Url`] of a [`Mixin::src`].
///
/// Only [TeamSpeak] URLs are allowed at the moment.
///
/// [TeamSpeak]: https://teamspeak.com
#[derive(Clone, Debug, Deref, Display, Eq, Into, PartialEq, Serialize)]
pub struct MixinSrcUrl(Url);

impl MixinSrcUrl {
    /// Creates a new [`MixinSrcUrl`] if the given [`Url`] is suitable for that.
    #[inline]
    #[must_use]
    pub fn new(url: Url) -> Option<Self> {
        (url.scheme() == "ts" && url.host().is_some()).then(|| Self(url))
    }
}

impl<'de> Deserialize<'de> for MixinSrcUrl {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(Url::deserialize(deserializer)?)
            .ok_or_else(|| D::Error::custom("Not a valid Mixin.src URL"))
    }
}

/// Type of a `Mixin.src` URL.
///
/// Only [TeamSpeak] URLs are allowed at the moment.
///
/// [TeamSpeak]: https://teamspeak.com
#[graphql_scalar]
impl<S> GraphQLScalar for MixinSrcUrl
where
    S: ScalarValue,
{
    fn resolve(&self) -> Value {
        Value::scalar(self.0.as_str().to_owned())
    }

    fn from_input_value(v: &InputValue) -> Option<Self> {
        v.as_scalar()
            .and_then(ScalarValue::as_str)
            .and_then(|s| Url::parse(s).ok())
            .and_then(Self::new)
    }

    fn from_str(value: ScalarToken<'_>) -> ParseScalarResult<'_, S> {
        <String as ParseScalarValue<S>>::from_str(value)
    }
}

/// Status indicating availability of an `Input`, `Output`, or a `Mixin`.
#[derive(Clone, Copy, Debug, Eq, GraphQLEnum, PartialEq, SmartDefault)]
pub enum Status {
    /// Inactive, no operations are performed and no media traffic is flowed.
    #[default]
    Offline,

    /// Initializing, media traffic doesn't yet flow as expected.
    Initializing,

    /// Active, all operations are performing successfully and media traffic
    /// flows as expected.
    Online,
}

/// Label of a [`Restream`] or an [`Output`].
#[derive(Clone, Debug, Deref, Display, Eq, Into, PartialEq, Serialize)]
pub struct Label(String);

impl Label {
    /// Creates a new [`Label`] if the given value meets its invariants.
    #[must_use]
    pub fn new<'s, S: Into<Cow<'s, str>>>(val: S) -> Option<Self> {
        static REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^[^,\n\t\r\f\v]{1,70}$").unwrap());

        let val = val.into();
        (!val.is_empty() && REGEX.is_match(&val))
            .then(|| Self(val.into_owned()))
    }
}

impl<'de> Deserialize<'de> for Label {
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(<Cow<'_, str>>::deserialize(deserializer)?)
            .ok_or_else(|| D::Error::custom("Not a valid Label"))
    }
}

/// Type of a `Restream` or an `Output` label.
///
/// It should meet `[^,\n\t\r\f\v]{1,70}` format.
#[graphql_scalar]
impl<S> GraphQLScalar for Label
where
    S: ScalarValue,
{
    fn resolve(&self) -> Value {
        Value::scalar(self.0.as_str().to_owned())
    }

    fn from_input_value(v: &InputValue) -> Option<Self> {
        v.as_scalar()
            .and_then(ScalarValue::as_str)
            .and_then(Self::new)
    }

    fn from_str(value: ScalarToken<'_>) -> ParseScalarResult<'_, S> {
        <String as ParseScalarValue<S>>::from_str(value)
    }
}

/// Volume rate of an audio track in percents.
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    SmartDefault,
)]
pub struct Volume(#[default(Self::ORIGIN.0)] u16);

impl Volume {
    /// Maximum possible value of a [`Volume`] rate.
    pub const MAX: Volume = Volume(1000);

    /// Value of a [`Volume`] rate corresponding to the original one of an audio
    /// track.
    pub const ORIGIN: Volume = Volume(100);

    /// Minimum possible value of a [`Volume`] rate. Actually, disables audio.
    pub const OFF: Volume = Volume(0);

    /// Creates a new [`Volume`] rate value if it satisfies the required
    /// invariants:
    /// - within [`Volume::OFF`] and [`Volume::MAX`] values.
    #[must_use]
    pub fn new<N: TryInto<u16>>(num: N) -> Option<Self> {
        let num = num.try_into().ok()?;
        if (Self::OFF.0..=Self::MAX.0).contains(&num) {
            Some(Self(num))
        } else {
            None
        }
    }

    /// Displays this [`Volume`] as a fraction of `1`, i.e. `100%` as `1`, `50%`
    /// as `0.50`, and so on.
    #[must_use]
    pub fn display_as_fraction(self) -> String {
        format!("{}.{:02}", self.0 / 100, self.0 % 100)
    }

    /// Indicates whether this [`Volume`] rate value corresponds is the
    /// [`Volume::ORIGIN`]al one.
    #[allow(clippy::trivially_copy_pass_by_ref)] // required for `serde`
    #[inline]
    #[must_use]
    pub fn is_origin(&self) -> bool {
        *self == Self::ORIGIN
    }
}

/// Type a volume rate of audio track in percents.
///
/// It's values are always within range of `0` and `1000` (inclusively).
///
/// `0` means disabled audio.
#[graphql_scalar]
impl<S> GraphQLScalar for Volume
where
    S: ScalarValue,
{
    fn resolve(&self) -> Value {
        Value::scalar(i32::from(self.0))
    }

    fn from_input_value(v: &InputValue) -> Option<Self> {
        v.as_scalar()
            .and_then(ScalarValue::as_int)
            .and_then(Self::new)
    }

    fn from_str(value: ScalarToken<'_>) -> ParseScalarResult<'_, S> {
        <String as ParseScalarValue<S>>::from_str(value)
    }
}

/// Delay of a [`Mixin`] being mixed with an [`Output`].
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize,
    Default,
    Eq,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
)]
pub struct Delay(Duration);

impl Delay {
    /// Creates a new [`Delay`] out of the given milliseconds.
    #[inline]
    #[must_use]
    pub fn from_millis<N: TryInto<u64>>(millis: N) -> Option<Self> {
        millis
            .try_into()
            .ok()
            .map(|m| Self(Duration::from_millis(m)))
    }

    /// Returns milliseconds of this [`Delay`].
    #[inline]
    #[must_use]
    pub fn as_millis(&self) -> i32 {
        self.0.as_millis().try_into().unwrap()
    }

    /// Indicates whether this [`Delay`] introduces no actual delay.
    #[inline]
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == Duration::default()
    }
}

/// Type of a `Mixin` delay in milliseconds.
///
/// Negative values are not allowed.
#[graphql_scalar]
impl<S> GraphQLScalar for Delay
where
    S: ScalarValue,
{
    fn resolve(&self) -> Value {
        Value::scalar(self.as_millis())
    }

    fn from_input_value(v: &InputValue) -> Option<Self> {
        v.as_scalar()
            .and_then(ScalarValue::as_int)
            .and_then(Self::from_millis)
    }

    fn from_str(value: ScalarToken<'_>) -> ParseScalarResult<'_, S> {
        <String as ParseScalarValue<S>>::from_str(value)
    }
}

#[cfg(test)]
mod volume_spec {
    use super::Volume;

    #[test]
    fn displays_as_fraction() {
        for (input, expected) in &[
            (1, "0.01"),
            (10, "0.10"),
            (200, "2.00"),
            (107, "1.07"),
            (170, "1.70"),
            (1000, "10.00"),
        ] {
            let actual = Volume::new(*input).unwrap().display_as_fraction();
            assert_eq!(&actual, *expected);
        }
    }
}
