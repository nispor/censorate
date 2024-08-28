// SPDX-License-Identifier: Apache-2.0

use nipart::{
    NipartError, NipartEvent, NipartEventAddress, NipartLogEntry,
    NipartLogLevel, NipartPluginEvent, NipartUserEvent,
};
use tokio::sync::mpsc::{Receiver, Sender};

use super::{WorkFlow, WorkFlowQueue};
use crate::PluginRoles;

// Check the session queue every 5 seconds
const WORKFLOW_QUEUE_CHECK_INTERVAL: u64 = 5000;

pub(crate) async fn start_commander_thread(
    commander_to_switch: Sender<NipartEvent>,
    switch_to_commander: Receiver<NipartEvent>,
    plugin_roles: PluginRoles,
) -> Result<(), NipartError> {
    tokio::spawn(async move {
        commander_thread(
            commander_to_switch,
            switch_to_commander,
            plugin_roles,
        )
        .await;
    });
    log::debug!("Commander started");
    Ok(())
}

async fn commander_thread(
    mut commander_to_switch: Sender<NipartEvent>,
    mut switch_to_commander: Receiver<NipartEvent>,
    plugin_roles: PluginRoles,
) {
    let mut workflow_queue = WorkFlowQueue::new();

    let mut workflow_queue_check_interval = tokio::time::interval(
        std::time::Duration::from_millis(WORKFLOW_QUEUE_CHECK_INTERVAL),
    );

    // The first tick just completes instantly
    workflow_queue_check_interval.tick().await;

    loop {
        if let Err(e) = tokio::select! {
            _ = workflow_queue_check_interval.tick() => {
                process_workflow_queue(
                    &mut workflow_queue, &mut commander_to_switch).await
            }
            Some(event) = switch_to_commander.recv() => {
                log_to_user(event.uuid,
                    NipartLogLevel::Debug,
                    format!("Received event {event}"),
                    &commander_to_switch).await;
                log_to_user(event.uuid,
                    NipartLogLevel::Trace,
                    format!("Received event {event:?}"),
                    &commander_to_switch).await;
                process_event(
                    event,
                    &mut workflow_queue,
                    &mut commander_to_switch,
                    &plugin_roles).await
            }
        } {
            log::error!("{e}");
        }
    }
}

async fn process_workflow_queue(
    workflow_queue: &mut WorkFlowQueue,
    commander_to_switch: &mut Sender<NipartEvent>,
) -> Result<(), NipartError> {
    for event in workflow_queue.process()? {
        log_to_user(
            event.uuid,
            NipartLogLevel::Debug,
            format!("Send event {event}"),
            commander_to_switch,
        )
        .await;
        log_to_user(
            event.uuid,
            NipartLogLevel::Trace,
            format!("Sent event {event:?}"),
            commander_to_switch,
        )
        .await;
        if let Err(e) = commander_to_switch.send(event).await {
            log::error!("{e}");
        }
    }
    Ok(())
}

async fn process_event(
    event: NipartEvent,
    workflow_queue: &mut WorkFlowQueue,
    commander_to_switch: &mut Sender<NipartEvent>,
    plugin_roles: &PluginRoles,
) -> Result<(), NipartError> {
    if event.plugin != NipartPluginEvent::None {
        process_plugin_event(
            event,
            workflow_queue,
            commander_to_switch,
            plugin_roles,
        )
        .await?;
    } else {
        process_user_event(
            event,
            workflow_queue,
            commander_to_switch,
            plugin_roles,
        )
        .await?;
    }
    Ok(())
}

async fn process_plugin_event(
    event: NipartEvent,
    workflow_queue: &mut WorkFlowQueue,
    commander_to_switch: &mut Sender<NipartEvent>,
    plugin_roles: &PluginRoles,
) -> Result<(), NipartError> {
    if event.plugin.is_reply() {
        workflow_queue.add_reply(event);
        process_workflow_queue(workflow_queue, commander_to_switch).await
    } else {
        match event.plugin {
            NipartPluginEvent::GotDhcpLease(lease) => {
                log_to_user(
                    event.uuid,
                    NipartLogLevel::Debug,
                    format!("Got DHCP {lease:?}"),
                    commander_to_switch,
                )
                .await;

                let (workflow, share_data) = WorkFlow::new_apply_dhcp_lease(
                    event.uuid,
                    *lease,
                    plugin_roles,
                    event.timeout,
                );
                workflow_queue.add_workflow(workflow, share_data);
                process_workflow_queue(workflow_queue, commander_to_switch)
                    .await?;
            }
            _ => {
                log::error!("Unknown user event {event:?}");
            }
        }
        Ok(())
    }
}

async fn process_user_event(
    event: NipartEvent,
    workflow_queue: &mut WorkFlowQueue,
    commander_to_switch: &mut Sender<NipartEvent>,
    plugin_roles: &PluginRoles,
) -> Result<(), NipartError> {
    let all_plugins_count = plugin_roles.all_plugin_count();
    let (workflow, share_data) = match event.user {
        NipartUserEvent::QueryPluginInfo => WorkFlow::new_query_plugin_info(
            event.uuid,
            all_plugins_count,
            event.timeout,
        ),
        NipartUserEvent::QueryLogLevel => WorkFlow::new_query_log_level(
            event.uuid,
            all_plugins_count,
            event.timeout,
        ),
        NipartUserEvent::ChangeLogLevel(l) => WorkFlow::new_change_log_level(
            l,
            event.uuid,
            all_plugins_count,
            event.timeout,
        ),
        NipartUserEvent::Quit => {
            WorkFlow::new_quit(event.uuid, all_plugins_count, event.timeout)
        }
        NipartUserEvent::QueryNetState(opt) => WorkFlow::new_query_net_state(
            opt,
            event.uuid,
            plugin_roles,
            event.timeout,
        ),
        NipartUserEvent::ApplyNetState(des, opt) => {
            WorkFlow::new_apply_net_state(
                *des,
                opt,
                event.uuid,
                plugin_roles,
                event.timeout,
            )
        }
        NipartUserEvent::QueryCommits(opt) => {
            WorkFlow::new_query_commits(opt, event.uuid, event.timeout)
        }
        _ => {
            log::error!("Unknown user event {event:?}");
            return Ok(());
        }
    };
    workflow_queue.add_workflow(workflow, share_data);
    process_workflow_queue(workflow_queue, commander_to_switch).await
}

async fn log_to_user(
    uuid: u128,
    level: NipartLogLevel,
    message: String,
    sender: &Sender<NipartEvent>,
) {
    let event = NipartLogEntry::new(level, message)
        .to_event(uuid, NipartEventAddress::Commander);
    if let Err(e) = sender.send(event).await {
        log::warn!("Failed to send log {e}");
    }
}
