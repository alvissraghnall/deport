use crate::cli_utils::format_url;

pub(crate) fn handle_list (routes_manager: &crate::routes::RouteManager, tls: bool) -> anyhow::Result<()> {
    list_routes(routes_manager, tls);
    Ok(())
}

fn list_routes(routes_manager: &crate::routes::RouteManager, tls: bool) {

    let list = routes_manager.list();
    if list.is_empty() {
        println!("No active routes found");
        return;
    }
    println!("Active routes:");
    for (hostname, route) in list {
        let url = format_url(hostname.as_str(), route.port,  tls);
        let pid_str = format!("pid {}", route.pid);
        let label = if route.pid == 0 { "inactive" } else { pid_str.as_str() };
        println!("{} -> {} ({})", url, format!("localhost:{}", route.port), label);
    }

}