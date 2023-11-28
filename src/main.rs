mod args;
mod config;
mod dhcpd;
mod options;
mod packet;
mod server;
mod utils;

use args::Args;

use std::net::UdpSocket;
use std::path::Path;
use std::process::exit;

use crate::dhcpd::Dhcpd;
use crate::server::Server;

fn main() -> anyhow::Result<()> {
    let Args { cfg } = Args::parse_args();
    let cfgfile = Path::new(&cfg);
    if !cfgfile.exists() {
        println!("Cfg File {} Not Exists.", cfg);
        exit(1)
    }
    let conf = config::read_config(cfg).unwrap();
    println!("conf: {:?}", conf);

    let socket = UdpSocket::bind("0.0.0.0:67").unwrap();
    socket.set_broadcast(true).unwrap();
    let dhcpd = Dhcpd::new(conf.clone());

    Server::serve(
        socket,
        conf.listen_addr.clone(),
        conf.broadcast.clone(),
        dhcpd,
    );

    Ok(())
}
