// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};

use crate::{
    state::json::get_json_value_difference, ErrorKind, Interface,
    InterfaceState, InterfaceType, Interfaces, MergedInterfaces,
    MergedOvsDbGlobalConfig, NetworkState, NipartError, OvsBridgeBondConfig,
    OvsBridgeConfig, OvsBridgeInterface, OvsDbGlobalConfig, OvsDbIfaceConfig,
    OvsInterface,
};

impl MergedOvsDbGlobalConfig {
    pub(crate) fn is_changed(&self) -> bool {
        self.is_changed
    }

    pub(crate) fn verify(
        &self,
        current: OvsDbGlobalConfig,
    ) -> Result<(), NipartError> {
        let desired = match self.desired.as_ref() {
            Some(d) => d,
            None => {
                return Ok(());
            }
        };
        let empty_map: HashMap<String, Option<String>> = HashMap::new();
        let external_ids: HashMap<String, Option<String>> = desired
            .external_ids
            .as_ref()
            .unwrap_or(&empty_map)
            .iter()
            .filter_map(|(k, v)| {
                if v.is_some() {
                    Some((k.to_string(), v.clone()))
                } else {
                    None
                }
            })
            .collect();
        let other_config: HashMap<String, Option<String>> = desired
            .other_config
            .as_ref()
            .unwrap_or(&empty_map)
            .iter()
            .filter_map(|(k, v)| {
                if v.is_some() {
                    Some((k.to_string(), v.clone()))
                } else {
                    None
                }
            })
            .collect();

        let desired = OvsDbGlobalConfig {
            external_ids: Some(external_ids),
            other_config: Some(other_config),
        };

        let desired_value = serde_json::to_value(desired)?;
        let current_value = if current.is_none() {
            serde_json::to_value(OvsDbGlobalConfig {
                external_ids: Some(HashMap::new()),
                other_config: Some(HashMap::new()),
            })?
        } else {
            serde_json::to_value(current)?
        };

        if let Some((reference, desire, current)) = get_json_value_difference(
            "ovsdb".to_string(),
            &desired_value,
            &current_value,
        ) {
            Err(NipartError::new(
                ErrorKind::VerificationError,
                format!(
                    "Verification failure: {reference} desire '{desire}', \
                    current '{current}'"
                ),
            ))
        } else {
            Ok(())
        }
    }
}

impl OvsBridgeConfig {
    pub(crate) fn update(&mut self, other: Option<&OvsBridgeConfig>) {
        if let Some(other) = other {
            self.ports.clone_from(&other.ports);
        }
    }
}

impl OvsBridgeInterface {
    pub(crate) fn sanitize_current_for_verify(&mut self) {
        if let Some(port_confs) = self
            .bridge
            .as_mut()
            .and_then(|br_conf| br_conf.ports.as_mut())
        {
            for port_conf in port_confs {
                if let Some(bond_conf) = port_conf.bond.as_mut() {
                    bond_conf.sanitize_current_for_verify();
                }
            }
        }
    }

    pub(crate) fn update_ovs_bridge(&mut self, other: &OvsBridgeInterface) {
        if let Some(br_conf) = &mut self.bridge {
            br_conf.update(other.bridge.as_ref());
        } else {
            self.bridge.clone_from(&other.bridge);
        }
    }
}

impl OvsInterface {
    pub(crate) fn update_ovs_iface(&mut self, other: &Self) {
        if other.patch.is_some() {
            self.patch.clone_from(&other.patch);
        }
        if other.dpdk.is_some() {
            self.dpdk.clone_from(&other.dpdk);
        }
    }
}

impl OvsDbIfaceConfig {
    pub(crate) fn new_empty() -> Self {
        Self {
            external_ids: Some(HashMap::new()),
            other_config: Some(HashMap::new()),
        }
    }
}

impl OvsBridgeBondConfig {
    pub(crate) fn sanitize_current_for_verify(&mut self) {
        // None ovsbd equal to empty
        if self.ovsdb.is_none() {
            self.ovsdb = Some(OvsDbIfaceConfig::new_empty());
        }
    }
}
impl MergedInterfaces {
    // This function remove extra(undesired) ovs patch port from post-apply
    // current, so it will not interfere with verification
    pub(crate) fn process_allow_extra_ovs_patch_ports_for_verify(
        &mut self,
        current: &mut Interfaces,
    ) {
        let mut ovs_patch_port_names: HashSet<String> = HashSet::new();
        for cur_iface in current.iter().filter_map(|i| {
            if let Interface::OvsInterface(o) = i {
                Some(o)
            } else {
                None
            }
        }) {
            if cur_iface.is_ovs_patch_port() {
                ovs_patch_port_names.insert(cur_iface.base.name.to_string());
            }
        }

        for des_iface in self.iter_mut().filter_map(|i| {
            if let Some(Interface::OvsBridge(o)) = i.desired.as_mut() {
                if o.bridge.as_ref().and_then(|c| c.allow_extra_patch_ports)
                    == Some(true)
                    && o.base.state == InterfaceState::Up
                {
                    // Remove allow_extra_patch_ports as current state
                    // does not have it
                    if let Some(Interface::OvsBridge(c)) = i.for_verify.as_mut()
                    {
                        if let Some(c) = c.bridge.as_mut() {
                            c.allow_extra_patch_ports = None;
                        }
                    }
                    Some(o)
                } else {
                    None
                }
            } else {
                None
            }
        }) {
            if let Some(cur_iface) = current.get_iface_mut(
                des_iface.base.name.as_str(),
                InterfaceType::OvsBridge,
            ) {
                let mut ports_to_delete: HashSet<String> = HashSet::new();
                if let (Some(des_ports), Some(cur_ports)) =
                    (des_iface.ports(), cur_iface.ports())
                {
                    for cur_port_name in cur_ports {
                        if ovs_patch_port_names.contains(cur_port_name)
                            && !des_ports.contains(&cur_port_name)
                        {
                            ports_to_delete.insert(cur_port_name.to_string());
                        }
                    }
                }
                for port_name in ports_to_delete.iter() {
                    cur_iface.remove_port(port_name);
                }
            }
        }
    }
}

impl NetworkState {
    pub(crate) fn isolate_ovn(&mut self) -> Result<(), NipartError> {
        if let Some(ovn_maps_str) = self
            .ovsdb
            .as_mut()
            .and_then(|o| o.external_ids.as_mut())
            .and_then(|eids| {
                eids.remove(OvsDbGlobalConfig::OVN_BRIDGE_MAPPINGS_KEY)
            })
            .flatten()
        {
            self.ovn = ovn_maps_str.as_str().try_into()?;
        }
        Ok(())
    }
}
