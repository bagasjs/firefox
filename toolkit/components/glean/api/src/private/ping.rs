// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use inherent::inherent;

use crate::ipc::need_ipc;

/// A Glean ping.
///
/// See [Glean Pings](https://mozilla.github.io/glean/book/user/pings/index.html).
#[derive(Clone)]
pub enum Ping {
    Parent {
        inner: glean::private::PingType,
        name: String,
    },
    Child,
}

impl malloc_size_of::MallocSizeOf for Ping {
    fn size_of(&self, ops: &mut malloc_size_of::MallocSizeOfOps) -> usize {
        match self {
            Ping::Child => 0,
            Ping::Parent { inner, .. } => inner.size_of(ops),
        }
    }
}

impl Ping {
    /// Create a new ping type for the given name, whether to include the client ID and whether to
    /// send this ping empty.
    ///
    /// ## Arguments
    ///
    /// * `name` - The name of the ping.
    /// * `include_client_id` - Whether to include the client ID in the assembled ping when submitting.
    /// * `send_if_empty` - Whether the ping should be sent empty or not.
    /// * `reason_codes` - The valid reason codes for this ping.
    pub fn new<S: Into<String>>(
        name: S,
        include_client_id: bool,
        send_if_empty: bool,
        precise_timestamps: bool,
        include_info_sections: bool,
        enabled: bool,
        schedules_pings: Vec<String>,
        reason_codes: Vec<String>,
        follows_collection_enabled: bool,
        uploader_capabilities: Vec<String>,
    ) -> Self {
        if need_ipc() {
            Ping::Child
        } else {
            let name = name.into();
            Ping::Parent {
                inner: glean::private::PingType::new(
                    name.clone(),
                    include_client_id,
                    send_if_empty,
                    precise_timestamps,
                    include_info_sections,
                    enabled,
                    schedules_pings,
                    reason_codes,
                    follows_collection_enabled,
                    uploader_capabilities,
                ),
                name,
            }
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        match self {
            Ping::Parent { inner, .. } => inner.set_enabled(enabled),
            Ping::Child => {
                panic!("Cannot use ping set_enabled API from non-parent process!");
            }
        }
    }

    /// **Test-only API**
    ///
    /// Attach a callback to be called right before a new ping is submitted.
    /// The provided function is called exactly once before submitting a ping.
    ///
    /// Note: The callback will be called on any call to submit.
    /// A ping might not be sent afterwards, e.g. if the ping is otherwise empty (and
    /// `send_if_empty` is `false`).
    pub fn test_before_next_submit(&self, cb: impl FnOnce(Option<&str>) + Send + 'static) {
        match self {
            Ping::Parent { inner, .. } => inner.test_before_next_submit(cb),
            Ping::Child => {
                panic!("Cannot use ping test API from non-parent process!");
            }
        };
    }
}

#[inherent]
impl glean::traits::Ping for Ping {
    /// Submits the ping for eventual uploading
    ///
    /// # Arguments
    ///
    /// * `reason` - the reason the ping was triggered. Included in the
    ///   `ping_info.reason` part of the payload.
    pub fn submit(&self, reason: Option<&str>) {
        match self {
            #[allow(unused)]
            Ping::Parent { inner, name } => {
                #[cfg(feature = "with_gecko")]
                if gecko_profiler::can_accept_markers() {
                    use gecko_profiler::gecko_profiler_category;
                    gecko_profiler::add_marker(
                        "Ping::submit",
                        super::profiler_utils::TelemetryProfilerCategory,
                        Default::default(),
                        super::profiler_utils::PingMarker::new_for_submit(
                            name.clone(),
                            reason.map(str::to_string),
                        ),
                    );
                }
                inner.submit(reason);
            }
            Ping::Child => {
                log::error!(
                    "Unable to submit ping in non-main process. This operation will be ignored."
                );
                // If we're in automation we can panic so the instrumentor knows they've gone wrong.
                // This is a deliberate violation of Glean's "metric APIs must not throw" design.
                assert!(!crate::ipc::is_in_automation(), "Attempted to submit a ping in non-main process, which is forbidden. This panics in automation.");
                // TODO: Record an error.
            }
        };
    }
}

#[cfg(test)]
mod test {
    use once_cell::sync::Lazy;

    use super::*;
    use crate::common_test::*;

    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    // Smoke test for what should be the generated code.
    static PROTOTYPE_PING: Lazy<Ping> = Lazy::new(|| {
        Ping::new(
            "prototype",
            false,
            true,
            true,
            true,
            true,
            vec![],
            vec![],
            true,
            vec![],
        )
    });

    #[test]
    fn smoke_test_custom_ping() {
        let _lock = lock_test();

        let called = Arc::new(AtomicBool::new(false));
        let rcalled = Arc::clone(&called);
        PROTOTYPE_PING.test_before_next_submit(move |reason| {
            (*rcalled).store(true, Ordering::Relaxed);
            assert_eq!(None, reason);
        });
        PROTOTYPE_PING.submit(None);
        assert!((*called).load(Ordering::Relaxed));
    }
}
