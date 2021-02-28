//! Client [GraphQL] API providing application usage.
//!
//! [GraphQL]: https://graphql.com

use actix_web::http::StatusCode;
use anyhow::anyhow;
use futures::stream::BoxStream;
use futures_signals::signal::SignalExt as _;
use juniper::{graphql_object, graphql_subscription, GraphQLObject, RootNode};
use once_cell::sync::Lazy;
use rand::Rng as _;

use crate::{
    api::graphql,
    spec,
    state::{
        Delay, InputKey, InputSrcUrl, Label, MixinId, MixinSrcUrl,
        OutputDstUrl, OutputId, Restream, RestreamId, RestreamKey, Volume,
    },
    Spec,
};

use super::Context;

/// Full schema of [`api::graphql::client`].
///
/// [`api::graphql::client`]: graphql::client
pub type Schema =
    RootNode<'static, QueriesRoot, MutationsRoot, SubscriptionsRoot>;

/// Constructs and returns new [`Schema`], ready for use.
#[inline]
#[must_use]
pub fn schema() -> Schema {
    Schema::new(QueriesRoot, MutationsRoot, SubscriptionsRoot)
}

/// Root of all [GraphQL mutations][1] in the [`Schema`].
///
/// [1]: https://spec.graphql.org/June2018/#sec-Root-Operation-Types
#[derive(Clone, Copy, Debug)]
pub struct MutationsRoot;

#[graphql_object(name = "Mutation", context = Context)]
impl MutationsRoot {
    /// Applies the specified JSON `spec` of `Restream`s to this server.
    ///
    /// If `replace` is `true` then replaces all the existing `Restream`s with
    /// the one defined by the `spec`. Otherwise, merges the `spec` with
    /// existing `Restream`s.
    ///
    /// ### Result
    ///
    /// Returns `null` if a `Restream` with the given `id` doesn't exist,
    /// otherwise always returns `true`.
    #[graphql(arguments(
        spec(description = "JSON spec obtained with `export` query."),
        replace(
            description = "Indicator whether the `spec` should replace \
                           existing definitions.",
            default = false,
        ),
        restream_id(description = "Optional ID of a concrete `Restream` \
                                   to apply the `spec` to without touching \
                                   other `Restream`s."),
    ))]
    fn import(
        spec: String,
        replace: bool,
        restream_id: Option<RestreamId>,
        context: &Context,
    ) -> Result<Option<bool>, graphql::Error> {
        let spec = serde_json::from_str::<Spec>(&spec)?.into_v1();

        Ok(if let Some(id) = restream_id {
            if spec.restreams.len() != 1 {
                return Err(graphql::Error::new("INVALID_SPEC")
                    .status(StatusCode::BAD_REQUEST)
                    .message("JSON spec should contain exactly one Restream"));
            }
            context
                .state()
                .restreams
                .lock_mut()
                .iter_mut()
                .find_map(|r| {
                    if r.id == id {
                        r.apply(spec.restreams.into_iter().next()?, replace);
                        Some(true)
                    } else {
                        None
                    }
                })
        } else {
            context.state().apply(spec, replace);
            Some(true)
        })
    }

    /// Sets a new `Restream` or updates an existing one (if `id` is specified).
    ///
    /// ### Idempotency
    ///
    /// Idempotent if `id` is specified. Otherwise is non-idempotent, always
    /// creates a new `Restream` and errors on the `key` duplicates.
    ///
    /// ### Result
    ///
    /// Returns `null` if a `Restream` with the given `id` doesn't exist,
    /// otherwise always returns `true`.
    #[graphql(arguments(
        key(description = "Unique key to set the `Restream` with."),
        label(description = "Optional label to set the `Restream` with."),
        src(description = "URL to pull a live stream from.\
                           \n\n\
                           If not specified then `Restream` will await for a \
                           live stream being pushed to its endpoint."),
        backup_src(
            description = "URL to pull a live stream from for a backup \
                           endpoint.\
                           \n\n\
                           If not specified then `Restream` will await for a \
                           live stream being pushed to its backup endpoint.\
                           \n\n\
                           Has no effect if `withBackup` argument is not \
                           `true`.",
        ),
        with_backup(
            description = "Indicator whether the `Restream` should have a \
                           backup endpoint for a live stream.",
            default = false,
        ),
        id(description = "ID of the `Restream` to be updated rather than \
                          creating a new one."),
    ))]
    fn set_restream(
        key: RestreamKey,
        label: Option<Label>,
        src: Option<InputSrcUrl>,
        backup_src: Option<InputSrcUrl>,
        with_backup: bool,
        id: Option<RestreamId>,
        context: &Context,
    ) -> Result<Option<bool>, graphql::Error> {
        let mut input = spec::v1::Input {
            key: InputKey::new("in").unwrap(),
            srcs: vec![],
            enabled: true,
        };
        if with_backup {
            input.srcs.push(
                spec::v1::Input {
                    key: InputKey::new("main").unwrap(),
                    srcs: src
                        .map(|url| vec![spec::v1::InputSrc::RemoteUrl(url)])
                        .unwrap_or_default(),
                    enabled: true,
                }
                .into(),
            );
            input.srcs.push(
                spec::v1::Input {
                    key: InputKey::new("backup").unwrap(),
                    srcs: backup_src
                        .map(|url| vec![spec::v1::InputSrc::RemoteUrl(url)])
                        .unwrap_or_default(),
                    enabled: true,
                }
                .into(),
            );
        } else if let Some(url) = src {
            input.srcs.push(spec::v1::InputSrc::RemoteUrl(url));
        }

        let spec = spec::v1::Restream {
            key,
            label,
            input,
            outputs: vec![],
        };

        Ok(if let Some(id) = id {
            context.state().edit_restream(id, spec)
        } else {
            context.state().add_restream(spec).map(Some)
        }
        .map_err(|e| {
            graphql::Error::new("DUPLICATE_RESTREAM_KEY")
                .status(StatusCode::CONFLICT)
                .message(&e)
        })?
        .map(|_| true))
    }

    /// Removes a `Restream` by its `id`.
    ///
    /// ### Result
    ///
    /// Returns `null` if `Restream` with the given `id` doesn't exist,
    /// otherwise always returns `true`.
    #[graphql(arguments(id(
        description = "ID of the `Restream` to be removed."
    )))]
    fn remove_restream(id: RestreamId, context: &Context) -> Option<bool> {
        context.state().remove_restream(id)?;
        Some(true)
    }

    /// Enables a `Restream` by its `id`.
    ///
    /// Enabled `Restream` is allowed to accept or pull a live stream.
    ///
    /// ### Result
    ///
    /// Returns `true` if a `Restream` with the given `id` has been enabled,
    /// `false` if it has been enabled already, and `null` if it doesn't exist.
    #[graphql(arguments(id(
        description = "ID of the `Restream` to be enabled."
    )))]
    fn enable_restream(id: RestreamId, context: &Context) -> Option<bool> {
        context.state().enable_restream(id)
    }

    /// Disables a `Restream` by its `id`.
    ///
    /// Disabled `Restream` stops all on-going re-streaming processes and is not
    /// allowed to accept or pull a live stream.
    ///
    /// ### Result
    ///
    /// Returns `true` if a `Restream` with the given `id` has been disabled,
    /// `false` if it has been disabled already, and `null` if it doesn't exist.
    #[graphql(arguments(id(
        description = "ID of the `Restream` to be disabled."
    )))]
    fn disable_restream(id: RestreamId, context: &Context) -> Option<bool> {
        context.state().disable_restream(id)
    }

    /// Adds a new `Output` to the specified `Restream`.
    ///
    /// ### Non-idempotent
    ///
    /// Always creates a new `Output` and errors on the `dst` duplicates within
    /// the specified `Restream`.
    ///
    /// ### Result
    ///
    /// Returns `null` if `Restream` with the given `restreamId` doesn't exist.
    /// Otherwise always returns `true`.
    #[graphql(arguments(
        restream_id(
            description = "ID of the `Restream` to add a new `Output` \
                           to."
        ),
        dst(description = "Destination URL to re-stream a live stream onto.\
                           \n\n\
                           At the moment only [RTMP] and [Icecast] are \
                           supported.\
                           \n\n\
                           [Icecast]: https://icecast.org\n\
                           [RTMP]: https://en.wikipedia.org/wiki/\
                                   Real-Time_Messaging_Protocol"),
        label(description = "Optional label to add a new `Output` with."),
        mix(description = "Optional TeamSpeak URL to mix into this `Output`."),
    ))]
    fn add_output(
        restream_id: RestreamId,
        dst: OutputDstUrl,
        label: Option<Label>,
        mix: Option<MixinSrcUrl>,
        context: &Context,
    ) -> Result<Option<bool>, graphql::Error> {
        let spec = spec::v1::Output {
            dst,
            label,
            volume: Volume::ORIGIN,
            mixins: mix
                .map(|src| {
                    let delay = (src.scheme() == "ts")
                        .then(|| Delay::from_millis(3500))
                        .flatten()
                        .unwrap_or_default();
                    vec![spec::v1::Mixin {
                        src,
                        volume: Volume::ORIGIN,
                        delay,
                    }]
                })
                .unwrap_or_default(),
            enabled: false,
        };

        Ok(context
            .state()
            .add_new_output(restream_id, spec)
            .map_err(|e| {
                graphql::Error::new("DUPLICATE_OUTPUT_URL")
                    .status(StatusCode::CONFLICT)
                    .message(&e)
            })?
            .map(|_| true))
    }

    /// Removes an `Output` by its `id` from the specified `Restream`.
    ///
    /// ### Result
    ///
    /// Returns `null` if the specified `Restream`/`Output` doesn't exist,
    /// otherwise always returns `true`.
    #[graphql(arguments(
        id(description = "ID of the `Output` to be removed."),
        restream_id(description = "ID of the `Restream` to remove the \
                                   `Output` from."),
    ))]
    fn remove_output(
        id: OutputId,
        restream_id: RestreamId,
        context: &Context,
    ) -> Option<bool> {
        context.state().remove_output(id, restream_id).map(|_| true)
    }

    /// Enables an `Output` by its `id` in the specified `Restream`.
    ///
    /// Enabled `Output` starts re-streaming a live stream to its destination.
    ///
    /// ### Result
    ///
    /// Returns `true` if an `Output` with the given `id` has been enabled,
    /// `false` if it has been enabled already, and `null` if the specified
    /// `Restream`/`Output` doesn't exist.
    #[graphql(arguments(
        id(description = "ID of the `Output` to be enabled."),
        restream_id(description = "ID of the `Restream` to enable the \
                                   `Output` in."),
    ))]
    fn enable_output(
        id: OutputId,
        restream_id: RestreamId,
        context: &Context,
    ) -> Option<bool> {
        context.state().enable_output(id, restream_id)
    }

    /// Disables an `Output` by its `id` in the specified `Restream`.
    ///
    /// Disabled `Output` stops re-streaming a live stream to its destination.
    ///
    /// ### Result
    ///
    /// Returns `true` if an `Output` with the given `id` has been disabled,
    /// `false` if it has been disabled already, and `null` if the specified
    /// `Restream`/`Output` doesn't exist.
    #[graphql(arguments(
        id(description = "ID of the `Output` to be disabled."),
        restream_id(description = "ID of the `Restream` to disable the \
                                   `Output` in."),
    ))]
    fn disable_output(
        id: OutputId,
        restream_id: RestreamId,
        context: &Context,
    ) -> Option<bool> {
        context.state().disable_output(id, restream_id)
    }

    /// Enables all `Output`s in the specified `Restream`.
    ///
    /// Enabled `Output`s start re-streaming a live stream to their
    /// destinations.
    ///
    /// ### Result
    ///
    /// Returns `true` if at least one `Output` has been enabled, `false` if all
    /// `Output`s have been enabled already, and `null` if the specified
    /// `Restream` doesn't exist.
    #[graphql(arguments(restream_id(
        description = "ID of the `Restream` to enable all `Output`s in."
    )))]
    fn enable_all_outputs(
        restream_id: RestreamId,
        context: &Context,
    ) -> Option<bool> {
        context.state().enable_all_outputs(restream_id)
    }

    /// Disables all `Output`s in the specified `Restream`.
    ///
    /// Disabled `Output`s stop re-streaming a live stream to their
    /// destinations.
    ///
    /// ### Result
    ///
    /// Returns `true` if at least one `Output` has been disabled, `false` if
    /// all `Output`s have been disabled already, and `null` if the specified
    /// `Restream` doesn't exist.
    #[graphql(arguments(restream_id(
        description = "ID of the `Restream` to disable all `Output`s in."
    )))]
    fn disable_all_outputs(
        restream_id: RestreamId,
        context: &Context,
    ) -> Option<bool> {
        context.state().disable_all_outputs(restream_id)
    }

    /// Tunes a `Volume` rate of the specified `Output` or one of its `Mixin`s.
    ///
    /// ### Result
    ///
    /// Returns `true` if a `Volume` rate has been changed, `false` if it has
    /// the same value already, or `null` if the specified `Output` or `Mixin`
    /// doesn't exist.
    #[graphql(arguments(
        restream_id(description = "ID of the `Restream` to tune the \
                                   `Output` in."),
        output_id(description = "ID of the tuned `Output`."),
        mixin_id(description = "Optional ID of the tuned `Mixin`.\
                                \n\n\
                                If set, then tunes the `Mixin` rather than \
                                the `Output`."),
        volume(description = "Volume rate in percents to be set."),
    ))]
    fn tune_volume(
        restream_id: RestreamId,
        output_id: OutputId,
        mixin_id: Option<MixinId>,
        volume: Volume,
        context: &Context,
    ) -> Option<bool> {
        context
            .state()
            .tune_volume(restream_id, output_id, mixin_id, volume)
    }

    /// Tunes a `Delay` of the specified `Mixin` before mix it into its
    /// `Output`.
    ///
    /// ### Result
    ///
    /// Returns `true` if a `Delay` has been changed, `false` if it has the same
    /// value already, or `null` if the specified `Output` or `Mixin` doesn't
    /// exist.
    #[graphql(arguments(
        restream_id(description = "ID of the `Restream` to tune the the \
                                   `Mixin` in."),
        output_id(description = "ID of the `Output` of the tuned `Mixin`."),
        mixin_id(description = "ID of the tuned `Mixin`."),
        delay(description = "Number of milliseconds to delay the `Mixin` \
                             before mix it into its `Output`."),
    ))]
    fn tune_delay(
        restream_id: RestreamId,
        output_id: OutputId,
        mixin_id: MixinId,
        delay: Delay,
        context: &Context,
    ) -> Option<bool> {
        context
            .state()
            .tune_delay(restream_id, output_id, mixin_id, delay)
    }

    /// Sets or unsets the password to protect this GraphQL API with.
    ///
    /// Once password is set, any subsequent requests to this GraphQL API should
    /// perform [HTTP Basic auth][1], where any username is allowed, but the
    /// password should match the one being set.
    ///
    /// ### Result
    ///
    /// Returns if password has been changed or unset, otherwise `false` if
    /// nothing changes.
    ///
    /// [1]: https://en.wikipedia.org/wiki/Basic_access_authentication
    #[graphql(arguments(
        new(
            description = "New password to be set. In `null` then unsets the \
                           current password."
        ),
        old(description = "Old password for authorization, if it was set \
                           previously."),
    ))]
    fn set_password(
        new: Option<String>,
        old: Option<String>,
        context: &Context,
    ) -> Result<bool, graphql::Error> {
        static HASH_CFG: Lazy<argon2::Config<'static>> =
            Lazy::new(argon2::Config::default);

        let mut current = context.state().password_hash.lock_mut();

        if let Some(hash) = &*current {
            match old {
                None => {
                    return Err(graphql::Error::new("NO_OLD_PASSWORD")
                        .status(StatusCode::FORBIDDEN)
                        .message("Old password required for this action"))
                }
                Some(pass) => {
                    if !argon2::verify_encoded(hash, pass.as_bytes()).unwrap() {
                        return Err(graphql::Error::new("WRONG_OLD_PASSWORD")
                            .status(StatusCode::FORBIDDEN)
                            .message("Wrong old password specified"));
                    }
                }
            }
        }

        if current.is_none() && new.is_none() {
            return Ok(false);
        }

        *current = new.map(|v| {
            argon2::hash_encoded(
                v.as_bytes(),
                &rand::thread_rng().gen::<[u8; 32]>(),
                &*HASH_CFG,
            )
            .unwrap()
        });
        Ok(true)
    }
}

/// Root of all [GraphQL queries][1] in the [`Schema`].
///
/// [1]: https://spec.graphql.org/June2018/#sec-Root-Operation-Types
#[derive(Clone, Copy, Debug)]
pub struct QueriesRoot;

#[graphql_object(name = "Query", context = Context)]
impl QueriesRoot {
    /// Returns the current `Info` parameters of this server.
    fn info(context: &Context) -> Info {
        Info {
            public_host: context.config().public_host.clone().unwrap(),
            password_hash: context.state().password_hash.get_cloned(),
        }
    }

    /// Returns all the `Restream`s happening on this server.
    fn all_restreams(context: &Context) -> Vec<Restream> {
        context.state().restreams.get_cloned()
    }

    /// Returns `Restream`s happening on this server and identifiable by the
    /// given `ids` in an exportable JSON format.
    ///
    /// If no `ids` specified, then returns all the `Restream`s happening on
    /// this server at the moment.
    #[graphql(arguments(ids(
        description = "IDs of `Restream`s to be exported.\
                       \n\n\
                       If empty, then all the `Restream`s will be exported."
        default = Vec::new(),
    )))]
    fn export(
        ids: Vec<RestreamId>,
        context: &Context,
    ) -> Result<Option<String>, graphql::Error> {
        let restreams = context
            .state()
            .restreams
            .get_cloned()
            .into_iter()
            .filter_map(|r| {
                (ids.is_empty() || ids.contains(&r.id)).then(|| r.export())
            })
            .collect::<Vec<_>>();
        (!restreams.is_empty())
            .then(|| {
                let spec: Spec = spec::v1::Spec { restreams }.into();
                serde_json::to_string(&spec).map_err(|e| {
                    anyhow!("Failed to JSON-serialize spec: {}", e).into()
                })
            })
            .transpose()
    }
}

/// Root of all [GraphQL subscriptions][1] in the [`Schema`].
///
/// [1]: https://spec.graphql.org/June2018/#sec-Root-Operation-Types
#[derive(Clone, Copy, Debug)]
pub struct SubscriptionsRoot;

#[graphql_subscription(name = "Subscription", context = Context)]
impl SubscriptionsRoot {
    /// Subscribes to updates of `Info` parameters of this server.
    async fn info(context: &Context) -> BoxStream<'static, Info> {
        let public_host = context.config().public_host.clone().unwrap();
        context
            .state()
            .password_hash
            .signal_cloned()
            .dedupe_cloned()
            .map(move |h| Info {
                public_host: public_host.clone(),
                password_hash: h,
            })
            .to_stream()
            .boxed()
    }

    /// Subscribes to updates of all `Restream`s happening on this server.
    async fn all_restreams(
        context: &Context,
    ) -> BoxStream<'static, Vec<Restream>> {
        context
            .state()
            .restreams
            .signal_cloned()
            .dedupe_cloned()
            .to_stream()
            .boxed()
    }
}

/// Information about parameters that this server operates with.
#[derive(Clone, Debug, GraphQLObject)]
pub struct Info {
    /// Host that this server is reachable via in public.
    ///
    /// Use it for constructing URLs to this server.
    pub public_host: String,

    /// [Argon2] hash of the password that this server's GraphQL API is
    /// protected with, if any.
    ///
    /// Non-`null` value means that any request to GraphQL API should perform
    /// [HTTP Basic auth][1]. Any username is allowed, but the password should
    /// match this hash.
    ///
    /// [Argon2]: https://en.wikipedia.org/wiki/Argon2
    /// [1]: https://en.wikipedia.org/wiki/Basic_access_authentication
    pub password_hash: Option<String>,
}
