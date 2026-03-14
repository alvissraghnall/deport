use std::sync::Arc;

use dashmap::DashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
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

    #[tokio::test]
    async fn tryout() {
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
        manager.insert("example.com".into(), route2);

        if let Some(route) = manager.get(&"localhost".to_string()) {
            assert_eq!(
                route,
                Route {
                    port: 8080,
                    pid: 1234
                }
            );
            println!("Route for localhost: {:?}", route);
        }

        let updated_route = Route {
            port: 8081,
            pid: 9999,
        };
        match manager
            .update(&"localhost".to_string(), updated_route)
        {
            Ok(route) => println!("Updated route: {:?}", route),
            Err(err) => println!("Error: {}", err),
        }

        assert!(
            manager.get(&"localhost".to_string()).unwrap()
                == Route {
                    port: 8081,
                    pid: 9999
                }
        );
    }
}
