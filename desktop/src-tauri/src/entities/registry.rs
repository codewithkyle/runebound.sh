use std::collections::HashMap;
use std::sync::Arc;

use super::domain::EntityDomain;
use super::domains::{EventDomain, FactionDomain, ItemDomain, LocationDomain, NpcDomain};
use super::kind::EntityKind;

pub struct EntityDomainRegistry {
    domains: HashMap<EntityKind, Arc<dyn EntityDomain>>,
}

impl EntityDomainRegistry {
    pub fn new() -> Self {
        Self {
            domains: HashMap::new(),
        }
    }

    pub fn register(&mut self, domain: Arc<dyn EntityDomain>) {
        self.domains.insert(domain.kind(), domain);
    }

    pub fn domain(&self, kind: EntityKind) -> Option<Arc<dyn EntityDomain>> {
        self.domains.get(&kind).cloned()
    }

    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = (EntityKind, Arc<dyn EntityDomain>)> + '_ {
        self.domains
            .iter()
            .map(|(kind, domain)| (*kind, domain.clone()))
    }
}

pub fn build_default_registry() -> EntityDomainRegistry {
    let mut registry = EntityDomainRegistry::new();
    registry.register(Arc::new(NpcDomain::new()));
    registry.register(Arc::new(LocationDomain::new()));
    registry.register(Arc::new(FactionDomain::new()));
    registry.register(Arc::new(ItemDomain::new()));
    registry.register(Arc::new(EventDomain::new()));
    registry
}
