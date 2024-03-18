// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use crate::{
    MergedNetworkState, NetworkState, NipartApplyOption, NipartDhcpConfig,
    NipartDhcpLease, NipartLogLevel, NipartMonitorEvent, NipartMonitorRule,
    NipartQueryOption,
};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct NipartPluginInfo {
    pub name: String,
    pub roles: Vec<NipartRole>,
}

#[derive(
    Serialize,
    Deserialize,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
)]
#[non_exhaustive]
pub enum NipartRole {
    Dhcp,
    QueryAndApply,
    ApplyDhcpLease,
    Ovs,
    Lldp,
    Monitor,
    Config,
}

impl std::fmt::Display for NipartRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Dhcp => "dhcp",
                Self::QueryAndApply => "query_and_apply",
                Self::Ovs => "ovs",
                Self::Lldp => "lldp",
                Self::Monitor => "monitor",
                Self::Config => "config",
                Self::ApplyDhcpLease => "apply_dhcp_lease",
            }
        )
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub enum NipartPluginEvent {
    #[default]
    None,
    Quit,

    QueryPluginInfo,
    QueryPluginInfoReply(NipartPluginInfo),

    ChangeLogLevel(NipartLogLevel),
    QueryLogLevel,
    QueryLogLevelReply(NipartLogLevel),

    QueryNetState(NipartQueryOption),
    QueryRelatedNetState(Box<NetworkState>),
    QueryNetStateReply(Box<NetworkState>, u32),

    ApplyNetState(Box<MergedNetworkState>, NipartApplyOption),
    ApplyNetStateReply,

    /// Empty `Vec<String>` means query all interfaces
    QueryDhcpConfig(Box<Vec<String>>),
    QueryDhcpConfigReply(Box<Vec<NipartDhcpConfig>>),

    ApplyDhcpConfig(Box<Vec<NipartDhcpConfig>>),
    ApplyDhcpConfigReply,

    /// DHCP plugin notify commander on new lease been acquired
    GotDhcpLease(Box<NipartDhcpLease>),
    /// Commander request responsible plugins to apply DHCP lease
    ApplyDhcpLease(Box<NipartDhcpLease>),
    ApplyDhcpLeaseReply,

    /// Register a monitor rule to plugin with monitor role.
    /// No reply required.
    RegisterMonitorRule(Box<NipartMonitorRule>),
    /// Remove a monitor rule from monitor plugin.
    /// No reply required.
    RemoveMonitorRule(Box<NipartMonitorRule>),
    /// Monitor plugin notify. No reply required.
    GotMonitorEvent(Box<NipartMonitorEvent>),
}

impl std::fmt::Display for NipartPluginEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "{}", "none"),
            Self::Quit => write!(f, "{}", "quit"),
            Self::QueryPluginInfo => write!(f, "{}", "query_plugin_info"),
            Self::QueryPluginInfoReply(_) => {
                write!(f, "{}", "query_plugin_info_reply")
            }
            Self::ChangeLogLevel(l) => write!(f, "change_log_level:{l}"),
            Self::QueryLogLevel => write!(f, "{}", "query_log_level"),
            Self::QueryLogLevelReply(_) => {
                write!(f, "{}", "query_log_level_reply")
            }
            Self::QueryNetState(_) => write!(f, "{}", "query_netstate"),
            Self::QueryNetStateReply(_, _) => {
                write!(f, "{}", "query_netstate_reply")
            }
            Self::QueryRelatedNetState(_) => {
                write!(f, "{}", "query_related_netstate")
            }
            Self::ApplyNetState(_, _) => write!(f, "{}", "apply_netstate"),
            Self::ApplyNetStateReply => write!(f, "{}", "apply_netstate_reply"),
            Self::QueryDhcpConfig(_) => write!(f, "{}", "query_dhcp_config"),
            Self::QueryDhcpConfigReply(_) => {
                write!(f, "{}", "query_dhcp_config_reply")
            }
            Self::ApplyDhcpConfig(configs) => write!(
                f,
                "apply_dhcp_config:{}",
                configs
                    .as_slice()
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<String>>()
                    .join(",")
            ),
            Self::ApplyDhcpConfigReply => {
                write!(f, "{}", "apply_dhcp_config_reply")
            }
            Self::GotDhcpLease(_) => write!(f, "{}", "got_dhcp_lease"),
            Self::ApplyDhcpLease(_) => write!(f, "{}", "apply_dhcp_lease"),
            Self::ApplyDhcpLeaseReply => {
                write!(f, "{}", "apply_dhcp_lease_reply")
            }
            Self::RegisterMonitorRule(rule) => {
                write!(f, "register_monitor_rule:{rule}")
            }
            Self::RemoveMonitorRule(rule) => {
                write!(f, "remove_monitor_rule:{rule}")
            }
            Self::GotMonitorEvent(event) => {
                write!(f, "got_monitor_event:{event}")
            }
        }
    }
}

impl NipartPluginEvent {
    pub fn is_reply(&self) -> bool {
        matches!(
            self,
            Self::QueryPluginInfoReply(_)
                | Self::QueryLogLevelReply(_)
                | Self::QueryNetStateReply(_, _)
                | Self::ApplyNetStateReply
                | Self::QueryDhcpConfigReply(_)
                | Self::ApplyDhcpConfigReply
                | Self::ApplyDhcpLeaseReply
                | Self::GotMonitorEvent(_)
        )
    }
}
