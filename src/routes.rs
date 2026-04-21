use std::sync::Arc;

use dashmap::DashMap;
use rkyv::{Archive, Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Archive, Deserialize, Serialize)]
pub struct Route {
    pub port: u16,
    pub pid: u32,
}

// type Hostname = String;
type Hostname = Arc<str>;

pub struct RouteManager {
    routes: DashMap<Hostname, Route>,
}

impl RouteManager {
    pub fn new() -> Self {
        Self {
            routes: DashMap::new(),
        }
    }

    pub fn insert(&self, hostname: Hostname, route: Route) {
        self.routes.insert(hostname, route);
    }

    pub fn get(&self, hostname: &str) -> Option<Route> {
        self.routes.get(hostname).map(|r| r.clone())
    }

    pub fn list(&self) -> Vec<(String, Route)> {
        self.routes
            .iter()
            .map(|val| (val.key().to_string(), val.value().clone()))
            .collect()
    }

    #[allow(dead_code)]
    pub fn update(&self, hostname: &str, route: Route) -> Result<(), &'static str> {
        if let Some(mut existing) = self.routes.get_mut(hostname) {
            *existing = route;
            Ok(())
        } else {
            Err("Hostname not found")
        }
    }

    pub fn remove(&self, hostname: &str) {
        self.routes.remove(hostname);
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_new_route_manager_is_empty() {
        let manager = RouteManager::new();
        assert!(manager.list().is_empty());
    }

    #[tokio::test]
    async fn test_insert_and_get_route() {
        let manager = RouteManager::new();
        let route1 = Route {
            port: 8080,
            pid: 1234,
        };
        let hostname1: Arc<str> = Arc::from("localhost");

        manager.insert(hostname1.clone(), route1.clone());

        let retrieved_route = manager.get(hostname1.as_ref()).expect("Route should be found");
        assert_eq!(retrieved_route, route1);

        let non_existent_route = manager.get("nonexistent.com");
        assert!(non_existent_route.is_none());
    }

    #[tokio::test]
    async fn test_list_routes() {
        let manager = RouteManager::new();
        let route1 = Route { port: 8080, pid: 1 };
        let route2 = Route { port: 9090, pid: 2 };
        let hostname2: Arc<str> = Arc::from("host2.local");
        let hostname1: Arc<str> = Arc::from("host1.local");

        manager.insert(hostname1.clone(), route1.clone());
        manager.insert(hostname2.clone(), route2.clone());

        let routes_list = manager.list();
        assert_eq!(routes_list.len(), 2);

        let mut routes_map: HashMap<String, Route> = routes_list.into_iter().collect();

        assert_eq!(routes_map.remove(hostname1.as_ref()).unwrap(), route1);
        assert_eq!(routes_map.remove(hostname2.as_ref()).unwrap(), route2);
        assert!(routes_map.is_empty());
    }

    #[tokio::test]
    async fn test_update_route() {
        let manager = RouteManager::new();
        let initial_route = Route { port: 8080, pid: 1234 };
        let hostname: Arc<str> = Arc::from("test.com");

        manager.insert(hostname.clone(), initial_route);

        let updated_route = Route { port: 8081, pid: 9999 };
        let result = manager.update(hostname.as_ref(), updated_route.clone());
        assert!(result.is_ok());

        let retrieved_route = manager.get(hostname.as_ref()).expect("Updated route should be found");
        assert_eq!(retrieved_route, updated_route);

        let non_existent_update_result = manager.update("nonexistent.com", Route { port: 1000, pid: 0 });
        assert!(non_existent_update_result.is_err());
        assert_eq!(non_existent_update_result.unwrap_err(), "Hostname not found");
    }

    #[tokio::test]
    async fn test_remove_route() {
        let manager = RouteManager::new();
        let route1 = Route {
            port: 8080,
            pid: 1234,
        };
        let hostname1: Arc<str> = Arc::from("localhost");

        manager.insert(hostname1.clone(), route1);
        assert!(manager.get(hostname1.as_ref()).is_some());
        assert_eq!(manager.list().len(), 1);

        manager.remove(hostname1.as_ref());
        assert!(manager.get(hostname1.as_ref()).is_none());
        assert!(manager.list().is_empty());

        manager.remove("nonexistent.com");
        assert!(manager.list().is_empty());
    }

    #[tokio::test]
    async fn tryout_original() {
        let manager = RouteManager::new();

        let route1 = Route {
            port: 8080,
            pid: 1234,
        };
        let route2 = Route {
            port: 9090,
            pid: 5678,
        };

        manager.insert(Arc::from("localhost"), route1);
        manager.insert(Arc::from("example.com"), route2);

        if let Some(route) = manager.get("localhost") {
            assert_eq!(
                route,
                Route {
                    port: 8080,
                    pid: 1234
                }
            );
            println!("Route for localhost: {:?}", route);
        }

        let updated_route = Route { port: 8081, pid: 9999 };
        match manager.update("localhost", updated_route.clone()) {
            Ok(()) => println!("Updated route: {:?}", updated_route), // Print updated_route, not route
            Err(err) => println!("Error: {}", err),
        }

        assert!(
            manager.get("localhost").unwrap()
                == Route {
                    port: 8081,
                    pid: 9999
                }
        );
    }
}
