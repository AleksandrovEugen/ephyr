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
    state::{
        Delay, InputId, InputName, InputSrcUrl, Label, MixinId, MixinSrcUrl,
        OutputDstUrl, OutputId, Restream, Volume,
    },
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

/// Root of all [GraphQL mutations][1] in [`Schema`].
///
/// [1]: https://spec.graphql.org/June2018/#sec-Root-Operation-Types
#[derive(Clone, Copy, Debug)]
pub struct MutationsRoot;

#[graphql_object(name = "Mutations", context = Context)]
impl MutationsRoot {
    /// Adds a new `Restream` with a `PullInput`.
    ///
    /// If `id` is specified, then tries to update parameters of the existent
    /// `Restream`.
    ///
    /// ### Idempotency
    ///
    /// Idempotent if `id` is specified. Otherwise is non-idempotent, always
    /// creates a new `Restream` and errors on the `src` duplicates.
    ///
    /// ### Result
    ///
    /// Returns `null` if `Restream` with the given `id` doesn't exist.
    /// Otherwise always returns `true`.
    #[graphql(arguments(
        src(description = "RTMP URL to pull media stream from."),
        label(description = "Optional label for this `Restream`."),
        id(
            description = "ID of `Restream` to be updated rather than creating \
                           a new one."
        ),
    ))]
    fn add_pull_input(
        src: InputSrcUrl,
        label: Option<Label>,
        id: Option<InputId>,
        context: &Context,
    ) -> Result<Option<bool>, graphql::Error> {
        match context.state().add_pull_input(src, label, id) {
            None => Ok(None),
            Some(true) => Ok(Some(true)),
            Some(false) => Err(graphql::Error::new("DUPLICATE_SRC_RTMP_URL")
                .status(StatusCode::CONFLICT)
                .message("Provided `src` is used already")),
        }
    }

    /// Adds a new `Restream` with a `PushInput` (or `FailoverPushInput`).
    ///
    /// If `failover` is `true`, then uses a `FailoverPushInput` input instead
    /// of a regular `PushInput`.
    ///
    /// If `id` is specified, then tries to update parameters of the existent
    /// `Restream`.
    ///
    /// ### Idempotency
    ///
    /// Idempotent if `id` is specified. Otherwise is non-idempotent, always
    /// creates a new `Restream` and errors on the `name` duplicates.
    ///
    /// ### Result
    ///
    /// Returns `null` if `Restream` with the given `id` doesn't exist.
    /// Otherwise always returns `true`.
    #[graphql(arguments(
        name(description = "Name of RTMP media stream used in its URL."),
        label(description = "Optional label for this `Restream`."),
        failover(
            description = "Indicator whether a `FailoverPushInput` should be \
                           used instead of a regular `PushInput`.",
            default = false,
        ),
        id(
            description = "ID of `Restream` to be updated rather than creating \
                           a new one."
        ),
    ))]
    fn add_push_input(
        name: InputName,
        label: Option<Label>,
        failover: bool,
        id: Option<InputId>,
        context: &Context,
    ) -> Result<Option<bool>, graphql::Error> {
        match context.state().add_push_input(name, failover, label, id) {
            None => Ok(None),
            Some(true) => Ok(Some(true)),
            Some(false) => Err(graphql::Error::new("DUPLICATE_INPUT_NAME")
                .status(StatusCode::CONFLICT)
                .message("Provided `name` is used already")),
        }
    }

    /// Removes `Restream` by its `id`.
    ///
    /// ### Result
    ///
    /// Returns `true` if `Restream` with the given `id` has been removed, or
    /// `false` if it doesn't exist.
    #[graphql(arguments(id(description = "ID of `Restream` to be removed.")))]
    fn remove_input(id: InputId, context: &Context) -> bool {
        context.state().remove_input(id)
    }

    /// Enables `Restream` by its `id`.
    ///
    /// Enabled `Restream` starts accepting or pulling media traffic.
    ///
    /// ### Result
    ///
    /// Returns `true` if `Restream` with the given `id` has been enabled,
    /// `false` if it has been enabled already, and `null` if it doesn't exist.
    #[graphql(arguments(id(description = "ID of `Restream` to be enabled.")))]
    fn enable_input(id: InputId, context: &Context) -> Option<bool> {
        context.state().enable_input(id)
    }

    /// Disables `Restream` by its `id`.
    ///
    /// Disabled `Restream` stops and forbids accepting or pulling media
    /// traffic.
    ///
    /// ### Result
    ///
    /// Returns `true` if `Restream` with the given `id` has been disabled,
    /// `false` if it has been disabled already, and `null` if it doesn't exist.
    #[graphql(arguments(id(
        description = "ID of `Restream` to be disabled."
    )))]
    fn disable_input(id: InputId, context: &Context) -> Option<bool> {
        context.state().disable_input(id)
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
    /// Returns `null` if `Restream` with the given `inputId` doesn't exist.
    /// Otherwise always returns `true`.
    #[graphql(arguments(
        input_id(description = "ID of `Restream` to add `Output` to."),
        dst(description = "\
        URL to push media stream onto.\
        \n\n\
        At the moment only [RTMP] and [Icecast] are supported.\
        \n\n\
        [Icecast]: https://icecast.org\n\
        [RTMP]: https://en.wikipedia.org/wiki/Real-Time_Messaging_Protocol"),
        label(description = "Optional label for this `Output`."),
        mix(description = "Optional TeamSpeak URL to mix into this `Output`."),
    ))]
    fn add_output(
        input_id: InputId,
        dst: OutputDstUrl,
        label: Option<Label>,
        mix: Option<MixinSrcUrl>,
        context: &Context,
    ) -> Result<Option<bool>, graphql::Error> {
        context
            .state()
            .add_new_output(input_id, dst, label, mix)
            .map(|added| {
                if added {
                    Ok(added)
                } else {
                    Err(graphql::Error::new("DUPLICATE_DST_RTMP_URL")
                        .status(StatusCode::CONFLICT)
                        .message(
                            "Provided `dst` is used already for this input",
                        ))
                }
            })
            .transpose()
    }

    /// Removes `Output` by its ID from the specified `Restream`.
    ///
    /// ### Result
    ///
    /// Returns `true` if `Output` with the given `id` has been removed,
    /// `false` if it has been removed already, and `null` if the specified
    /// `Restream` doesn't exist.
    #[graphql(arguments(
        input_id(description = "ID of `Restream` to remove `Output` from."),
        output_id(description = "ID of `Output` to be removed."),
    ))]
    fn remove_output(
        input_id: InputId,
        output_id: OutputId,
        context: &Context,
    ) -> Option<bool> {
        context.state().remove_output(input_id, output_id)
    }

    /// Enables `Output` by its ID in the specified `Restream`.
    ///
    /// Enabled `Output` starts pushing media traffic to its destination.
    ///
    /// ### Result
    ///
    /// Returns `true` if `Output` with the given `id` has been enabled,
    /// `false` if it has been enabled already, and `null` if the specified
    /// `Restream`/`Output` doesn't exist.
    #[graphql(arguments(
        input_id(description = "ID of `Restream` to enable `Output` in."),
        output_id(description = "ID of `Output` to be enabled."),
    ))]
    fn enable_output(
        input_id: InputId,
        output_id: OutputId,
        context: &Context,
    ) -> Option<bool> {
        context.state().enable_output(input_id, output_id)
    }

    /// Disables `Output` by its ID in the specified `Restream`.
    ///
    /// Disabled `Output` stops pushing media traffic to its destination.
    ///
    /// ### Result
    ///
    /// Returns `true` if `Output` with the given `id` has been disabled,
    /// `false` if it has been disabled already, and `null` if the specified
    /// `Restream`/`Output` doesn't exist.
    #[graphql(arguments(
        input_id(description = "ID of `Restream` to disable `Output` in."),
        output_id(description = "ID of `Output` to be disabled."),
    ))]
    fn disable_output(
        input_id: InputId,
        output_id: OutputId,
        context: &Context,
    ) -> Option<bool> {
        context.state().disable_output(input_id, output_id)
    }

    /// Enables all `Output`s in the specified `Restream`.
    ///
    /// Enabled `Output`s start pushing media traffic to their destinations.
    ///
    /// ### Result
    ///
    /// Returns `true` if at least `Output`has been enabled, `false` if all
    /// `Output`s have been enabled already, and `null` if the specified
    /// `Restream` doesn't exist.
    #[graphql(arguments(input_id(
        description = "ID of `Restream` to enable all `Output`s in."
    )))]
    fn enable_all_outputs(
        input_id: InputId,
        context: &Context,
    ) -> Option<bool> {
        context.state().enable_all_outputs(input_id)
    }

    /// Disables all `Output`s in the specified `Restream`.
    ///
    /// Disabled `Output`s stop pushing media traffic to their destinations.
    ///
    /// ### Result
    ///
    /// Returns `true` if at least `Output` has been disabled, `false` if all
    /// `Output`s have been disabled already, and `null` if the specified
    /// `Restream` doesn't exist.
    #[graphql(arguments(input_id(
        description = "ID of `Restream` to disable all `Output`s in."
    )))]
    fn disable_all_outputs(
        input_id: InputId,
        context: &Context,
    ) -> Option<bool> {
        context.state().disable_all_outputs(input_id)
    }

    /// Tunes a `Volume` rate of the specified `Output` or one of its `Mixin`s.
    ///
    /// ### Result
    ///
    /// Returns `true` if a `Volume` rate has been changed, `false` if it has
    /// the same value already, and `null` if the specified `Output` or `Mixin`
    /// doesn't exist.
    #[graphql(arguments(
        input_id(description = "ID of `Restream` of the tuned `Output`."),
        output_id(description = "ID of the tuned `Output`."),
        mixin_id(description = "\
        Optional ID of the tuned `Mixin`.\
        \n\n\
        If set, then tunes the `Mixin` rather that the `Output`."),
        delay(description = "Volume rate in percents to be set."),
    ))]
    fn tune_volume(
        input_id: InputId,
        output_id: OutputId,
        mixin_id: Option<MixinId>,
        volume: Volume,
        context: &Context,
    ) -> Option<bool> {
        context
            .state()
            .tune_volume(input_id, output_id, mixin_id, volume)
    }

    /// Tunes a `Delay` before being mixed in of the specified `Mixin` in the
    /// specified `Output`.
    ///
    /// ### Result
    ///
    /// Returns `true` if a `Delay` has been changed, `false` if it has the same
    /// value already, and `null` if the specified `Output` or `Mixin` doesn't
    /// exist.
    #[graphql(arguments(
        input_id(description = "ID of `Restream` of the tuned `Mixin`."),
        output_id(description = "ID of `Output` of the tuned `Mixin`."),
        mixin_id(description = "ID of the tuned `Mixin`."),
        delay(description = "Number of milliseconds to delay the `Mixin` \
                             before being mixed in."),
    ))]
    fn tune_delay(
        input_id: InputId,
        output_id: OutputId,
        mixin_id: MixinId,
        delay: Delay,
        context: &Context,
    ) -> Option<bool> {
        context
            .state()
            .tune_delay(input_id, output_id, mixin_id, delay)
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

/// Root of all [GraphQL subscriptions][1] in [`Schema`].
///
/// [1]: https://spec.graphql.org/June2018/#sec-Root-Operation-Types
#[derive(Clone, Copy, Debug)]
pub struct SubscriptionsRoot;

/// Root of all [GraphQL queries][1] in [`Schema`].
///
/// [1]: https://spec.graphql.org/June2018/#sec-Root-Operation-Types
#[derive(Clone, Copy, Debug)]
pub struct QueriesRoot;

#[graphql_object(name = "Queries", context = Context)]
impl QueriesRoot {
    /// Returns current `Info` parameters of this server.
    fn info(context: &Context) -> Info {
        Info {
            public_host: context.config().public_host.clone().unwrap(),
            password_hash: context.state().password_hash.get_cloned(),
        }
    }

    /// Returns all `Restream`s happening on this server.
    fn all_restreams(context: &Context) -> Vec<Restream> {
        context.state().restreams.get_cloned()
    }

    /// Returns all `Restream`s happening on this server in an exportable JSON
    /// format.
    fn export_all_restreams(
        context: &Context,
    ) -> Result<String, graphql::Error> {
        let exported = context
            .state()
            .restreams
            .get_cloned()
            .iter()
            .map(Restream::export)
            .collect::<Vec<_>>();
        serde_json::to_string(&exported)
            .map_err(|e| anyhow!("Failed to JSON-serialize spec: {}", e).into())
    }

    /// Returns a single `Restream` happening on this server in an exportable
    /// JSON format.
    #[graphql(arguments(input_id(
        description = "ID of the `Restream` to be exported."
    )))]
    fn export_restream(
        input_id: InputId,
        context: &Context,
    ) -> Result<Option<String>, graphql::Error> {
        context
            .state()
            .restreams
            .get_cloned()
            .into_iter()
            .find_map(|r| {
                (r.id == input_id).then(|| {
                    serde_json::to_string(&r.export()).map_err(|e| {
                        anyhow!("Failed to JSON-serialize spec: {}", e).into()
                    })
                })
            })
            .transpose()
    }
}

#[graphql_subscription(name = "Subscriptions", context = Context)]
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

/// Information about parameters that server operates with.
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
