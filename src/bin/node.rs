use std::collections::HashMap;

#[macro_use]
extern crate clap;
extern crate config;
extern crate log;
extern crate mynode;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate simplelog;

fn main() -> Result<(), mynode::Error> {
    let args = get_app_args();
    let cfg = Config::new(args.value_of("config").unwrap())?;
    setup_log(&cfg)?;
    mynode::Node {
        peers: cfg.parse_peers()?,
        id: cfg.id,
        addr: cfg.listen,
        threads: cfg.threads,
        data_dir: cfg.data_dir,
    }
    .listen()
}

fn get_app_args() -> clap::ArgMatches<'static> {
    app_from_crate!()
        .arg(
            clap::Arg::with_name("config")
                .short("c")
                .long("config")
                .help("Configuration file path")
                .takes_value(true)
                .default_value("/etc/node.yaml"),
        )
        .get_matches()
}

fn setup_log(cfg: &Config) -> Result<(), mynode::Error> {
    let log_level = cfg.log_level.parse::<simplelog::LevelFilter>()?;

    let mut log_config = simplelog::ConfigBuilder::new();
    if log_level != simplelog::LevelFilter::Debug {
        log_config.add_filter_allow_str("mynode");
    }

    simplelog::SimpleLogger::init(log_level, log_config.build())?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct Config {
    id: String,
    listen: String,
    threads: usize,
    log_level: String,
    data_dir: String,
    peers: HashMap<String, String>,
}

impl Config {
    fn new(file: &str) -> Result<Self, config::ConfigError> {
        let mut c = config::Config::new();
        c.set_default("id", "node")?;
        c.set_default("listen", "0.0.0.0:9605")?;
        c.set_default("threads", 4)?;
        c.set_default("log_level", "info")?;
        c.set_default("data_dir", "/var/lib/nodedb")?;

        c.merge(config::File::with_name(file))?;
        c.merge(config::Environment::with_prefix("NODE"))?;
        c.try_into()
    }

    fn parse_peers(&self) -> Result<HashMap<String, std::net::SocketAddr>, mynode::Error> {
        let mut peers = HashMap::new();
        for (id, address) in self.peers.iter() {
            peers.insert(
                id.clone(),
                if let Ok(sa) = address.parse::<std::net::SocketAddr>() {
                    sa
                } else {
                    let ip = address.parse::<std::net::IpAddr>()?;
                    std::net::SocketAddr::new(ip, 9605)
                },
            );
        }
        Ok(peers)
    }
}
