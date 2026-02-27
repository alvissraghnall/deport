use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Route {
    pub port: u16,
    pub pid: u32,
}

type Hostname = String;
type Routes = HashMap<Hostname, Route>;

type Data = Arc<RwLock<Routes>>;

#[derive(Clone)]
pub struct RouteManager {
    data: Data,
}

impl RouteManager {
    pub fn new() -> Self {
        RouteManager {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // Insert a new route or update an existing one
    pub async fn insert(&self, hostname: Hostname, route: Route) {
        let mut data = self.data.write().await;
        data.insert(hostname, route);
    }

    // Get a route by hostname, returning an owned value
    pub async fn get(&self, hostname: &Hostname) -> Option<Route> {
        let data = self.data.read().await; // shared lock for reading
        data.get(hostname).cloned() // return an owned value
    }

    // Update a route and return the updated route (or a default in case of error)
    pub async fn update(&self, hostname: &Hostname, route: Route) -> Result<Route, &'static str> {
        let mut data = self.data.write().await;
        if let Some(existing_route) = data.get_mut(hostname) {
            *existing_route = route;
            Ok(existing_route.clone())
        } else {
            Err("Hostname not found")
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn tryout () {
        let manager = RouteManager::new();

        let route1 = Route {
            port: 8080,
            pid: 1234,
        };
        let route2 = Route {
            port: 9090,
            pid: 5678,
        };

        // Insert routes
        manager.insert("localhost".to_string(), route1).await;
        manager.insert("example.com".to_string(), route2).await;

        // Retrieve a route
        if let Some(route) = manager.get(&"localhost".to_string()).await {
            
            assert_eq!(route, Route { port: 8080, pid: 1234 });
            println!("Route for localhost: {:?}", route);
        }

        // Update a route
        let updated_route = Route {
            port: 8081,
            pid: 9999,
        };
        match manager
            .update(&"localhost".to_string(), updated_route)
            .await
        {
            Ok(route) => println!("Updated route: {:?}", route),
            Err(err) => println!("Error: {}", err),
        }

        assert!(manager.get(&"localhost".to_string()).await.unwrap() == Route { port: 8081, pid: 9999 });
    }
}
