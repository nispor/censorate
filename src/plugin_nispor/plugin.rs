// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex};

use nipart::{
    MergedNetworkState, NetworkState, NipartApplyOption, NipartConnection,
    NipartError, NipartEvent, NipartEventAction, NipartEventAddress,
    NipartPlugin, NipartPluginEvent, NipartRole, NipartUserEvent,
    DEFAULT_TIMEOUT,
};
use tokio::sync::mpsc::Sender;

use crate::{nispor_apply, nispor_retrieve};

const STATE_PRIORITY: u32 = 50;

#[derive(Debug, Default)]
struct NipartPluginNisporShareData {}

impl NipartPluginNisporShareData {
    fn _clear(&mut self) {}
}

#[derive(Debug)]
pub(crate) struct NipartPluginNispor {
    socket_path: String,
    _data: Mutex<NipartPluginNisporShareData>,
}

impl NipartPlugin for NipartPluginNispor {
    const PLUGIN_NAME: &'static str = "nispor";
    const LOG_SUFFIX: &'static str = " (plugin nispor)\n";

    fn get_socket_path(&self) -> &str {
        self.socket_path.as_str()
    }

    fn roles(&self) -> Vec<NipartRole> {
        vec![NipartRole::QueryAndApply]
    }

    async fn init(socket_path: &str) -> Result<Self, NipartError> {
        Ok(Self {
            socket_path: socket_path.to_string(),
            _data: Mutex::new(NipartPluginNisporShareData::default()),
        })
    }

    async fn handle_event(
        _plugin: &Arc<Self>,
        to_daemon: &Sender<NipartEvent>,
        event: NipartEvent,
    ) -> Result<(), NipartError> {
        log::debug!("Plugin nispor got event {event}");
        log::trace!("Plugin nispor got event {event:?}");
        match event.plugin {
            NipartPluginEvent::QueryNetState(_) => {
                let state = nispor_retrieve(false).await?;
                let mut reply = NipartEvent::new(
                    NipartEventAction::Done,
                    NipartUserEvent::None,
                    NipartPluginEvent::QueryNetStateReply(
                        Box::new(state),
                        STATE_PRIORITY,
                    ),
                    NipartEventAddress::Unicast(Self::PLUGIN_NAME.to_string()),
                    NipartEventAddress::Commander,
                    DEFAULT_TIMEOUT,
                );
                reply.uuid = event.uuid;
                to_daemon.send(reply).await?;
                Ok(())
            }
            // TODO: Currently, we are returning full state, but we should
            // return       only related network state back
            NipartPluginEvent::QueryRelatedNetState(_) => {
                let state = nispor_retrieve(false).await?;
                let mut reply = NipartEvent::new(
                    NipartEventAction::Done,
                    event.user.clone(),
                    NipartPluginEvent::QueryRelatedNetStateReply(
                        Box::new(state),
                        STATE_PRIORITY,
                    ),
                    NipartEventAddress::Unicast(Self::PLUGIN_NAME.to_string()),
                    NipartEventAddress::Commander,
                    DEFAULT_TIMEOUT,
                );
                reply.uuid = event.uuid;
                to_daemon.send(reply).await?;
                Ok(())
            }
            NipartPluginEvent::ApplyNetState(merged_state, opt) => {
                // We spawn new thread for apply instead of blocking
                // here
                let to_daemon_clone = to_daemon.clone();
                tokio::spawn(async move {
                    handle_apply(
                        *merged_state,
                        opt,
                        to_daemon_clone,
                        event.uuid,
                    )
                    .await
                });
                Ok(())
            }
            _ => {
                log::warn!("Plugin nispor got unknown event {event:?}");
                Ok(())
            }
        }
    }
}

async fn handle_apply(
    merged_state: MergedNetworkState,
    opt: NipartApplyOption,
    to_daemon: Sender<NipartEvent>,
    uuid: u128,
) {
    let mut reply = match nispor_apply(merged_state, opt).await {
        Ok(()) => NipartEvent::new(
            NipartEventAction::Done,
            NipartUserEvent::None,
            NipartPluginEvent::ApplyNetStateReply,
            NipartEventAddress::Unicast(
                NipartPluginNispor::PLUGIN_NAME.to_string(),
            ),
            NipartEventAddress::Commander,
            DEFAULT_TIMEOUT,
        ),
        Err(e) => NipartEvent::new(
            NipartEventAction::Done,
            NipartUserEvent::Error(e),
            NipartPluginEvent::ApplyNetStateReply,
            NipartEventAddress::Unicast(
                NipartPluginNispor::PLUGIN_NAME.to_string(),
            ),
            NipartEventAddress::Commander,
            DEFAULT_TIMEOUT,
        ),
    };
    reply.uuid = uuid;
    log::trace!("Sending reply {reply:?}");
    if let Err(e) = to_daemon.send(reply).await {
        log::error!("Failed to reply {e}")
    }
}
