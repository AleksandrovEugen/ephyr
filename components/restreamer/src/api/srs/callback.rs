//! [HTTP Callback API][1] of [SRS] exposed by application.
//!
//! [SRS]: https://github.com/ossrs/srs
//! [1]: https://github.com/ossrs/srs/wiki/v3_EN_HTTPCallback

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// Request performed by [SRS] to [HTTP Callback API][1].
///
/// [SRS]: https://github.com/ossrs/srs
/// [1]: https://github.com/ossrs/srs/wiki/v3_EN_HTTPCallback
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Request {
    /// Event that [SRS] reports about.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    pub action: Event,

    /// ID of [SRS] client that happened event is related to.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    pub client_id: u32,

    /// IP address of [SRS] client that happened event is related to.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    pub ip: IpAddr,

    /// [SRS] `app` of RTMP stream that happened event is related to.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    pub app: String,

    /// [SRS] `stream` of RTMP stream that happened event is related to.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
}

/// Possible [SRS] events in [HTTP Callback API][1] that this application reacts
/// onto.
///
/// [SRS]: https://github.com/ossrs/srs
/// [1]: https://github.com/ossrs/srs/wiki/v3_EN_HTTPCallback
#[allow(clippy::pub_enum_variant_names)]
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// [SRS] client connects to [SRS] `app`.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    OnConnect,

    /// [SRS] client publishes new RTMP stream.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    OnPublish,

    /// [SRS] client stops publishing RTMP stream.
    ///
    /// [SRS]: https://github.com/ossrs/srs
    OnUnpublish,
}
