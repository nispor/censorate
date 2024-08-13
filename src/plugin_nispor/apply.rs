// SPDX-License-Identifier: Apache-2.0

use nipart::{
    ErrorKind, Interface, InterfaceType, MergedInterface, MergedInterfaces,
    MergedNetworkState, NipartApplyOption, NipartDhcpLease, NipartError,
};

use crate::{
    hostname::set_running_hostname,
    ip::{nipart_ipv4_to_np, nipart_ipv6_to_np},
    veth::nms_veth_conf_to_np,
    vlan::nms_vlan_conf_to_np,
};

pub(crate) async fn nispor_apply(
    merged_state: MergedNetworkState,
    _opt: NipartApplyOption,
) -> Result<(), NipartError> {
    if let Some(hostname) = merged_state
        .get_desired_hostname()
        .and_then(|c| c.running.as_ref())
    {
        set_running_hostname(hostname)?;
    }

    delete_ifaces(&merged_state.interfaces).await?;

    let mut ifaces: Vec<&MergedInterface> = merged_state
        .interfaces
        .iter()
        .filter(|i| i.is_changed())
        .collect();

    ifaces.sort_unstable_by_key(|iface| iface.merged.name());
    // Use sort_by_key() instead of unstable one, do we can alphabet
    // activation order which is required to simulate the OS boot-up.
    ifaces.sort_by_key(|iface| {
        if let Some(i) = iface.for_apply.as_ref() {
            i.base_iface().up_priority
        } else {
            u32::MAX
        }
    });

    let mut np_ifaces: Vec<nispor::IfaceConf> = Vec::new();
    for merged_iface in ifaces.iter().filter(|i| {
        i.merged.iface_type() != InterfaceType::Unknown && !i.merged.is_absent()
    }) {
        np_ifaces.push(nipart_iface_to_np(merged_iface)?);
    }

    // TODO: Purge DHCP/autoconf IP/routes if DHCP/autoconf disabled

    let mut net_conf = nispor::NetConf::default();
    net_conf.ifaces = Some(np_ifaces);

    if let Err(e) = net_conf.apply_async().await {
        Err(NipartError::new(
            ErrorKind::PluginFailure,
            format!("Unknown error from nipsor plugin: {}, {}", e.kind, e.msg),
        ))
    } else {
        Ok(())
    }
}

fn nipart_iface_type_to_np(
    nms_iface_type: &InterfaceType,
) -> nispor::IfaceType {
    match nms_iface_type {
        InterfaceType::LinuxBridge => nispor::IfaceType::Bridge,
        InterfaceType::Bond => nispor::IfaceType::Bond,
        InterfaceType::Ethernet => nispor::IfaceType::Ethernet,
        InterfaceType::Veth => nispor::IfaceType::Veth,
        InterfaceType::Vlan => nispor::IfaceType::Vlan,
        _ => nispor::IfaceType::Unknown,
    }
}

fn nipart_iface_to_np(
    merged_iface: &MergedInterface,
) -> Result<nispor::IfaceConf, NipartError> {
    let mut np_iface = nispor::IfaceConf::default();

    let for_apply = match merged_iface.for_apply.as_ref() {
        Some(i) => i,
        None => {
            return Err(NipartError::new(
                ErrorKind::Bug,
                format!(
                    "nipart_iface_to_np() got MergedInterface with \
                    for_apply set to None: {merged_iface:?}"
                ),
            ));
        }
    };

    let mut np_iface_type = nipart_iface_type_to_np(&for_apply.iface_type());

    if let Interface::Ethernet(iface) = for_apply {
        if iface.veth.is_some() {
            np_iface_type = nispor::IfaceType::Veth;
        }
    }

    np_iface.name = for_apply.name().to_string();
    np_iface.iface_type = Some(np_iface_type);
    if for_apply.is_absent() {
        np_iface.state = nispor::IfaceState::Absent;
        return Ok(np_iface);
    }

    np_iface.state = nispor::IfaceState::Up;

    let base_iface = &for_apply.base_iface();
    if let Some(ctrl_name) = &base_iface.controller {
        np_iface.controller = Some(ctrl_name.to_string())
    }
    if base_iface.can_have_ip() {
        np_iface.ipv4 = Some(nipart_ipv4_to_np(base_iface.ipv4.as_ref()));
        np_iface.ipv6 = Some(nipart_ipv6_to_np(base_iface.ipv6.as_ref()));
    }

    np_iface.mac_address = base_iface.mac_address.clone();

    if let Interface::Ethernet(eth_iface) = for_apply {
        np_iface.veth = nms_veth_conf_to_np(eth_iface.veth.as_ref());
    } else if let Interface::Vlan(vlan_iface) = &merged_iface.merged {
        np_iface.vlan = nms_vlan_conf_to_np(vlan_iface.vlan.as_ref());
    }

    Ok(np_iface)
}

async fn delete_ifaces(
    merged_ifaces: &MergedInterfaces,
) -> Result<(), NipartError> {
    let mut deleted_veths: Vec<&str> = Vec::new();
    let mut np_ifaces: Vec<nispor::IfaceConf> = Vec::new();
    for iface in merged_ifaces
        .kernel_ifaces
        .values()
        .filter(|i| i.merged.is_absent())
    {
        // Deleting one end of veth peer is enough
        if deleted_veths.contains(&iface.merged.name()) {
            continue;
        }

        if let Some(Interface::Ethernet(eth_iface)) = &iface.current {
            if let Some(peer_name) = eth_iface
                .veth
                .as_ref()
                .map(|veth_conf| veth_conf.peer.as_str())
            {
                deleted_veths.push(eth_iface.base.name.as_str());
                deleted_veths.push(peer_name);
            }
        }
        log::debug!("Deleting interface {}", iface.merged.name());
        np_ifaces.push(nipart_iface_to_np(iface)?);
    }

    let mut net_conf = nispor::NetConf::default();
    net_conf.ifaces = Some(np_ifaces);

    if let Err(e) = net_conf.apply_async().await {
        Err(NipartError::new(
            ErrorKind::PluginFailure,
            format!("Unknown error from nipsor plugin: {}, {}", e.kind, e.msg),
        ))
    } else {
        Ok(())
    }
}

pub(crate) async fn nispor_apply_dhcp_lease(
    lease: NipartDhcpLease,
) -> Result<(), NipartError> {
    match lease {
        NipartDhcpLease::V4(lease) => {
            let mut net_conf = nispor::NetConf::default();
            let mut np_iface = nispor::IfaceConf::default();
            np_iface.name = lease.iface.to_string();
            let mut ip_conf = nispor::IpConf::default();
            let mut ip_addr = nispor::IpAddrConf::default();
            ip_addr.address = lease.ip.to_string();
            ip_addr.prefix_len = lease.prefix_length;
            ip_addr.valid_lft = format!("{}sec", lease.lease_time);
            ip_addr.preferred_lft = format!("{}sec", lease.lease_time);
            // BUG: We should preserve existing IP address
            ip_conf.addresses.push(ip_addr);
            np_iface.ipv4 = Some(ip_conf);
            np_iface.state = nispor::IfaceState::Up;
            net_conf.ifaces = Some(vec![np_iface]);

            log::debug!("Plugin nispor apply {net_conf:?}");

            if let Err(e) = net_conf.apply_async().await {
                Err(NipartError::new(
                    ErrorKind::PluginFailure,
                    format!(
                        "Unknown error nispor apply_async: {}, {}",
                        e.kind, e.msg
                    ),
                ))
            } else {
                Ok(())
            }
        }
        NipartDhcpLease::V6(_) => {
            todo!()
        }
    }
}
