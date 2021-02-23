//! Shareable (exportable and importable) specification of application
//! [`State`].
//!
//! [`State`]: crate::state::State

use derive_more::From;
use serde::{Deserialize, Serialize};

pub use crate::state::{
    Delay, InputName, InputSrcUrl, Label, MixinSrcUrl, OutputDstUrl, Volume,
};

/// Shareable (exportable and importable) specification of a
/// [`state::Restream`].
///
/// [`state::Restream`]: crate::state::Restream
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Restream {
    /// Optional label of this [`Restream`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,

    /// Indicator whether this [`Restream`] is enabled, so allows to receive a
    /// live stream from its [`Input`].
    pub enabled: bool,

    /// [`Input`] that a live stream is received from.
    pub input: Input,

    /// [`Output`]s that a live stream is re-streamed to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Output>,
}

/// Shareable (exportable and importable) specification of a [`state::Input`].
///
/// [`state::Input`]: crate::state::Input
#[derive(Clone, Debug, Deserialize, Eq, From, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum Input {
    /// Receiving a live stream from an external client.
    Push(PushInput),

    /// Receiving a live stream from an external client with an additional
    /// backup endpoint.
    FailoverPush(FailoverPushInput),

    /// Pulling a live stream from a remote server.
    Pull(PullInput),
}

/// Shareable (exportable and importable) specification of a
/// [`state::PushInput`].
///
/// [`state::PushInput`]: crate::state::PushInput
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PushInput {
    /// Name of a live stream to expose its endpoints with for receiving and
    /// re-streaming media traffic.
    pub name: InputName,
}

/// Shareable (exportable and importable) specification of a
/// [`state::FailoverPushInput`].
///
/// [`state::FailoverPushInput`]: crate::state::FailoverPushInput
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FailoverPushInput {
    /// Name of a live stream to expose its endpoints with for receiving and
    /// re-streaming media traffic.
    pub name: InputName,
}

/// Shareable (exportable and importable) specification of a
/// [`state::PullInput`].
///
/// [`state::PullInput`]: crate::state::PullInput
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PullInput {
    /// URL of a live stream to be pulled from.
    pub src: InputSrcUrl,
}

/// Shareable (exportable and importable) specification of a [`state::Output`].
///
/// [`state::Output`]: crate::state::Output
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Output {
    /// URL to push a live stream onto.
    pub dst: OutputDstUrl,

    /// Optional label of this [`Output`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Label>,

    /// Indicator whether this [`Output`] is enabled, so performs a live stream
    /// re-streaming to its destination.
    pub enabled: bool,

    /// Volume rate of this [`Output`]'s audio tracks when mixed with
    /// [`Output::mixins`].
    #[serde(default, skip_serializing_if = "Volume::is_origin")]
    pub volume: Volume,

    /// [`Mixin`]s to mix this [`Output`] with before re-streaming to its
    /// destination.
    ///
    /// If empty, then no mixing is performed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mixins: Vec<Mixin>,
}

/// Shareable (exportable and importable) specification of a [`state::Mixin`].
///
/// [`state::Mixin`]: crate::state::Mixin
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Mixin {
    /// URL of the source to be mixed in.
    pub src: MixinSrcUrl,

    /// Volume rate of this [`Mixin`]'s audio tracks to mix them with.
    #[serde(default, skip_serializing_if = "Volume::is_origin")]
    pub volume: Volume,

    /// Delay that this [`Mixin`] should wait before being mixed with an
    /// [`Output`].
    #[serde(default, skip_serializing_if = "Delay::is_zero")]
    pub delay: Delay,
}

#[cfg(test)]
mod spec {
    use url::Url;

    use super::{
        Delay, InputName, Label, Mixin, MixinSrcUrl, Output, OutputDstUrl,
        PushInput, Restream, Volume,
    };

    #[test]
    fn serializes_deserializes() {
        let deserialized = Restream {
            label: Label::new("My input"),
            enabled: false,
            input: PushInput {
                name: InputName::new("test").unwrap(),
            }
            .into(),
            outputs: vec![
                Output {
                    dst: OutputDstUrl::new(
                        Url::parse("icecast://127.0.0.1/hi").unwrap(),
                    )
                    .unwrap(),
                    label: None,
                    enabled: true,
                    volume: Volume::ORIGIN,
                    mixins: vec![],
                },
                Output {
                    dst: OutputDstUrl::new(
                        Url::parse("rtmp://127.0.0.1/test2/in").unwrap(),
                    )
                    .unwrap(),
                    label: Label::new("Second out"),
                    enabled: false,
                    volume: Volume::ORIGIN,
                    mixins: vec![],
                },
                Output {
                    dst: OutputDstUrl::new(
                        Url::parse("rtmps://127.0.0.1/test5/in").unwrap(),
                    )
                    .unwrap(),
                    label: Label::new("Mixed TeamSpeak"),
                    enabled: false,
                    volume: Volume::OFF,
                    mixins: vec![Mixin {
                        src: MixinSrcUrl::new(
                            Url::parse("ts://127.0.0.1/chan?name=some")
                                .unwrap(),
                        )
                        .unwrap(),
                        volume: Volume::MAX,
                        delay: Delay::default(),
                    }],
                },
            ],
        };

        let serialized = "{\
            \"label\":\"My input\",\
            \"enabled\":false,\
            \"input\":{\
                \"type\":\"push\",\
                \"name\":\"test\"\
            },\
            \"outputs\":[{\
                \"dst\":\"icecast://127.0.0.1/hi\",\
                \"enabled\":true\
            },{\
                \"dst\":\"rtmp://127.0.0.1/test2/in\",\
                \"label\":\"Second out\",\
                \"enabled\":false\
            },{\
                \"dst\":\"rtmps://127.0.0.1/test5/in\",\
                \"label\":\"Mixed TeamSpeak\",\
                \"enabled\":false,\
                \"volume\":0,\
                \"mixins\":[{\
                    \"src\":\"ts://127.0.0.1/chan?name=some\",\
                    \"volume\":1000\
                }]\
            }]\
        }";

        assert_eq!(serde_json::to_string(&deserialized).unwrap(), serialized);
        assert_eq!(
            serde_json::from_str::<Restream>(serialized).unwrap(),
            deserialized,
        );
    }
}
