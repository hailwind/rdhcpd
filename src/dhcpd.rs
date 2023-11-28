use crate::config::Config;
use crate::options;
use crate::packet;
use crate::server;
use crate::utils;

use duration_str::parse;
use mac_address::MacAddress;
use serde::{Deserialize, Serialize};
use serde_json;

use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter};
use std::net::Ipv4Addr;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

const INFINITE_LEASE: u128 = 1000 * 86400 * 365; //10 years as ms

#[derive(Deserialize, Serialize, Debug, Clone)]
struct Lease {
    mac: [u8; 6],
    expiry: u128,
}
impl Lease {
    pub fn new(mac: [u8; 6], expiry: u128) -> Lease {
        Lease { mac, expiry }
    }
}

#[derive(Debug)]
pub struct Dhcpd {
    conf: Config,
    leases: HashMap<Ipv4Addr, Lease>,
    last_lease: u32,
    lease_duration: Duration,
}
impl Dhcpd {
    pub fn new(conf: Config) -> Dhcpd {
        let (hm, last_lease) = load_leases(
            conf.lease_static.as_str(),
            conf.lease_file.as_str(),
            conf.start.into(),
            conf.end.into(),
        );
        let ll = conf.lease_time.clone();
        if hm.is_ok() {
            // println!("loaded leases count: {}", hm.len());
            Dhcpd {
                conf,
                leases: hm.unwrap(),
                last_lease: last_lease,
                lease_duration: parse(ll.as_str()).unwrap(),
            }
        } else {
            Dhcpd {
                conf,
                leases: HashMap::new(),
                last_lease: 0,
                lease_duration: parse(ll.as_str()).unwrap(),
            }
        }
    }
    fn start_num(&self) -> u32 {
        self.conf.start.into()
    }
    fn end_num(&self) -> u32 {
        self.conf.end.into()
    }
    fn lease_nums(&self) -> u32 {
        self.end_num() - self.start_num()
    }
    fn subnet_mask(&self) -> Ipv4Addr {
        self.conf.netmask
    }
    fn gateway_ip(&self) -> Ipv4Addr {
        self.conf.gateway
    }
    fn dns_servers(&self) -> Vec<Ipv4Addr> {
        self.conf.dns_servers.clone()
    }
    fn lease_secs(&self) -> u32 {
        self.lease_duration.as_secs() as u32
    }
    fn available(&self, chaddr: &[u8; 6], addr: &Ipv4Addr) -> bool {
        let pos: u32 = (*addr).into();
        pos >= self.start_num()
            && pos < self.start_num() + self.lease_nums()
            && match self.leases.get(addr) {
                Some(lease) => lease.mac == *chaddr || utils::now_timestamp_ms() > lease.expiry,
                None => true,
            }
    }
    fn current_lease(&self, chaddr: &[u8; 6]) -> Option<Ipv4Addr> {
        for (i, v) in &self.leases {
            if v.mac == *chaddr {
                return Some(*i);
            }
        }
        None
    }
    fn save_leases(&self) {
        if let Ok(file) = File::create(self.conf.lease_file.as_str()) {
            let writer = BufWriter::new(file);
            let r = serde_json::to_writer(writer, &self.leases);
            if r.is_err() {
                println!("ERROR: {:?}", r);
            } else {
                println!("save leases to {} success.", self.conf.lease_file.as_str());
            }
        }
    }
    fn nak(&self, s: &server::Server, req_packet: packet::Packet, message: &str) {
        let _ = s.reply(
            options::MessageType::Nak,
            vec![options::DhcpOption::Message(message.to_string())],
            Ipv4Addr::new(0, 0, 0, 0),
            req_packet,
        );
    }
    fn reply(
        &self,
        s: &server::Server,
        msg_type: options::MessageType,
        req_packet: packet::Packet,
        offer_ip: &Ipv4Addr,
    ) {
        let _ = s.reply(
            msg_type,
            vec![
                options::DhcpOption::IpAddressLeaseTime(self.lease_secs()),
                options::DhcpOption::SubnetMask(self.subnet_mask()),
                options::DhcpOption::Router(vec![self.gateway_ip()]),
                options::DhcpOption::DomainNameServer(self.dns_servers()),
            ],
            *offer_ip,
            req_packet,
        );
    }
}

impl server::Handler for Dhcpd {
    fn handle_request(&mut self, server: &server::Server, in_packet: packet::Packet) {
        match in_packet.message_type() {
            Ok(options::MessageType::Discover) => {
                // Otherwise prefer existing (including expired if available)
                if let Some(ip) = self.current_lease(&in_packet.chaddr) {
                    println!("Sending Reply to discover");
                    self.reply(server, options::MessageType::Offer, in_packet, &ip);
                    return;
                }
                // Otherwise choose a free ip if available
                for _ in 0..self.lease_nums() {
                    self.last_lease = (self.last_lease + 1) % self.lease_nums();
                    let off_ip = (self.start_num() + &self.last_lease).into();
                    if self.available(&in_packet.chaddr, &off_ip) {
                        println!("{:?} is available, send to discover", off_ip);
                        self.reply(server, options::MessageType::Offer, in_packet, &off_ip);
                        break;
                    }
                }
            }

            Ok(options::MessageType::Request) => {
                // Ignore requests to alternative DHCP server
                if !server.for_this_server(&in_packet) {
                    //println!("Not for this server");
                    // return;
                }

                let req_ip = match in_packet.option(options::REQUESTED_IP_ADDRESS) {
                    Some(options::DhcpOption::RequestedIpAddress(x)) => *x,
                    _ => in_packet.ciaddr,
                };
                // for (ip, (mac, _)) in &self.leases {
                //     println!("IP: {:?}, MAC: {:?}", ip, mac);
                // }
                if let Some(ip) = self.current_lease(&in_packet.chaddr) {
                    println!("Found Current Lease: {:?}", &ip);
                    self.reply(server, options::MessageType::Ack, in_packet, &ip);
                    return;
                }
                if !&self.available(&in_packet.chaddr, &req_ip) {
                    println!("Sending Reply by Request Msg for 'Requested IP not available'");
                    self.nak(server, in_packet, "Requested IP not available");
                    return;
                }
                println!("insert into leases: {:?}", req_ip);
                self.leases.insert(
                    req_ip,
                    Lease::new(in_packet.chaddr, utils::now_timestamp_ms()),
                );
                self.save_leases();
                println!("Sending Reply by Request Msg for {:?}", &req_ip);
                self.reply(server, options::MessageType::Ack, in_packet, &req_ip);
            }

            Ok(options::MessageType::Release) | Ok(options::MessageType::Decline) => {
                // Ignore requests to alternative DHCP server
                if !server.for_this_server(&in_packet) {
                    return;
                }
                if let Some(ip) = self.current_lease(&in_packet.chaddr) {
                    self.leases.remove(&ip);
                    self.save_leases();
                }
            }

            // TODO - not necessary but support for dhcp4r::INFORM might be nice
            _ => {}
        }
    }
}

fn load_leases(
    leases_static: &str,
    leases_file: &str,
    start: u32,
    end: u32,
) -> (Result<HashMap<Ipv4Addr, Lease>, Box<dyn Error>>, u32) {
    let mut leases: HashMap<Ipv4Addr, Lease> = HashMap::new();
    let mut last_lease = 0;

    if Path::new(leases_file).exists() {
        if let Ok(lf) = File::open(leases_file) {
            let reader = BufReader::new(lf);
            if let Ok(obj) = serde_json::from_reader(reader) {
                leases.clone_from(&obj);
            }
        }
    }

    for (k, _) in &leases {
        let ux: u32 = k.clone().into();
        if ux > last_lease && ux > start && ux < end {
            last_lease = ux;
        }
    }
    if Path::new(leases_static).exists() {
        if let Ok(file) = File::open(leases_static) {
            let sreader = BufReader::new(file);
            for line in sreader.lines() {
                if let Ok(line) = line {
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() == 2 {
                        let mac = MacAddress::from_str(parts[0]).unwrap();
                        let ip = parts[1].trim().parse::<Ipv4Addr>().unwrap();
                        leases.insert(
                            ip,
                            Lease::new(mac.bytes(), utils::now_timestamp_ms() + INFINITE_LEASE),
                        );
                    }
                }
            }
        }
    }

    (Ok(leases), last_lease)
}
