use crate::handler::ProtocolsChange;
use crate::StreamProtocol;
use std::collections::HashSet;

#[derive(Default, Clone, Debug)]
pub struct SupportedProtocols {
    protocols: HashSet<StreamProtocol>,
}

impl SupportedProtocols {
    pub fn on_protocols_change(&mut self, change: ProtocolsChange) -> bool {
        match change {
            ProtocolsChange::Added(added) => {
                let mut changed = false;

                for p in added {
                    changed |= self.protocols.insert(p.clone());
                }

                changed
            }
            ProtocolsChange::Removed(removed) => {
                let mut changed = false;

                for p in removed {
                    changed |= self.protocols.remove(p);
                }

                changed
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &StreamProtocol> {
        self.protocols.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handler::{ProtocolsAdded, ProtocolsRemoved};
    use once_cell::sync::Lazy;

    #[test]
    fn protocols_change_added_returns_correct_changed_value() {
        let mut protocols = SupportedProtocols::default();

        let changed = protocols.on_protocols_change(add_foo());
        assert!(changed);

        let changed = protocols.on_protocols_change(add_foo());
        assert!(!changed);

        let changed = protocols.on_protocols_change(add_foo_bar());
        assert!(changed);
    }

    #[test]
    fn protocols_change_removed_returns_correct_changed_value() {
        let mut protocols = SupportedProtocols::default();

        let changed = protocols.on_protocols_change(remove_foo());
        assert!(!changed);

        protocols.on_protocols_change(add_foo());

        let changed = protocols.on_protocols_change(remove_foo());
        assert!(changed);
    }

    fn add_foo() -> ProtocolsChange<'static> {
        ProtocolsChange::Added(ProtocolsAdded::from_set(&FOO_PROTOCOLS))
    }

    fn add_foo_bar() -> ProtocolsChange<'static> {
        ProtocolsChange::Added(ProtocolsAdded::from_set(&FOO_BAR_PROTOCOLS))
    }

    fn remove_foo() -> ProtocolsChange<'static> {
        ProtocolsChange::Removed(ProtocolsRemoved::from_set(&FOO_PROTOCOLS))
    }

    static FOO_PROTOCOLS: Lazy<HashSet<StreamProtocol>> =
        Lazy::new(|| HashSet::from([StreamProtocol::new("/foo")]));
    static FOO_BAR_PROTOCOLS: Lazy<HashSet<StreamProtocol>> =
        Lazy::new(|| HashSet::from([StreamProtocol::new("/foo"), StreamProtocol::new("/bar")]));
}
