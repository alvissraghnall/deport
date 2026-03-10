use std::os;



pub fn handle_command (args: &[String]) {

    let cmd = &args[1];
}

struct ParsedRunArgs {
  force: bool,
  /** Fixed app port (overrides automatic assignment). */
  app_port: Option<u16>,
  /** Override the inferred base name (from --name flag). */
  name: Option<String>,
  /** The child command and its arguments, passed through untouched. */
  command_args: [String],
}

fn app_port_from_env () -> Option<u16> {
  let env_val = std::env::var("DEPORT_APP_PORT").ok();
  
  match env_val {
      Some(val) => {
          let port = val.parse::<u16>().ok();
              if port < Some(1 as u16) || port >= Some(65534) {
                  panic!("Error: Invalid DEPORT_APP_PORT={}. Must be 1-65535.", val);
              }
              return port;
      },
      None => {
          return None;
      }
  }
}