use crate::handler::ProtocolsChange;
use std::collections::HashSet;

#[derive(Default, Clone, Debug)]
pub struct SupportedProtocols {
    protocols: HashSet<String>,
}

impl SupportedProtocols {
    pub fn on_protocols_change(&mut self, change: ProtocolsChange) {
        match change {
            ProtocolsChange::Added(added) => {
                self.protocols.extend(added.cloned());
            }
            ProtocolsChange::Removed(removed) => {
                for p in removed {
                    self.protocols.remove(p);
                }
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.protocols.iter()
    }
}
