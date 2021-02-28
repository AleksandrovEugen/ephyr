//! [FFmpeg]-based definitions and implementations.
//!
//! [FFmpeg]: https://ffmpeg.org

use std::{
    borrow::Cow,
    collections::HashMap,
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use derive_more::From;
use ephyr_log::{log, Drain as _};
use futures::{future, pin_mut, FutureExt as _, TryFutureExt as _};
use tokio::{io, process::Command, sync::Mutex, time};
use url::Url;
use uuid::Uuid;

use crate::{
    display_panic,
    state::{
        self, Delay, InputId, InputSrcUrl, MixinId, MixinSrcUrl, OutputDstUrl,
        OutputId, State, Status, Volume,
    },
    teamspeak,
};

/// Pool of [FFmpeg] processes performing re-streaming of a media traffic.
///
/// [FFmpeg]: https://ffmpeg.org
#[derive(Debug)]
pub struct RestreamersPool {
    /// Path to a [FFmpeg] binary used for spawning processes.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    ffmpeg_path: PathBuf,

    /// Pool of currently running [FFmpeg] re-streaming processes identified by
    /// an ID of the correspondent element in a [`State`].
    ///
    /// So, potentially allows duplication.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    pool: HashMap<Uuid, Restreamer>,

    /// Application [`State`] dictating which [FFmpeg] processes should run.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    state: State,
}

impl RestreamersPool {
    /// Creates a new [`RestreamersPool`] out of the given parameters.
    #[inline]
    #[must_use]
    pub fn new<P: Into<PathBuf>>(ffmpeg_path: P, state: State) -> Self {
        Self {
            ffmpeg_path: ffmpeg_path.into(),
            pool: HashMap::new(),
            state,
        }
    }

    /// Adjusts this [`RestreamersPool`] to run [FFmpeg] re-streaming processes
    /// according to the given renewed [`state::Restream`]s.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    pub fn apply(&mut self, restreams: &[state::Restream]) {
        // The most often case is when one new FFmpeg process is added.
        let mut new_pool = HashMap::with_capacity(self.pool.len() + 1);

        for r in restreams {
            let _ = self.apply_input(&r.key, &r.input, &mut new_pool);


            todo!()
            if !r.enabled {
                continue;
            }
            let _ = self
                .pool
                .remove(&r.id.into())
                .and_then(|mut p| (!p.needs_restart(r)).then(|| p))
                .or_else(|| {
                    RestreamerKind::new_input(r).map(|kind| {
                        Restreamer::run(
                            self.ffmpeg_path.clone(),
                            kind,
                            self.state.clone(),
                        )
                    })
                })
                .map(|p| drop(new.insert(r.id.into(), p)));

            if !r.input.is_online() {
                continue;
            }

            for o in &r.outputs {
                if !o.enabled {
                    continue;
                }
                let mut prev = self.pool.remove(&o.id.into());
                if prev.as_mut().map_or(true, |p| p.needs_restart(r)) {
                    RestreamerKind::new_output(
                        o,
                        r.id,
                        r.srs_url(),
                        prev.map(|p| p.kind),
                    )
                    .map(|kind| {
                        Restreamer::run(
                            self.ffmpeg_path.clone(),
                            kind,
                            self.state.clone(),
                        )
                    })
                } else {
                    prev
                }
                .map(|p| drop(new.insert(o.id.into(), p)))
                .unwrap_or_default();
            }
        }

        self.pool = new_pool;
    }

    /// Traverses the given [`state::Input`] filling the `new_pool` with
    /// required [FFmpeg] re-streaming processes. Tries to preserve already
    /// running [FFmpeg] processes in its `pool` as much as possible.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    fn apply_input(
        &mut self,
        key: &state::RestreamKey,
        input: &state::Input,
        new_pool: &mut HashMap<Uuid, Restreamer>,
    ) -> Option<()> {
        for s in &input.srcs {
            if let state::InputSrc::Local(i) = s {
                let _ = self.apply_input(key, i, new);
            }
        }

        let new_kind = RestreamerKind::from_input(input, key)?;

        let process = self
            .pool
            .remove(&input.id.into())
            .and_then(|mut p| (!p.kind.needs_restart(&new_kind)).then(|| p))
            .unwrap_or_else(|| {
                Restreamer::run(
                    self.ffmpeg_path.clone(),
                    kind: new_kind,
                    self.state.clone(),
                )
            });

        drop(new_pool.insert(input.id.into(), process));
        Some(())
    }
}

/// Handle to a running [FFmpeg] process performing a re-streaming.
///
/// [FFmpeg]: https://ffmpeg.org
#[derive(Debug)]
pub struct Restreamer {
    /// Abort handle of a spawned [FFmpeg] process of this [`Restreamer`].
    ///
    /// [FFmpeg]: https://ffmpeg.org
    abort: DroppableAbortHandle,

    /// Kind of a spawned [FFmpeg] process describing the actual job it
    /// performs.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    kind: RestreamerKind,
}

/// Data of a concrete kind of a running [FFmpeg] process performing a
/// re-streaming, that allows to spawn and re-spawn it at any time.
///
/// [FFmpeg]: https://ffmpeg.org
#[derive(Clone, Debug, From)]
pub enum RestreamerKind {
    /// Re-streaming of a live stream from one URL endpoint to another one "as
    /// is", without performing any live stream modifications, optionally
    /// transmuxing it to the destination format.
    Copy(CopyRestreamer),
}

impl RestreamerKind {
    /// Creates a new [FFmpeg] process re-streaming a [`state::InputSrc`] to its
    /// [`state::Input`] endpoint.
    ///
    /// Returns [`None`] if a [FFmpeg] re-streaming process cannot not be
    /// created for the given [`state::Input`], or the later doesn't require it.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    #[must_use]
    pub fn from_input(
        input: &state::Input,
        key: &state::RestreamKey,
    ) -> Option<Self> {
        if !input.enabled || input.srcs.is_empty() {
            return None;
        }

        input
            .srcs
            .iter()
            .find(|s| matches!(s, state::InputSrc::Remote(_)))
            .map(|remote| remote.url.clone().into())
            .or_else(|| {
                input.srcs.iter().find_map(|s| {
                    if let state::InputSrc::Local(
                        input
                        @
                        state::Input {
                            status: Status::Online,
                            ..
                        },
                    ) = s
                    {
                        Some(input.rtmp_endpoint_url(key).into())
                    } else {
                        None
                    }
                })
            })
            .map(|from_url| {
                CopyRestreamer {
                    id: input.id.into(),
                    from_url,
                    to_url: input.rtmp_endpoint_url(key),
                }
                .into()
            })
    }

    /// Checks whether this [`Restreamer`] must be restarted, as cannot apply
    /// the new `actual` params on itself correctly, without interruptions.
    #[inline]
    #[must_use]
    pub fn needs_restart(&self, actual: &Self) -> bool {
        match (self, actual) {
            (Self::Copy(old), Self::Copy(new)) => old.needs_restart(new),
            _ => true,
        }
    }
}

/// Kind of a [FFmpeg] re-streaming process that re-streams a live stream from
/// one URL endpoint to another one "as is", without performing any live stream
/// modifications, optionally transmuxing it to the destination format.
///
/// [FFmpeg]: https://ffmpeg.org
#[derive(Clone, Debug)]
pub struct CopyRestreamer {
    /// ID of an element in a [`State`] this [`CopyRestreamer`] process is
    /// related to.
    pub id: Uuid,

    /// [`Url`] to pull a live stream from.
    pub from_url: Url,

    /// [`Url`] to publish the pulled live stream onto.
    pub to_url: Url,
}

impl CopyRestreamer {
    /// Checks whether this [`CopyRestreamer`] process must be restarted, as
    /// cannot apply the new `actual` params on itself correctly, without
    /// interruptions.
    #[inline]
    #[must_use]
    pub fn needs_restart(&self, actual: &Self) -> bool {
        self.from_url != actual.from_url || self.to_url != actual.to_url
    }
}

////////////////////////////////////////////////////////////////////////////////

impl Restreamer {
    /// Creates a new [`Restreamer`] spawning the actual [FFmpeg] process in
    /// background. Once this [`Restreamer`] is dropped, its [FFmpeg] process is
    /// aborted.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    #[must_use]
    pub fn run<P: AsRef<Path> + Send + 'static>(
        ffmpeg_path: P,
        kind: RestreamerKind,
        state: State,
    ) -> Self {
        let (kind_for_abort, state_for_abort) = (kind.clone(), state.clone());

        let kind_for_spawn = kind.clone();
        let (spawner, abort_handle) = future::abortable(async move {
            loop {
                let (kind, state) = (&kind_for_spawn, &state);

                let mut cmd = Command::new(ffmpeg_path.as_ref());

                let _ = AssertUnwindSafe(
                    async move {
                        kind.renew_status(Status::Initializing, state);

                        kind.setup_ffmpeg(
                            cmd.kill_on_drop(true)
                                .stdin(Stdio::null())
                                .stdout(Stdio::null())
                                .stderr(Stdio::piped()),
                            state,
                        );

                        let running = kind.run_ffmpeg(cmd);
                        pin_mut!(running);

                        let set_online = async move {
                            time::delay_for(Duration::from_secs(5)).await;
                            kind.renew_status(Status::Online, state);
                            future::pending::<()>().await;
                            Ok(())
                        };
                        pin_mut!(set_online);

                        future::try_select(running, set_online)
                            .await
                            .map_err(|e| {
                                log::error!(
                                    "Failed to run FFmpeg re-streamer: {}",
                                    e.factor_first().0,
                                )
                            })
                            .map(|r| r.factor_first().0)
                    }
                    .unwrap_or_else(|_| {
                        kind.renew_status(Status::Offline, state);
                    }),
                )
                .catch_unwind()
                .await
                .map_err(|p| {
                    log::crit!(
                        "Panicked while spawning/observing FFmpeg \
                         re-streamer: {}",
                        display_panic(&p),
                    );
                });

                time::delay_for(Duration::from_secs(2)).await;
            }
        });

        // Spawn FFmpeg re-streamer as a child process.
        drop(tokio::spawn(spawner.map(move |_| {
            kind_for_abort.renew_status(Status::Offline, &state_for_abort)
        })));

        Self {
            abort: DroppableAbortHandle(abort_handle),
            kind,
        }
    }

    /// Checks whether this [`Restreamer`] must be restarted, as cannot apply
    /// the new `actual` state on itself correctly, without interruptions.
    #[inline]
    #[must_use]
    pub fn needs_restart(&mut self, actual: &state::Restream) -> bool {
        match &mut self.kind {
            RestreamerKind::FailoverPushInput(i) => i.needs_restart(actual),
            RestreamerKind::PullInput(i) => i.needs_restart(actual),
            RestreamerKind::CopyOutput(o) => o.needs_restart(actual),
            RestreamerKind::TeamspeakMixedOutput(o) => o.needs_restart(actual),
        }
    }
}

/// Data of a concrete kind of a running [FFmpeg] process performing a
/// re-streaming, that allows to spawn and re-spawn it at any time.
///
/// [FFmpeg]: https://ffmpeg.org
#[derive(Clone, Debug, From)]
pub enum RestreamerKind2 {
    /// Re-streaming of a live stream from [`FailoverPushInput`]'s `/main` or
    /// `/backup` endpoint to its `/in` endpoint.
    ///
    /// [`FailoverPushInput`]: state::FailoverPushInput
    FailoverPushInput(FailoverPushInputRestreamer),

    /// Re-streaming of a [`PullInput::src`] live stream to the correspondent
    /// [`Restream::srs_url`] endpoint.
    ///
    /// [`PullInput::src`]: state::PullInput::src
    /// [`Restream::srs_url`]: state::Restream::srs_url
    PullInput(PullInputRestreamer),

    /// Re-streaming of a [`Restream::srs_url`] live stream to the correspondent
    /// [`Output::dst`] remote endpoint "as is", without performing any live
    /// stream modifications.
    ///
    /// [`Output::dst`]: state::Output::dst
    /// [`Restream::srs_url`]: state::Restream::srs_url
    CopyOutput(CopyOutputRestreamer),

    /// Re-streaming of a [`Restream::srs_url`] live stream to the correspondent
    /// [`Output::dst`] remote endpoint being mixed with a [TeamSpeak]
    /// [`Mixin::src`] endpoint.
    ///
    /// [`Restream::srs_url`]: state::Restream::srs_url
    /// [`Mixin::src`]: state::Mixin::src
    /// [`Output::dst`]: state::Output::dst
    /// [TeamSpeak]: https://teamspeak.com
    TeamspeakMixedOutput(TeamspeakMixedOutputRestreamer),
}

impl RestreamerKind2 {
    /// Creates a new [FFmpeg] process re-streaming a live stream from some
    /// source to the [`Restream::srs_url`] endpoint.
    ///
    /// Returns [`None`] if a [FFmpeg] re-streaming process must not be created
    /// according to the given [`state::Output`].
    ///
    /// [`Restream::srs_url`]: state::Restream::srs_url
    /// [FFmpeg]: https://ffmpeg.org
    #[must_use]
    pub fn new_input(r: &state::Restream) -> Option<Self> {
        if !r.enabled {
            return None;
        }
        PullInputRestreamer::new(r)
            .map(Into::into)
            .or_else(|| FailoverPushInputRestreamer::new(r).map(Into::into))
    }

    /// Creates a new [FFmpeg] process re-streaming a live stream from the given
    /// `srs_url` to the given [`Output::dst`] endpoint.
    ///
    /// `prev` value may be specified to consume already initialized resources,
    /// which are unwanted to be re-created.
    ///
    /// Returns [`None`] if a [FFmpeg] re-streaming process must not be created
    /// according to the given [`state::Output`].
    ///
    /// [`Output::dst`]: state::Output::dst
    /// [FFmpeg]: https://ffmpeg.org
    #[must_use]
    pub fn new_output(
        o: &state::Output,
        input_id: InputId,
        srs_url: Url,
        prev: Option<RestreamerKind>,
    ) -> Option<Self> {
        if !o.enabled {
            return None;
        }
        TeamspeakMixedOutputRestreamer::new(o, input_id, &srs_url, prev)
            .map(Into::into)
            .or_else(|| {
                CopyOutputRestreamer::new(o, input_id, srs_url).map(Into::into)
            })
    }

    /// Renews [`Status`] of this [FFmpeg] re-streaming process in the `actual`
    /// [`State`].
    ///
    /// [FFmpeg]: https://ffmpeg.org
    #[inline]
    pub fn renew_status(&self, status: Status, actual: &State) {
        match self {
            Self::FailoverPushInput(i) => i.renew_status(status, actual),
            Self::PullInput(i) => i.renew_status(status, actual),
            Self::CopyOutput(o) => o.renew_status(status, actual),
            Self::TeamspeakMixedOutput(o) => o.renew_status(status, actual),
        }
    }

    /// Properly setups the given [FFmpeg] [`Command`] before running it.
    ///
    /// The specified [`State`] may be used to retrieve up-to-date parameters,
    /// which don't trigger re-creation of the whole [FFmpeg] re-streaming
    /// process.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    #[inline]
    fn setup_ffmpeg(&self, cmd: &mut Command, state: &State) {
        match self {
            Self::FailoverPushInput(i) => i.setup_ffmpeg(cmd),
            Self::PullInput(i) => i.setup_ffmpeg(cmd),
            Self::CopyOutput(o) => o.setup_ffmpeg(cmd),
            Self::TeamspeakMixedOutput(o) => o.setup_ffmpeg(cmd, state),
        }
    }

    /// Properly runs the given [FFmpeg] [`Command`] awaiting its completion.
    ///
    /// # Errors
    ///
    /// This method doesn't return [`Ok`] as the running [FFmpeg] [`Command`] is
    /// aborted by dropping and is intended to never stop. If it returns, than
    /// an [`io::Error`] occurs and the [FFmpeg] [`Command`] cannot run.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    async fn run_ffmpeg(&self, mut cmd: Command) -> io::Result<()> {
        if let Self::TeamspeakMixedOutput(o) = self {
            return o.run_ffmpeg(cmd).await;
        }

        let process = cmd.spawn()?;

        let out = process.wait_with_output().await?;

        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "FFmpeg re-streamer stopped with exit code: {}\n{}",
                out.status,
                String::from_utf8_lossy(&out.stderr),
            ),
        ))
    }
}

/// Kind of a [FFmpeg] re-streaming process that re-streams a [`PullInput::src`]
/// live stream to the correspondent [`Restream::srs_url`] endpoint.
///
/// [`PullInput::src`]: state::PullInput::src
/// [`Restream::srs_url`]: state::Restream::srs_url
/// [FFmpeg]: https://ffmpeg.org
#[derive(Clone, Debug)]
pub struct PullInputRestreamer {
    /// ID of a [`state::Input`] this [`PullInputRestreamer`] process is related
    /// to.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    input_id: InputId,

    /// Remote [`Url`] to pull a live stream from.
    upstream_url: InputSrcUrl,

    /// Local [SRS] [`Url`] to publish the pulled live stream onto.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    srs_url: Url,
}

impl PullInputRestreamer {
    /// Creates a new [`PullInputRestreamer`] data.
    ///
    /// Returns [`None`] if a [`PullInputRestreamer`] process must not be
    /// created according to the given [`state::Restream`].
    #[must_use]
    pub fn new(r: &state::Restream) -> Option<Self> {
        if !r.enabled || !r.input.is_pull() {
            return None;
        }

        let upstream_url = r.upstream_url().unwrap();
        let upstream_url = match upstream_url.scheme() {
            "rtmp" | "rtmps" => upstream_url.clone(),
            _ => unimplemented!(),
        };

        Some(Self {
            input_id: r.id,
            upstream_url,
            srs_url: r.srs_url(),
        })
    }

    /// Checks whether this [`PullInputRestreamer`] process must be restarted,
    /// as cannot apply the new `actual` state on itself correctly, without
    /// interruptions.
    #[must_use]
    pub fn needs_restart(&self, actual: &state::Restream) -> bool {
        if actual.input.is_pull() {
            Some(&self.upstream_url) != actual.upstream_url()
        } else {
            true
        }
    }

    /// Renews [`Status`] of this [`PullInputRestreamer`] process in the
    /// `actual` [`State`].
    pub fn renew_status(&self, status: Status, actual: &State) {
        // `Status::Online` for `PullInput` is set by SRS HTTP Callback.
        if status != Status::Online {
            let _ = actual.restreams.lock_mut().iter_mut().find_map(|r| {
                (r.id == self.input_id && r.input.is_pull())
                    .then(|| r.input.set_status(status))
            });
        }
    }

    /// Properly setups the given [FFmpeg] [`Command`] for this
    /// [`PullInputRestreamer`] before running it.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    fn setup_ffmpeg(&self, cmd: &mut Command) {
        let _ = cmd
            .args(&["-i", self.upstream_url.as_str()])
            .args(&["-c", "copy"])
            .args(&["-f", "flv", self.srs_url.as_str()]);
    }
}

/// Kind of a [FFmpeg] re-streaming process that re-streams a
/// [`FailoverPushInput`]'s `/main` or `/backup` endpoint to its `/in` endpoint.
///
/// [`FailoverPushInput`]: state::FailoverPushInput
/// [FFmpeg]: https://ffmpeg.org
#[derive(Clone, Debug)]
pub struct FailoverPushInputRestreamer {
    /// ID of a [`state::Input`] this [`FailoverPushInputRestreamer`] process is
    /// related to.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    input_id: InputId,

    /// Local [SRS] [`Url`] of the `/main` or `/backup` endpoint of this
    /// [`FailoverPushInputRestreamer`] process to re-stream a live stream from.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    from_srs_url: Url,

    /// Local [SRS] [`Url`] of the `/in` endpoint of this
    /// [`FailoverPushInputRestreamer`] process to re-stream a live stream onto.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    to_srs_url: Url,
}

impl FailoverPushInputRestreamer {
    /// Creates a new [`FailoverPushInputRestreamer`] data.
    ///
    /// Returns [`None`] if a [`FailoverPushInputRestreamer`] process must not
    /// be created according to the given [`state::Restream`].
    #[must_use]
    pub fn new(r: &state::Restream) -> Option<Self> {
        if !r.enabled {
            return None;
        }
        if let state::Input::FailoverPush(input) = &r.input {
            input.has_traffic().then(|| Self {
                input_id: r.id,
                from_srs_url: input.from_url(),
                to_srs_url: r.srs_url(),
            })
        } else {
            None
        }
    }

    /// Checks whether this [`FailoverPushInputRestreamer`] process must be
    /// restarted, as cannot apply the new `actual` state on itself correctly,
    /// without interruptions.
    #[must_use]
    pub fn needs_restart(&self, actual: &state::Restream) -> bool {
        if let state::Input::FailoverPush(input) = &actual.input {
            !input.has_traffic()
                || self.from_srs_url != input.from_url()
                || self.to_srs_url != actual.srs_url()
        } else {
            true
        }
    }

    /// Renews [`Status`] of this [`FailoverPushInputRestreamer`] process in the
    /// `actual` [`State`].
    pub fn renew_status(&self, status: Status, actual: &State) {
        // `Status::Online` for `FailoverPushInput` is set by SRS HTTP Callback.
        if status != Status::Online {
            let _ = actual.restreams.lock_mut().iter_mut().find_map(|r| {
                (r.id == self.input_id && r.input.is_failover())
                    .then(|| r.input.set_status(status))
            });
        }
    }

    /// Properly setups the given [FFmpeg] [`Command`] for this
    /// [`FailoverPushInputRestreamer`] before running it.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    fn setup_ffmpeg(&self, cmd: &mut Command) {
        let _ = cmd
            .args(&["-i", self.from_srs_url.as_str()])
            .args(&["-c", "copy"])
            .args(&["-f", "flv", self.to_srs_url.as_str()]);
    }
}

/// Kind of a [FFmpeg] re-streaming process that re-streams a
/// [`Restream::srs_url`] live stream to the correspondent [`Output::dst`]
/// remote endpoint "as is", without performing any live stream modifications.
///
/// [`Output::dst`]: state::Output::dst
/// [`Restream::srs_url`]: state::Restream::srs_url
/// [FFmpeg]: https://ffmpeg.org
#[derive(Clone, Debug)]
pub struct CopyOutputRestreamer {
    /// ID of a [`state::Input`] this [`CopyOutputRestreamer`] process is
    /// related to.
    input_id: InputId,

    /// Local [SRS] [`Url`] to pull a live stream from.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    srs_url: Url,

    /// ID of a [`state::Output`] this [`CopyOutputRestreamer`] process is
    /// related to.
    output_id: OutputId,

    /// Remote [`Url`] to publish the pulled live stream onto.
    downstream_url: OutputDstUrl,
}

impl CopyOutputRestreamer {
    /// Creates a new [`CopyOutputRestreamer`] data.
    ///
    /// Returns [`None`] if a [`CopyOutputRestreamer`] process must not be
    /// created according to the given [`state::Output`].
    #[must_use]
    pub fn new(
        o: &state::Output,
        input_id: InputId,
        srs_url: Url,
    ) -> Option<Self> {
        let downstream_url = match o.dst.scheme() {
            "icecast" | "rtmp" | "rtmps" => o.dst.clone(),
            _ => unimplemented!(),
        };
        if !o.mixins.is_empty() {
            unimplemented!()
        }

        Some(Self {
            input_id,
            srs_url,
            output_id: o.id,
            downstream_url,
        })
    }

    /// Checks whether this [`CopyOutputRestreamer`] process must be restarted,
    /// as cannot apply the new `actual` state on itself correctly, without
    /// interruptions.
    #[must_use]
    pub fn needs_restart(&self, actual: &state::Restream) -> bool {
        if self.srs_url != actual.srs_url() {
            return true;
        }

        let output = actual.outputs.iter().find(|o| o.id == self.output_id);
        let output = if let Some(o) = output { o } else { return true };

        self.downstream_url != output.dst || !output.mixins.is_empty()
    }

    /// Renews [`Status`] of this [`CopyOutputRestreamer`] process in the
    /// `actual` [`State`].
    pub fn renew_status(&self, status: Status, state: &State) {
        let _ = state
            .restreams
            .lock_mut()
            .iter_mut()
            .find(|r| r.id == self.input_id)
            .and_then(|r| r.outputs.iter_mut().find(|o| o.id == self.output_id))
            .map(|o| o.status = status);
    }

    /// Properly setups the given [FFmpeg] [`Command`] for this
    /// [`CopyOutputRestreamer`] before running it.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    fn setup_ffmpeg(&self, cmd: &mut Command) {
        let _ = cmd.args(&["-i", self.srs_url.as_str()]);
        let _ = match self.downstream_url.scheme() {
            "icecast" => cmd
                .args(&["-c:a", "libmp3lame", "-b:a", "64k"])
                .args(&["-f", "mp3", "-content_type", "audio/mpeg"]),

            "rtmp" | "rtmps" => cmd.args(&["-c", "copy"]).args(&["-f", "flv"]),

            _ => unimplemented!(),
        }
        .arg(self.downstream_url.as_str());
    }
}

/// Kind of a [FFmpeg] re-streaming process that re-streams a
/// [`Restream::srs_url`] live stream to the correspondent [`Output::dst`]
/// remote endpoint mixing it with an audio from the [TeamSpeak] [`Mixin::src`]
/// endpoint.
///
/// [`Restream::srs_url`]: state::Restream::srs_url
/// [`Mixin::src`]: state::Mixin::src
/// [`Output::dst`]: state::Output::dst
/// [FFmpeg]: https://ffmpeg.org
/// [TeamSpeak]: https://teamspeak.com
#[derive(Clone, Debug)]
pub struct TeamspeakMixedOutputRestreamer {
    /// ID of a [`state::Input`] this [`TeamspeakMixedOutputRestreamer`] process
    /// is related to.
    input_id: InputId,

    /// Local [SRS] [`Url`] to pull a live stream from.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    srs_url: Url,

    /// [`Volume`] rate to mix an audio of the pulled live stream with.
    input_volume: Volume,

    /// [ZeroMQ] port of a spawned [FFmpeg] process listening real-time filter
    /// updates of the pulled live stream during mixing process.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    /// [ZeroMQ]: https://zeromq.org
    input_zmq_port: u16,

    /// ID of a [`state::Mixin`] this [`TeamspeakMixedOutputRestreamer`]
    /// process uses for mixing.
    mixin_id: MixinId,

    /// [TeamSpeak] [`Url`] to pull a live audio from for mixing.
    ///
    /// [TeamSpeak]: https://teamspeak.com
    mixin_url: MixinSrcUrl,

    /// [`Delay`] to mix the mixed-in [TeamSpeak] live audio with.
    ///
    /// [TeamSpeak]: https://teamspeak.com
    mixin_delay: Delay,

    /// [`Volume`] rate to mix the mixed-in [TeamSpeak] live audio with.
    ///
    /// [TeamSpeak]: https://teamspeak.com
    mixin_volume: Volume,

    /// [ZeroMQ] port of a spawned [FFmpeg] process listening real-time filter
    /// updates of the mixed-in [TeamSpeak] live audio during mixing process.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    /// [TeamSpeak]: https://teamspeak.com
    /// [ZeroMQ]: https://zeromq.org
    mixin_zmq_port: u16,

    /// Actual live audio captured from the [TeamSpeak] server.
    ///
    /// [TeamSpeak]: https://teamspeak.com
    mixin_src: Arc<Mutex<teamspeak::Input>>,

    /// ID of a [`state::Output`] this [`TeamspeakMixedOutputRestreamer`]
    /// process is related to.
    output_id: OutputId,

    /// Remote [`Url`] to publish the mixed live stream onto.
    downstream_url: OutputDstUrl,
}

impl TeamspeakMixedOutputRestreamer {
    /// Creates a new [`TeamspeakMixedOutputRestreamer`] data.
    ///
    /// `prev` is used to consume an already initialized [`teamspeak::Input`],
    /// if any.
    ///
    /// Returns [`None`] if a [`TeamspeakMixedOutputRestreamer`] process must
    /// not be created according to the given [`state::Output`].
    #[allow(clippy::non_ascii_literal)]
    #[must_use]
    pub fn new(
        o: &state::Output,
        input_id: InputId,
        srs_url: &Url,
        prev: Option<RestreamerKind>,
    ) -> Option<Self> {
        let downstream_url = match o.dst.scheme() {
            "icecast" | "rtmp" | "rtmps" => o.dst.clone(),
            _ => return None,
        };

        let mixin = o.mixins.first()?;
        if mixin.src.scheme() != "ts" || o.mixins.len() > 1 {
            return None;
        }
        let mixin_src = prev
            .and_then(|kind| {
                if let RestreamerKind::TeamspeakMixedOutput(o) = kind {
                    return Some(o.mixin_src);
                }
                None
            })
            .or_else(|| {
                let mut host = Cow::Borrowed(mixin.src.host_str()?);
                if let Some(port) = mixin.src.port() {
                    host = Cow::Owned(format!("{}:{}", host, port));
                }

                let channel = mixin.src.path().trim_start_matches('/');

                let name = mixin
                    .src
                    .query_pairs()
                    .find_map(|(k, v)| (k == "name").then(|| v.into_owned()))
                    .or_else(|| o.label.as_ref().map(|l| format!("🤖 {}", l)))
                    .unwrap_or_else(|| format!("🤖 {}", mixin.id));

                Some(Arc::new(Mutex::new(teamspeak::Input::new(
                    teamspeak::Connection::build(host.into_owned())
                        .channel(channel.to_owned())
                        .name(name),
                ))))
            })?;

        Some(Self {
            input_id,
            srs_url: srs_url.clone(),
            input_volume: o.volume,
            input_zmq_port: Self::new_unique_zmq_port(),
            mixin_id: mixin.id,
            mixin_url: mixin.src.clone(),
            mixin_delay: mixin.delay,
            mixin_volume: mixin.volume,
            mixin_zmq_port: Self::new_unique_zmq_port(),
            mixin_src,
            output_id: o.id,
            downstream_url,
        })
    }

    /// Generates a new port for a [ZeroMQ] listener, which is highly unlikely
    /// to be used already.
    ///
    /// [ZeroMQ]: https://zeromq.org
    #[must_use]
    fn new_unique_zmq_port() -> u16 {
        use std::{
            convert,
            sync::atomic::{AtomicU16, Ordering},
        };

        static LATEST_PORT: AtomicU16 = AtomicU16::new(20000);

        LATEST_PORT
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |p| {
                Some(p.checked_add(1).unwrap_or(20000))
            })
            .unwrap_or_else(convert::identity)
    }

    /// Checks whether this [`TeamspeakMixedOutputRestreamer`] process must be
    /// restarted, as cannot apply the new `actual` state on itself correctly,
    /// without interruptions.
    ///
    /// Also, tunes up [`Volume`]s in the spawned [FFmpeg] process via [ZeroMQ]
    /// protocol, if they have changed.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    /// [ZeroMQ]: https://zeromq.org
    #[must_use]
    pub fn needs_restart(&mut self, actual: &state::Restream) -> bool {
        if self.srs_url != actual.srs_url() {
            return true;
        }

        let output = actual.outputs.iter().find(|o| o.id == self.output_id);
        let output = if let Some(o) = output { o } else { return true };

        if self.downstream_url != output.dst || output.mixins.len() != 1 {
            return true;
        }

        let mixin = output.mixins.first().unwrap();

        if self.mixin_url != mixin.src || self.mixin_delay != mixin.delay {
            return true;
        }

        if self.input_volume != output.volume {
            self.input_volume = output.volume;
            Self::tune_volume(
                self.output_id.into(),
                self.input_zmq_port,
                self.input_volume,
            );
        }
        if self.mixin_volume != mixin.volume {
            self.mixin_volume = mixin.volume;
            Self::tune_volume(
                self.mixin_id.into(),
                self.mixin_zmq_port,
                self.mixin_volume,
            );
        }

        false
    }

    /// Renews [`Status`] of this [`TeamspeakMixedOutputRestreamer`] process in
    /// the `actual` [`State`].
    pub fn renew_status(&self, status: Status, actual: &State) {
        let _ = actual
            .restreams
            .lock_mut()
            .iter_mut()
            .find(|r| r.id == self.input_id)
            .and_then(|r| r.outputs.iter_mut().find(|o| o.id == self.output_id))
            .map(|o| o.status = status);
    }

    /// Properly setups the given [FFmpeg] [`Command`] for this
    /// [`TeamspeakMixedOutputRestreamer`] before running it.
    ///
    /// The specified [`State`] is used to retrieve up-to-date [`Volume`]s, as
    /// their changes don't trigger re-creation of the whole [FFmpeg]
    /// re-streaming process.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    fn setup_ffmpeg(&self, cmd: &mut Command, state: &State) {
        // We need up-to-date values of `Volume` here, right from the `State`,
        // as they won't be updated in a closured `self` value.
        let output = state
            .restreams
            .lock_mut()
            .iter()
            .find(|r| r.id == self.input_id)
            .and_then(|r| r.outputs.iter().find(|o| o.id == self.output_id))
            .cloned();
        let output = if let Some(o) = output { o } else { return };
        let mixin = output.mixins.first().unwrap();

        if ephyr_log::logger().is_debug_enabled() {
            let _ = cmd.stderr(Stdio::inherit()).args(&["-loglevel", "debug"]);
        } else {
            let _ = cmd.stderr(Stdio::null());
        }

        let _ = cmd
            .stdin(Stdio::piped())
            .args(&["-i", self.srs_url.as_str()])
            .args(&["-thread_queue_size", "512"])
            .args(&["-f", "f32be"])
            .args(&["-sample_rate", "48000"])
            .args(&["-channels", "2"])
            .args(&["-use_wallclock_as_timestamps", "true"])
            .args(&["-i", "pipe:0"])
            .args(&[
                "-filter_complex",
                &format!(
                    "[0:a]\
                        volume@{output_id}={input_vol},\
                        aresample=48000,\
                        azmq=bind_address=\
                            tcp\\\\\\://127.0.0.1\\\\\\:{input_port}\
                     [{output_id}];\
                     [1:a]\
                        volume@{mixin_id}={mixin_vol},\
                        aresample=async=1,\
                        {delay_filter}\
                        azmq=bind_address=\
                            tcp\\\\\\://127.0.0.1\\\\\\:{mixin_port}\
                     [{mixin_id}];\
                     [{output_id}][{mixin_id}]\
                         amix=inputs=2:duration=longest\
                     [out]",
                    output_id = self.output_id,
                    input_vol = output.volume.display_as_fraction(),
                    input_port = self.input_zmq_port,
                    mixin_id = self.mixin_id,
                    mixin_vol = mixin.volume.display_as_fraction(),
                    mixin_port = self.mixin_zmq_port,
                    delay_filter = (!self.mixin_delay.is_zero())
                        .then(|| format!(
                            "adelay=delays={}:all=1,",
                            self.mixin_delay.as_millis(),
                        ))
                        .unwrap_or_default()
                ),
            ])
            .args(&["-map", "[out]"])
            .args(&["-max_muxing_queue_size", "50000000"]);
        let _ = match self.downstream_url.scheme() {
            "icecast" => cmd
                .args(&["-c:a", "libmp3lame", "-b:a", "64k"])
                .args(&["-f", "mp3", "-content_type", "audio/mpeg"]),

            "rtmp" | "rtmps" => cmd
                .args(&["-map", "0:v"])
                .args(&["-c:a", "libfdk_aac", "-c:v", "copy", "-shortest"])
                .args(&["-f", "flv"]),

            _ => unimplemented!(),
        }
        .arg(self.downstream_url.as_str());
    }

    /// Runs the given [FFmpeg] [`Command`] by feeding to its STDIN the captured
    /// [TeamSpeak] live audio, and awaits its completion.
    ///
    /// # Errors
    ///
    /// This method doesn't return [`Ok`] as the running [FFmpeg] [`Command`] is
    /// aborted by dropping and is intended to never stop. If it returns, than
    /// an [`io::Error`] occurs and the [FFmpeg] [`Command`] cannot run.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    /// [TeamSpeak]: https://teamspeak.com
    async fn run_ffmpeg(&self, mut cmd: Command) -> io::Result<()> {
        let process = cmd.spawn()?;

        let ffmpeg_stdin = &mut process.stdin.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                "FFmpeg's STDIN hasn't been captured",
            )
        })?;

        let mut mixin_src = self.mixin_src.lock().await;
        let _ = io::copy(&mut *mixin_src, ffmpeg_stdin).await.map_err(|e| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("Failed to write into FFmpeg's STDIN: {}", e),
            )
        })?;

        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "FFmpeg re-streamer stopped unexpectedly",
        ))
    }

    /// Tunes [`Volume`] of the specified [FFmpeg] `track` by updating the
    /// `volume` [FFmpeg] filter in real-time via [ZeroMQ] protocol.
    ///
    /// [FFmpeg]: https://ffmpeg.org
    /// [ZeroMQ]: https://zeromq.org
    fn tune_volume(track: Uuid, port: u16, volume: Volume) {
        use zeromq::{BlockingRecv as _, BlockingSend as _, Socket as _};

        drop(tokio::spawn(
            AssertUnwindSafe(async move {
                let addr = format!("tcp://127.0.0.1:{}", port);

                let mut socket = zeromq::ReqSocket::new();
                socket.connect(&addr).await.map_err(|e| {
                    log::error!(
                        "Failed to establish ZeroMQ connection with {} : {}",
                        addr,
                        e,
                    )
                })?;

                socket
                    .send(
                        format!(
                            "volume@{} volume {}",
                            track,
                            volume.display_as_fraction()
                        )
                        .into(),
                    )
                    .await
                    .map_err(|e| {
                        log::error!(
                            "Failed to send ZeroMQ message to {} : {}",
                            addr,
                            e,
                        )
                    })?;

                let resp = socket.recv().await.map_err(|e| {
                    log::error!(
                        "Failed to receive ZeroMQ response from {} : {}",
                        addr,
                        e,
                    )
                })?;

                if resp.data.as_ref() != "OK".as_bytes() {
                    log::error!(
                        "Received invalid ZeroMQ response from {} : {}",
                        addr,
                        std::str::from_utf8(&*resp.data).map_or_else(
                            |_| Cow::Owned(format!("{:?}", &*resp.data)),
                            Cow::Borrowed,
                        ),
                    )
                }

                <Result<_, ()>>::Ok(())
            })
            .catch_unwind()
            .map_err(|p| {
                log::crit!(
                    "Panicked while sending ZeroMQ message: {}",
                    display_panic(&p),
                );
            }),
        ));
    }
}

/// Abort handle of a spawned [FFmpeg] [`Restreamer`] process.
///
/// [FFmpeg]: https://ffmpeg.org
#[derive(Clone, Debug)]
pub struct DroppableAbortHandle(future::AbortHandle);

impl Drop for DroppableAbortHandle {
    #[inline]
    fn drop(&mut self) {
        self.0.abort();
    }
}
