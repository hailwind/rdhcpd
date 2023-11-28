use serde::Deserialize;

use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::net::Ipv4Addr;
use std::path::Path;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub intf: String,
    pub listen_addr: Ipv4Addr,
    pub start: Ipv4Addr,
    pub end: Ipv4Addr,
    pub netmask: Ipv4Addr,
    pub broadcast: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns_servers: Vec<Ipv4Addr>,
    pub lease_static: String,
    pub lease_file: String,
    pub lease_time: String,
}

pub fn read_config<P: AsRef<Path>>(path: P) -> Result<Config, Box<dyn Error>> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let c = serde_yaml::from_reader(reader)?;
    Ok(c)
}
